use aurora_realtime_audio_api::AudioStreamFault;
use thiserror::Error;

const MAX_RECOVERY_ATTEMPTS: u32 = 5;
const INITIAL_RECOVERY_BACKOFF_MS: u64 = 250;
const MAX_RECOVERY_BACKOFF_MS: u64 = 4_000;

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
    /// The reopened stream has remained healthy for the caller-defined stability interval.
    StableRunObserved,
    /// Numeric quality threshold was crossed without stream loss.
    QualityDegraded,
    /// A callback/backend fault was observed.
    StreamFault(AudioStreamFault),
    /// Begin a bounded reopen attempt for the same selectors.
    RecoveryRequested,
    /// Reopen succeeded. Recovery history intentionally remains until stability is proven.
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

    /// Returns whether another reopen attempt is permitted in this failure episode.
    pub fn can_attempt_recovery(&self) -> bool {
        self.state == DuplexStreamState::Faulted && self.recovery_attempts < MAX_RECOVERY_ATTEMPTS
    }

    /// Returns the backoff for the next permitted recovery attempt.
    ///
    /// The first attempt is delayed 250 ms and subsequent attempts double up to 4 s. `None`
    /// means recovery is either not applicable in the current state or the bounded budget is
    /// exhausted. Sleeping/timer scheduling belongs to the control plane, never the callback.
    pub fn next_recovery_backoff_ms(&self) -> Option<u64> {
        if !self.can_attempt_recovery() {
            return None;
        }
        let shift = self.recovery_attempts.min(31);
        let multiplier = 1_u64.checked_shl(shift).unwrap_or(u64::MAX);
        Some(
            INITIAL_RECOVERY_BACKOFF_MS
                .saturating_mul(multiplier)
                .min(MAX_RECOVERY_BACKOFF_MS),
        )
    }

    /// Applies one lifecycle event.
    pub fn transition(
        &mut self,
        event: DuplexStateEvent,
    ) -> Result<DuplexStreamState, DuplexStateTransitionError> {
        let next = match (self.state, event) {
            (DuplexStreamState::Stopped, DuplexStateEvent::StartRequested) => {
                self.recovery_attempts = 0;
                self.fault = AudioStreamFault::None;
                DuplexStreamState::Starting
            }
            (DuplexStreamState::Starting, DuplexStateEvent::StreamsStarted) => {
                DuplexStreamState::Running
            }
            (
                DuplexStreamState::Running | DuplexStreamState::Degraded,
                DuplexStateEvent::StableRunObserved,
            ) => {
                self.recovery_attempts = 0;
                self.fault = AudioStreamFault::None;
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
            (DuplexStreamState::Faulted, DuplexStateEvent::RecoveryRequested)
                if self.recovery_attempts < MAX_RECOVERY_ATTEMPTS =>
            {
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
                self.fault = AudioStreamFault::None;
                self.recovery_attempts = 0;
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

    fn start(machine: &mut DuplexStateMachine) {
        machine
            .transition(DuplexStateEvent::StartRequested)
            .unwrap();
        machine
            .transition(DuplexStateEvent::StreamsStarted)
            .unwrap();
    }

    #[test]
    fn normal_start_degrade_fault_recover_stop_path_is_explicit() {
        let mut machine = DuplexStateMachine::default();
        start(&mut machine);
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
        assert_eq!(machine.next_recovery_backoff_ms(), Some(250));
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
        assert_eq!(machine.recovery_attempts(), 1);
        assert_eq!(
            machine
                .transition(DuplexStateEvent::StableRunObserved)
                .unwrap(),
            DuplexStreamState::Running
        );
        assert_eq!(machine.recovery_attempts(), 0);
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
    }

    #[test]
    fn recovery_budget_and_exponential_backoff_are_bounded() {
        let mut machine = DuplexStateMachine::default();
        start(&mut machine);
        machine
            .transition(DuplexStateEvent::StreamFault(AudioStreamFault::DeviceLost))
            .unwrap();
        let expected_backoff = [250, 500, 1_000, 2_000, 4_000];
        for (attempt, expected) in expected_backoff.into_iter().enumerate() {
            assert_eq!(machine.next_recovery_backoff_ms(), Some(expected));
            machine
                .transition(DuplexStateEvent::RecoveryRequested)
                .unwrap();
            assert_eq!(machine.recovery_attempts(), attempt as u32 + 1);
            machine
                .transition(DuplexStateEvent::RecoveryFailed)
                .unwrap();
        }
        assert!(!machine.can_attempt_recovery());
        assert_eq!(machine.next_recovery_backoff_ms(), None);
        let error = machine
            .transition(DuplexStateEvent::RecoveryRequested)
            .unwrap_err();
        assert_eq!(error.state, DuplexStreamState::Faulted);
        assert_eq!(machine.recovery_attempts(), MAX_RECOVERY_ATTEMPTS);
    }

    #[test]
    fn successful_reopen_does_not_hide_a_flapping_device() {
        let mut machine = DuplexStateMachine::default();
        start(&mut machine);
        for attempt in 1..=MAX_RECOVERY_ATTEMPTS {
            machine
                .transition(DuplexStateEvent::StreamFault(AudioStreamFault::DeviceLost))
                .unwrap();
            machine
                .transition(DuplexStateEvent::RecoveryRequested)
                .unwrap();
            machine
                .transition(DuplexStateEvent::RecoverySucceeded)
                .unwrap();
            assert_eq!(machine.recovery_attempts(), attempt);
        }
        machine
            .transition(DuplexStateEvent::StreamFault(AudioStreamFault::DeviceLost))
            .unwrap();
        assert!(!machine.can_attempt_recovery());
    }

    #[test]
    fn device_loss_is_numeric_and_invalid_transition_is_rejected() {
        let mut machine = DuplexStateMachine::default();
        let error = machine
            .transition(DuplexStateEvent::StreamsStarted)
            .unwrap_err();
        assert_eq!(error.state, DuplexStreamState::Stopped);
        start(&mut machine);
        machine
            .transition(DuplexStateEvent::StreamFault(AudioStreamFault::Callback))
            .unwrap();
        assert_eq!(machine.fault(), AudioStreamFault::Callback);
    }
}
