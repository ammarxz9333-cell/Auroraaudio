use aurora_realtime_audio_api::AudioStreamFault;
use thiserror::Error;

/// Live duplex lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum DuplexStreamState {
    /// Both streams are stopped.
    #[default]
    Stopped = 0,
    /// Devices are selected and streams are being opened.
    Starting = 1,
    /// Input and output streams are running without a reported fault.
    Running = 2,
    /// Audio continues with an observable quality or saturation warning.
    Degraded = 3,
    /// A backend or engine fault requires both streams to stop.
    Faulted = 4,
    /// The control thread is reopening the exact selected devices.
    Recovering = 5,
    /// Both streams are being stopped.
    Stopping = 6,
}

/// Control-thread lifecycle event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplexStateEvent {
    /// Begin opening the selected devices.
    StartRequested,
    /// Both streams opened and started.
    StreamsStarted,
    /// Numeric quality threshold was crossed without stream loss.
    QualityDegraded,
    /// A callback/backend fault was observed.
    StreamFault(AudioStreamFault),
    /// Begin a bounded reopen attempt for the same selectors.
    RecoveryRequested,
    /// Reopen succeeded.
    RecoverySucceeded,
    /// Reopen failed.
    RecoveryFailed,
    /// Begin cooperative shutdown.
    StopRequested,
    /// Both streams stopped.
    StreamsStopped,
}

/// Invalid lifecycle transition.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
#[error("invalid duplex state transition from {state:?} using {event:?}")]
pub struct DuplexStateTransitionError {
    /// State before the rejected event.
    pub state: DuplexStreamState,
    /// Rejected event.
    pub event: DuplexStateEvent,
}

/// Deterministic control-thread duplex state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DuplexStateMachine {
    state: DuplexStreamState,
    fault: AudioStreamFault,
    recovery_attempts: u32,
}

impl DuplexStateMachine {
    /// Returns current lifecycle state.
    pub fn state(&self) -> DuplexStreamState {
        self.state
    }

    /// Returns the numeric stream fault that caused `Faulted`.
    pub fn fault(&self) -> AudioStreamFault {
        self.fault
    }

    /// Returns bounded recovery attempts performed by the control thread.
    pub fn recovery_attempts(&self) -> u32 {
        self.recovery_attempts
    }

    /// Applies one lifecycle event.
    pub fn transition(
        &mut self,
        event: DuplexStateEvent,
    ) -> Result<DuplexStreamState, DuplexStateTransitionError> {
        let next = match (self.state, event) {
            (DuplexStreamState::Stopped, DuplexStateEvent::StartRequested) => {
                DuplexStreamState::Starting
            }
            (DuplexStreamState::Starting, DuplexStateEvent::StreamsStarted) => {
                DuplexStreamState::Running
            }
            (DuplexStreamState::Running, DuplexStateEvent::QualityDegraded) => {
                DuplexStreamState::Degraded
            }
            (
                DuplexStreamState::Running | DuplexStreamState::Degraded,
                DuplexStateEvent::StreamFault(fault),
            ) => {
                self.fault = fault;
                DuplexStreamState::Faulted
            }
            (DuplexStreamState::Faulted, DuplexStateEvent::RecoveryRequested) => {
                self.recovery_attempts = self.recovery_attempts.saturating_add(1);
                DuplexStreamState::Recovering
            }
            (DuplexStreamState::Recovering, DuplexStateEvent::RecoverySucceeded) => {
                self.fault = AudioStreamFault::None;
                DuplexStreamState::Running
            }
            (DuplexStreamState::Recovering, DuplexStateEvent::RecoveryFailed) => {
                DuplexStreamState::Faulted
            }
            (
                DuplexStreamState::Starting
                | DuplexStreamState::Running
                | DuplexStreamState::Degraded
                | DuplexStreamState::Faulted
                | DuplexStreamState::Recovering,
                DuplexStateEvent::StopRequested,
            ) => DuplexStreamState::Stopping,
            (DuplexStreamState::Stopping, DuplexStateEvent::StreamsStopped) => {
                DuplexStreamState::Stopped
            }
            _ => {
                return Err(DuplexStateTransitionError {
                    state: self.state,
                    event,
                })
            }
        };
        self.state = next;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_start_degrade_fault_recover_stop_path_is_explicit() {
        let mut machine = DuplexStateMachine::default();
        assert_eq!(
            machine
                .transition(DuplexStateEvent::StartRequested)
                .unwrap(),
            DuplexStreamState::Starting
        );
        assert_eq!(
            machine
                .transition(DuplexStateEvent::StreamsStarted)
                .unwrap(),
            DuplexStreamState::Running
        );
        assert_eq!(
            machine
                .transition(DuplexStateEvent::QualityDegraded)
                .unwrap(),
            DuplexStreamState::Degraded
        );
        assert_eq!(
            machine
                .transition(DuplexStateEvent::StreamFault(AudioStreamFault::DeviceLost))
                .unwrap(),
            DuplexStreamState::Faulted
        );
        assert_eq!(machine.fault(), AudioStreamFault::DeviceLost);
        assert_eq!(
            machine
                .transition(DuplexStateEvent::RecoveryRequested)
                .unwrap(),
            DuplexStreamState::Recovering
        );
        assert_eq!(
            machine
                .transition(DuplexStateEvent::RecoverySucceeded)
                .unwrap(),
            DuplexStreamState::Running
        );
        assert_eq!(
            machine.transition(DuplexStateEvent::StopRequested).unwrap(),
            DuplexStreamState::Stopping
        );
        assert_eq!(
            machine
                .transition(DuplexStateEvent::StreamsStopped)
                .unwrap(),
            DuplexStreamState::Stopped
        );
        assert_eq!(machine.recovery_attempts(), 1);
    }

    #[test]
    fn device_loss_is_numeric_and_invalid_transition_is_rejected() {
        let mut machine = DuplexStateMachine::default();
        let error = machine
            .transition(DuplexStateEvent::StreamsStarted)
            .unwrap_err();
        assert_eq!(error.state, DuplexStreamState::Stopped);
        machine
            .transition(DuplexStateEvent::StartRequested)
            .unwrap();
        machine
            .transition(DuplexStateEvent::StreamsStarted)
            .unwrap();
        machine
            .transition(DuplexStateEvent::StreamFault(AudioStreamFault::Callback))
            .unwrap();
        assert_eq!(machine.fault(), AudioStreamFault::Callback);
    }
}
