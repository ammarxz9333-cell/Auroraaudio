//! Fail-closed endpoint contract for one detachable Aurora speaker module.
//!
//! This module is intentionally transport- and MCU-agnostic. It models the
//! safety-critical control flow that an ESP32-P4/C6 or other endpoint firmware
//! must preserve: boot muted, explicit route preparation, synchronized arming,
//! epoch-checked commit, and immediate mute on clock/power/data faults.

use aurora_core::ChannelRole;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ModuleAttachment, ModuleClockSource, ModuleRoute, ModuleSyncStatus, SpeakerModuleCapabilities,
    SpeakerModuleState,
};

/// Runtime admission limits enforced locally by a detachable endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointPolicy {
    /// Maximum absolute skew accepted before the endpoint fails closed.
    pub maximum_skew_micros: u32,
    /// Minimum battery percentage required to start detached playback.
    pub minimum_battery_percent: u8,
}

impl Default for EndpointPolicy {
    fn default() -> Self {
        Self {
            maximum_skew_micros: 1_000,
            minimum_battery_percent: 10,
        }
    }
}

/// Power telemetry supplied by the board-specific power-management layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointPowerStatus {
    /// True when the dock or another external supply is present.
    pub external_power: bool,
    /// Battery state of charge when the module has a battery.
    pub battery_percent: Option<u8>,
    /// Whether the battery is currently charging.
    pub charging: bool,
}

impl EndpointPowerStatus {
    /// Safe default used before real power telemetry has arrived.
    pub const fn unknown() -> Self {
        Self {
            external_power: false,
            battery_percent: None,
            charging: false,
        }
    }

    fn valid(self) -> bool {
        self.battery_percent.map(|value| value <= 100).unwrap_or(true)
    }
}

/// Endpoint lifecycle. Every boot and every route transition starts muted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointPhase {
    BootMuted,
    Muted,
    Prepared,
    Armed,
    Committed,
    Playing,
    Fault,
}

/// One route staged or active at the endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointRoute {
    pub epoch: u64,
    pub route: ModuleRoute,
    pub clock_source: ModuleClockSource,
    pub roles: Vec<ChannelRole>,
    pub start_frame: Option<u64>,
}

/// Control messages sent from Aurora to one physical speaker module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case")]
pub enum EndpointCommand {
    /// Starts a new epoch and silences the amplifier before any reconfiguration.
    Mute {
        module_id: String,
        epoch: u64,
    },
    /// Stages a route and role set while the endpoint remains muted.
    Prepare {
        module_id: String,
        epoch: u64,
        route: ModuleRoute,
        clock_source: ModuleClockSource,
        roles: Vec<ChannelRole>,
    },
    /// Arms a prepared route against an absolute Aurora media frame.
    Arm {
        module_id: String,
        epoch: u64,
        start_frame: u64,
    },
    /// Commits the armed route. The amplifier remains muted until start_frame.
    Commit {
        module_id: String,
        epoch: u64,
    },
    /// Stops the current route and returns to a muted state.
    Stop {
        module_id: String,
        epoch: u64,
    },
}

/// Hardware-facing actions produced by the pure endpoint state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointAction {
    SetAmplifierMute(bool),
    ConfigureRoute {
        route: ModuleRoute,
        clock_source: ModuleClockSource,
        roles: Vec<ChannelRole>,
    },
    ArmPlayout {
        start_frame: u64,
    },
    StartPlayout,
    StopPlayout,
}

/// Local fault reasons that always force amplifier mute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EndpointFault {
    SynchronizationLost,
    ExcessiveSkew,
    PowerInsufficient,
    StreamUnderflow,
    TransportLost,
    AttachmentChanged,
}

/// Protocol/state-machine errors. Rejected commands never unmute the endpoint.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EndpointError {
    #[error("endpoint module id must not be empty")]
    EmptyModuleId,
    #[error("endpoint command targets another module")]
    WrongModule,
    #[error("endpoint power telemetry is invalid")]
    InvalidPowerTelemetry,
    #[error("endpoint epoch is stale or mismatched")]
    EpochMismatch,
    #[error("endpoint command is invalid in the current phase")]
    InvalidPhase,
    #[error("endpoint route is incompatible with module capabilities/state")]
    UnsupportedRoute,
    #[error("endpoint role count exceeds available output lanes")]
    TooManyRoles,
    #[error("endpoint power is insufficient for detached playback")]
    InsufficientPower,
    #[error("endpoint synchronization evidence is insufficient")]
    SynchronizationInsufficient,
    #[error("endpoint start frame must be non-zero")]
    InvalidStartFrame,
    #[error("endpoint is faulted and must be explicitly recovered")]
    Faulted,
}

/// Pure control state for one physical detachable module.
#[derive(Debug, Clone)]
pub struct DetachableEndpoint {
    module_id: String,
    capabilities: SpeakerModuleCapabilities,
    policy: EndpointPolicy,
    attachment: ModuleAttachment,
    power: EndpointPowerStatus,
    sync: ModuleSyncStatus,
    phase: EndpointPhase,
    current_epoch: Option<u64>,
    pending: Option<EndpointRoute>,
    active: Option<EndpointRoute>,
    amplifier_muted: bool,
    online: bool,
    last_fault: Option<EndpointFault>,
}

impl DetachableEndpoint {
    pub fn new(
        module_id: impl Into<String>,
        capabilities: SpeakerModuleCapabilities,
        policy: EndpointPolicy,
    ) -> Result<Self, EndpointError> {
        let module_id = module_id.into();
        if module_id.is_empty() {
            return Err(EndpointError::EmptyModuleId);
        }
        Ok(Self {
            module_id,
            capabilities,
            policy,
            attachment: ModuleAttachment::Detached,
            power: EndpointPowerStatus::unknown(),
            sync: ModuleSyncStatus {
                locked: false,
                scheduled_playout: false,
                estimated_skew_micros: None,
                clock_source: ModuleClockSource::Unlocked,
            },
            phase: EndpointPhase::BootMuted,
            current_epoch: None,
            pending: None,
            active: None,
            amplifier_muted: true,
            online: true,
            last_fault: None,
        })
    }

    pub fn module_id(&self) -> &str {
        &self.module_id
    }

    pub const fn phase(&self) -> EndpointPhase {
        self.phase
    }

    pub const fn amplifier_muted(&self) -> bool {
        self.amplifier_muted
    }

    pub fn active_route(&self) -> Option<&EndpointRoute> {
        self.active.as_ref()
    }

    pub const fn last_fault(&self) -> Option<EndpointFault> {
        self.last_fault
    }

    /// Updates power telemetry and immediately fails closed if an active
    /// detached route can no longer be powered safely.
    pub fn update_power(
        &mut self,
        power: EndpointPowerStatus,
    ) -> Result<Vec<EndpointAction>, EndpointError> {
        if !power.valid() {
            return Err(EndpointError::InvalidPowerTelemetry);
        }
        self.power = power;
        if self.requires_battery() && !self.detached_power_is_sufficient() {
            return Ok(self.fail_closed(EndpointFault::PowerInsufficient));
        }
        Ok(Vec::new())
    }

    /// Attachment changes invalidate all prepared/active routing. A fresh
    /// handover epoch is required before audio can resume.
    pub fn update_attachment(&mut self, attachment: ModuleAttachment) -> Vec<EndpointAction> {
        if self.attachment == attachment {
            return Vec::new();
        }
        self.attachment = attachment;
        self.fail_closed(EndpointFault::AttachmentChanged)
    }

    /// Synchronization telemetry is continuously enforced while a route is
    /// armed/committed/playing.
    pub fn update_sync(&mut self, sync: ModuleSyncStatus) -> Vec<EndpointAction> {
        self.sync = sync;
        if matches!(
            self.phase,
            EndpointPhase::Armed | EndpointPhase::Committed | EndpointPhase::Playing
        ) && self.sync_is_sufficient().is_err()
        {
            let fault = match self.sync.estimated_skew_micros {
                Some(skew) if skew > self.policy.maximum_skew_micros => {
                    EndpointFault::ExcessiveSkew
                }
                _ => EndpointFault::SynchronizationLost,
            };
            return self.fail_closed(fault);
        }
        Vec::new()
    }

    pub fn set_online(&mut self, online: bool) -> Vec<EndpointAction> {
        self.online = online;
        if !online {
            return self.fail_closed(EndpointFault::TransportLost);
        }
        Vec::new()
    }

    pub fn report_stream_underflow(&mut self) -> Vec<EndpointAction> {
        self.fail_closed(EndpointFault::StreamUnderflow)
    }

    /// Clears a latched fault only into a muted state. Audio still needs a new
    /// complete Mute -> Prepare -> Arm -> Commit epoch before it can restart.
    pub fn recover_to_muted(&mut self) -> Result<Vec<EndpointAction>, EndpointError> {
        if self.phase != EndpointPhase::Fault {
            return Err(EndpointError::InvalidPhase);
        }
        self.pending = None;
        self.active = None;
        self.current_epoch = None;
        self.last_fault = None;
        self.phase = EndpointPhase::Muted;
        self.amplifier_muted = true;
        Ok(vec![EndpointAction::SetAmplifierMute(true)])
    }

    pub fn apply(
        &mut self,
        command: &EndpointCommand,
    ) -> Result<Vec<EndpointAction>, EndpointError> {
        if self.phase == EndpointPhase::Fault {
            return Err(EndpointError::Faulted);
        }
        let (module_id, epoch) = command_identity(command);
        if module_id != self.module_id {
            return Err(EndpointError::WrongModule);
        }

        match command {
            EndpointCommand::Mute { .. } => self.apply_mute(epoch),
            EndpointCommand::Prepare {
                route,
                clock_source,
                roles,
                ..
            } => self.apply_prepare(epoch, *route, *clock_source, roles),
            EndpointCommand::Arm { start_frame, .. } => self.apply_arm(epoch, *start_frame),
            EndpointCommand::Commit { .. } => self.apply_commit(epoch),
            EndpointCommand::Stop { .. } => self.apply_stop(epoch),
        }
    }

    /// Advances the absolute Aurora media timeline. A committed route does not
    /// unmute early; it starts only once the scheduled frame is reached.
    pub fn on_media_frame(
        &mut self,
        frame_index: u64,
    ) -> Result<Vec<EndpointAction>, EndpointError> {
        if self.phase != EndpointPhase::Committed {
            return Ok(Vec::new());
        }
        self.sync_is_sufficient()?;
        let start = self
            .pending
            .as_ref()
            .and_then(|route| route.start_frame)
            .ok_or(EndpointError::InvalidPhase)?;
        if frame_index < start {
            return Ok(Vec::new());
        }

        let active = self.pending.take().ok_or(EndpointError::InvalidPhase)?;
        self.active = Some(active);
        self.phase = EndpointPhase::Playing;
        self.amplifier_muted = false;
        Ok(vec![
            EndpointAction::StartPlayout,
            EndpointAction::SetAmplifierMute(false),
        ])
    }

    /// Snapshot consumed by the existing Aurora modular-fabric planner/OS.
    pub fn speaker_module_state(&self) -> SpeakerModuleState {
        SpeakerModuleState {
            module_id: self.module_id.clone(),
            capabilities: self.capabilities.clone(),
            attachment: self.attachment.clone(),
            sync: self.sync,
            online: self.online && self.phase != EndpointPhase::Fault,
        }
    }

    fn apply_mute(&mut self, epoch: u64) -> Result<Vec<EndpointAction>, EndpointError> {
        if self.current_epoch.is_some_and(|current| epoch <= current) {
            return Err(EndpointError::EpochMismatch);
        }
        self.current_epoch = Some(epoch);
        self.pending = None;
        self.active = None;
        self.phase = EndpointPhase::Muted;
        self.amplifier_muted = true;
        Ok(vec![
            EndpointAction::SetAmplifierMute(true),
            EndpointAction::StopPlayout,
        ])
    }

    fn apply_prepare(
        &mut self,
        epoch: u64,
        route: ModuleRoute,
        clock_source: ModuleClockSource,
        roles: &[ChannelRole],
    ) -> Result<Vec<EndpointAction>, EndpointError> {
        self.require_epoch(epoch)?;
        if self.phase != EndpointPhase::Muted {
            return Err(EndpointError::InvalidPhase);
        }
        self.validate_route(route, clock_source, roles)?;
        self.pending = Some(EndpointRoute {
            epoch,
            route,
            clock_source,
            roles: roles.to_vec(),
            start_frame: None,
        });
        self.phase = EndpointPhase::Prepared;
        Ok(vec![EndpointAction::ConfigureRoute {
            route,
            clock_source,
            roles: roles.to_vec(),
        }])
    }

    fn apply_arm(
        &mut self,
        epoch: u64,
        start_frame: u64,
    ) -> Result<Vec<EndpointAction>, EndpointError> {
        self.require_epoch(epoch)?;
        if self.phase != EndpointPhase::Prepared {
            return Err(EndpointError::InvalidPhase);
        }
        if start_frame == 0 {
            return Err(EndpointError::InvalidStartFrame);
        }
        self.sync_is_sufficient()?;
        let pending = self.pending.as_mut().ok_or(EndpointError::InvalidPhase)?;
        pending.start_frame = Some(start_frame);
        self.phase = EndpointPhase::Armed;
        Ok(vec![EndpointAction::ArmPlayout { start_frame }])
    }

    fn apply_commit(&mut self, epoch: u64) -> Result<Vec<EndpointAction>, EndpointError> {
        self.require_epoch(epoch)?;
        if self.phase != EndpointPhase::Armed {
            return Err(EndpointError::InvalidPhase);
        }
        self.sync_is_sufficient()?;
        self.phase = EndpointPhase::Committed;
        self.amplifier_muted = true;
        Ok(Vec::new())
    }

    fn apply_stop(&mut self, epoch: u64) -> Result<Vec<EndpointAction>, EndpointError> {
        self.require_epoch(epoch)?;
        self.pending = None;
        self.active = None;
        self.phase = EndpointPhase::Muted;
        self.amplifier_muted = true;
        Ok(vec![
            EndpointAction::SetAmplifierMute(true),
            EndpointAction::StopPlayout,
        ])
    }

    fn require_epoch(&self, epoch: u64) -> Result<(), EndpointError> {
        if self.current_epoch == Some(epoch) {
            Ok(())
        } else {
            Err(EndpointError::EpochMismatch)
        }
    }

    fn validate_route(
        &self,
        route: ModuleRoute,
        clock_source: ModuleClockSource,
        roles: &[ChannelRole],
    ) -> Result<(), EndpointError> {
        if roles.is_empty() || roles.len() > self.capabilities.output_lanes {
            return Err(EndpointError::TooManyRoles);
        }

        match route {
            ModuleRoute::DockBus => {
                if !self.capabilities.dock_pcm
                    || !matches!(self.attachment, ModuleAttachment::Docked { .. })
                    || clock_source != ModuleClockSource::DockRecovered
                {
                    return Err(EndpointError::UnsupportedRoute);
                }
            }
            ModuleRoute::WiredNetwork => {
                if !self.capabilities.wired_network_audio
                    || clock_source != ModuleClockSource::Ptp
                {
                    return Err(EndpointError::UnsupportedRoute);
                }
            }
            ModuleRoute::WirelessNetwork => {
                if !self.capabilities.wireless_network_audio
                    || !matches!(self.attachment, ModuleAttachment::Detached)
                    || !matches!(
                        clock_source,
                        ModuleClockSource::Ptp | ModuleClockSource::AdaptivePeer
                    )
                {
                    return Err(EndpointError::UnsupportedRoute);
                }
                if !self.detached_power_is_sufficient() {
                    return Err(EndpointError::InsufficientPower);
                }
            }
            ModuleRoute::Muted => return Err(EndpointError::UnsupportedRoute),
        }
        Ok(())
    }

    fn sync_is_sufficient(&self) -> Result<(), EndpointError> {
        let route = self.pending.as_ref().or(self.active.as_ref());
        let Some(route) = route else {
            return Err(EndpointError::InvalidPhase);
        };
        if !self.online || !self.sync.locked || !self.sync.scheduled_playout {
            return Err(EndpointError::SynchronizationInsufficient);
        }
        if self.sync.clock_source != route.clock_source {
            return Err(EndpointError::SynchronizationInsufficient);
        }
        let skew = self
            .sync
            .estimated_skew_micros
            .ok_or(EndpointError::SynchronizationInsufficient)?;
        if skew > self.policy.maximum_skew_micros {
            return Err(EndpointError::SynchronizationInsufficient);
        }
        Ok(())
    }

    fn requires_battery(&self) -> bool {
        matches!(self.attachment, ModuleAttachment::Detached)
            && !self.power.external_power
            && (self.pending.as_ref().is_some_and(|route| {
                matches!(
                    route.route,
                    ModuleRoute::WirelessNetwork | ModuleRoute::WiredNetwork
                )
            }) || self.active.as_ref().is_some_and(|route| {
                matches!(
                    route.route,
                    ModuleRoute::WirelessNetwork | ModuleRoute::WiredNetwork
                )
            }))
    }

    fn detached_power_is_sufficient(&self) -> bool {
        if self.power.external_power {
            return true;
        }
        if !self.capabilities.battery_powered {
            return false;
        }
        self.power
            .battery_percent
            .is_some_and(|percent| percent >= self.policy.minimum_battery_percent)
    }

    fn fail_closed(&mut self, fault: EndpointFault) -> Vec<EndpointAction> {
        self.pending = None;
        self.active = None;
        self.phase = EndpointPhase::Fault;
        self.amplifier_muted = true;
        self.last_fault = Some(fault);
        vec![
            EndpointAction::SetAmplifierMute(true),
            EndpointAction::StopPlayout,
        ]
    }
}

fn command_identity(command: &EndpointCommand) -> (&str, u64) {
    match command {
        EndpointCommand::Mute { module_id, epoch }
        | EndpointCommand::Prepare {
            module_id, epoch, ..
        }
        | EndpointCommand::Arm {
            module_id, epoch, ..
        }
        | EndpointCommand::Commit { module_id, epoch }
        | EndpointCommand::Stop { module_id, epoch } => (module_id.as_str(), *epoch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities() -> SpeakerModuleCapabilities {
        SpeakerModuleCapabilities {
            output_lanes: 2,
            dock_pcm: true,
            wired_network_audio: true,
            wireless_network_audio: true,
            battery_powered: true,
        }
    }

    fn detached_ready() -> DetachableEndpoint {
        let mut endpoint =
            DetachableEndpoint::new("left-pod", capabilities(), EndpointPolicy::default()).unwrap();
        endpoint
            .update_power(EndpointPowerStatus {
                external_power: false,
                battery_percent: Some(80),
                charging: false,
            })
            .unwrap();
        endpoint.update_sync(ModuleSyncStatus {
            locked: true,
            scheduled_playout: true,
            estimated_skew_micros: Some(180),
            clock_source: ModuleClockSource::Ptp,
        });
        endpoint
    }

    fn prepare_rear(endpoint: &mut DetachableEndpoint, epoch: u64) {
        endpoint
            .apply(&EndpointCommand::Mute {
                module_id: "left-pod".into(),
                epoch,
            })
            .unwrap();
        endpoint
            .apply(&EndpointCommand::Prepare {
                module_id: "left-pod".into(),
                epoch,
                route: ModuleRoute::WirelessNetwork,
                clock_source: ModuleClockSource::Ptp,
                roles: vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft],
            })
            .unwrap();
    }

    #[test]
    fn endpoint_boots_fail_closed_and_muted() {
        let endpoint =
            DetachableEndpoint::new("left-pod", capabilities(), EndpointPolicy::default()).unwrap();
        assert_eq!(endpoint.phase(), EndpointPhase::BootMuted);
        assert!(endpoint.amplifier_muted());
    }

    #[test]
    fn committed_route_does_not_unmute_before_scheduled_frame() {
        let mut endpoint = detached_ready();
        prepare_rear(&mut endpoint, 7);
        endpoint
            .apply(&EndpointCommand::Arm {
                module_id: "left-pod".into(),
                epoch: 7,
                start_frame: 96_000,
            })
            .unwrap();
        endpoint
            .apply(&EndpointCommand::Commit {
                module_id: "left-pod".into(),
                epoch: 7,
            })
            .unwrap();

        assert!(endpoint.amplifier_muted());
        assert!(endpoint.on_media_frame(95_999).unwrap().is_empty());
        let actions = endpoint.on_media_frame(96_000).unwrap();
        assert_eq!(endpoint.phase(), EndpointPhase::Playing);
        assert!(!endpoint.amplifier_muted());
        assert_eq!(
            actions,
            vec![
                EndpointAction::StartPlayout,
                EndpointAction::SetAmplifierMute(false)
            ]
        );
    }

    #[test]
    fn stale_epoch_is_rejected_without_unmuting() {
        let mut endpoint = detached_ready();
        endpoint
            .apply(&EndpointCommand::Mute {
                module_id: "left-pod".into(),
                epoch: 4,
            })
            .unwrap();
        assert_eq!(
            endpoint.apply(&EndpointCommand::Mute {
                module_id: "left-pod".into(),
                epoch: 3,
            }),
            Err(EndpointError::EpochMismatch)
        );
        assert!(endpoint.amplifier_muted());
    }

    #[test]
    fn sync_loss_while_playing_mutes_and_latches_fault() {
        let mut endpoint = detached_ready();
        prepare_rear(&mut endpoint, 8);
        endpoint
            .apply(&EndpointCommand::Arm {
                module_id: "left-pod".into(),
                epoch: 8,
                start_frame: 48_000,
            })
            .unwrap();
        endpoint
            .apply(&EndpointCommand::Commit {
                module_id: "left-pod".into(),
                epoch: 8,
            })
            .unwrap();
        endpoint.on_media_frame(48_000).unwrap();

        let actions = endpoint.update_sync(ModuleSyncStatus {
            locked: false,
            scheduled_playout: true,
            estimated_skew_micros: None,
            clock_source: ModuleClockSource::Ptp,
        });
        assert_eq!(endpoint.phase(), EndpointPhase::Fault);
        assert_eq!(endpoint.last_fault(), Some(EndpointFault::SynchronizationLost));
        assert!(endpoint.amplifier_muted());
        assert!(actions.contains(&EndpointAction::SetAmplifierMute(true)));
    }

    #[test]
    fn excessive_runtime_skew_fails_closed() {
        let mut endpoint = detached_ready();
        prepare_rear(&mut endpoint, 9);
        endpoint
            .apply(&EndpointCommand::Arm {
                module_id: "left-pod".into(),
                epoch: 9,
                start_frame: 48_000,
            })
            .unwrap();

        endpoint.update_sync(ModuleSyncStatus {
            locked: true,
            scheduled_playout: true,
            estimated_skew_micros: Some(1_001),
            clock_source: ModuleClockSource::Ptp,
        });
        assert_eq!(endpoint.phase(), EndpointPhase::Fault);
        assert_eq!(endpoint.last_fault(), Some(EndpointFault::ExcessiveSkew));
    }

    #[test]
    fn detached_wireless_start_requires_battery_or_external_power() {
        let mut endpoint =
            DetachableEndpoint::new("left-pod", capabilities(), EndpointPolicy::default()).unwrap();
        endpoint
            .apply(&EndpointCommand::Mute {
                module_id: "left-pod".into(),
                epoch: 1,
            })
            .unwrap();
        assert_eq!(
            endpoint.apply(&EndpointCommand::Prepare {
                module_id: "left-pod".into(),
                epoch: 1,
                route: ModuleRoute::WirelessNetwork,
                clock_source: ModuleClockSource::Ptp,
                roles: vec![ChannelRole::SurroundLeft],
            }),
            Err(EndpointError::InsufficientPower)
        );
    }

    #[test]
    fn dock_bus_requires_physical_dock_and_recovered_clock() {
        let mut endpoint = detached_ready();
        endpoint
            .apply(&EndpointCommand::Mute {
                module_id: "left-pod".into(),
                epoch: 1,
            })
            .unwrap();
        assert_eq!(
            endpoint.apply(&EndpointCommand::Prepare {
                module_id: "left-pod".into(),
                epoch: 1,
                route: ModuleRoute::DockBus,
                clock_source: ModuleClockSource::DockRecovered,
                roles: vec![ChannelRole::FrontLeft],
            }),
            Err(EndpointError::UnsupportedRoute)
        );

        endpoint.update_attachment(ModuleAttachment::Docked {
            slot_id: "left-edge".into(),
        });
        endpoint.recover_to_muted().unwrap();
        endpoint.update_sync(ModuleSyncStatus::docked_locked());
        endpoint
            .apply(&EndpointCommand::Mute {
                module_id: "left-pod".into(),
                epoch: 2,
            })
            .unwrap();
        assert!(endpoint
            .apply(&EndpointCommand::Prepare {
                module_id: "left-pod".into(),
                epoch: 2,
                route: ModuleRoute::DockBus,
                clock_source: ModuleClockSource::DockRecovered,
                roles: vec![ChannelRole::FrontLeft, ChannelRole::TopFrontLeft],
            })
            .is_ok());
    }

    #[test]
    fn stream_underflow_is_fail_closed() {
        let mut endpoint = detached_ready();
        endpoint.report_stream_underflow();
        assert_eq!(endpoint.phase(), EndpointPhase::Fault);
        assert_eq!(endpoint.last_fault(), Some(EndpointFault::StreamUnderflow));
        assert!(endpoint.amplifier_muted());
    }

    #[test]
    fn fabric_snapshot_uses_same_identity_capabilities_and_sync() {
        let endpoint = detached_ready();
        let snapshot = endpoint.speaker_module_state();
        assert_eq!(snapshot.module_id, "left-pod");
        assert_eq!(snapshot.capabilities.output_lanes, 2);
        assert_eq!(snapshot.sync.clock_source, ModuleClockSource::Ptp);
        assert!(snapshot.online);
    }
}
