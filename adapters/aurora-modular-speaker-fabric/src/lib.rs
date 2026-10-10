//! Aurora detachable-speaker control plane.
//!
//! This crate models the part of Aurora that decides what a physical speaker
//! module is allowed to do when it is docked into the soundbar or detached and
//! operating over the network. It intentionally does not claim a physical
//! wireless transport implementation. Instead it emits deterministic route
//! decisions that a proven transport can consume later.
//!
//! Design rules:
//! - Docked modules prefer the wired dock bus and do not depend on wireless sync.
//! - Detached cinema modules fail closed unless synchronization is within policy.
//! - Re-docking restores the configured soundbar role automatically.
//! - Role changes are control-plane events; the real-time renderer only sees
//!   the resulting stable channel assignment.

use aurora_core::ChannelRole;
use thiserror::Error;

/// Operating layout requested by the user/session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FabricProfile {
    /// All docked modules participate in the soundbar.
    Soundbar,
    /// Detached modules may become surround/rear speakers.
    Cinema,
    /// Detached modules may form a normal stereo pair.
    StereoPair,
    /// Independent-room playback. Spatial cinema sync is not required.
    Multiroom,
}

/// Physical attachment state reported by a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentState {
    Docked,
    Detached,
}

/// Transport selected by the fabric controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleTransport {
    /// Synchronous wired audio/clock/power through the soundbar dock.
    DockBus,
    /// Timestamped network audio to the detached module.
    Network,
    /// No audio is sent.
    Muted,
}

/// Synchronization health for a detached module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// No trustworthy timing measurement exists yet.
    Unknown,
    /// The endpoint clock is locked to the media timeline.
    Locked {
        /// Absolute presentation offset from the target media timeline.
        offset_micros: u32,
        /// Short-term timing jitter.
        jitter_micros: u32,
        /// Estimated clock-rate mismatch.
        drift_ppm: i32,
    },
    /// Transport/clock worker reported a hard fault.
    Fault,
}

/// Static capabilities and role mapping of one detachable module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSpec {
    /// Stable hardware/module identity.
    pub id: String,
    /// Channel role used while the module is physically docked.
    pub dock_role: ChannelRole,
    /// Optional cinema role used after detaching.
    pub detached_cinema_role: Option<ChannelRole>,
    /// Optional left/right role used in standalone stereo mode.
    pub stereo_role: Option<ChannelRole>,
    /// Whether the module can receive synchronous audio through dock contacts.
    pub supports_dock_bus: bool,
    /// Whether the module can receive Aurora network audio when detached.
    pub supports_network_audio: bool,
    /// Whether the module contains a battery for detached operation.
    pub has_battery: bool,
}

/// Live telemetry used for one routing decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleTelemetry {
    pub attachment: AttachmentState,
    /// Battery state when known. Dock-powered modules may report no value.
    pub battery_percent: Option<u8>,
    pub sync: SyncState,
}

/// Safety/timing thresholds for detachable cinema operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FabricPolicy {
    /// Maximum absolute playout offset accepted for cinema.
    pub max_offset_micros: u32,
    /// Maximum short-term jitter accepted for cinema.
    pub max_jitter_micros: u32,
    /// Maximum absolute clock drift accepted before muting.
    pub max_drift_ppm: i32,
    /// Detached battery threshold below which cinema/stereo playback is muted.
    pub minimum_battery_percent: u8,
}

impl Default for FabricPolicy {
    fn default() -> Self {
        Self {
            max_offset_micros: 1_000,
            max_jitter_micros: 500,
            max_drift_ppm: 250,
            minimum_battery_percent: 5,
        }
    }
}

/// Human/machine-readable reason for the selected route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteReason {
    DockedSoundbar,
    DetachedCinema,
    DetachedStereo,
    DetachedMultiroom,
    ProfileDoesNotUseModule,
    UnsupportedDock,
    UnsupportedNetwork,
    BatteryTooLow,
    SyncNotLocked,
    SyncOutsideCinemaPolicy,
    NoRoleForProfile,
}

/// Final routing decision for one module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRoute {
    pub module_id: String,
    pub transport: ModuleTransport,
    pub role: Option<ChannelRole>,
    pub reason: RouteReason,
}

impl ModuleRoute {
    pub fn active(&self) -> bool {
        self.transport != ModuleTransport::Muted && self.role.is_some()
    }
}

/// Fail-closed configuration errors.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum FabricError {
    #[error("module id must not be empty")]
    EmptyModuleId,
    #[error("battery percentage must be in the range 0..=100")]
    InvalidBattery,
    #[error("minimum battery percentage must be in the range 0..=100")]
    InvalidBatteryPolicy,
    #[error("maximum drift policy must not be negative")]
    InvalidDriftPolicy,
}

/// Deterministic control-plane resolver.
#[derive(Debug, Clone, Copy)]
pub struct SpeakerFabric {
    policy: FabricPolicy,
}

impl SpeakerFabric {
    pub fn new(policy: FabricPolicy) -> Result<Self, FabricError> {
        if policy.minimum_battery_percent > 100 {
            return Err(FabricError::InvalidBatteryPolicy);
        }
        if policy.max_drift_ppm < 0 {
            return Err(FabricError::InvalidDriftPolicy);
        }
        Ok(Self { policy })
    }

    pub fn policy(&self) -> FabricPolicy {
        self.policy
    }

    /// Resolves one module. No fallback silently changes a cinema role: if a
    /// detached endpoint cannot meet timing or capability requirements it is
    /// muted and reports the reason.
    pub fn resolve(
        &self,
        spec: &ModuleSpec,
        telemetry: ModuleTelemetry,
        profile: FabricProfile,
    ) -> Result<ModuleRoute, FabricError> {
        if spec.id.trim().is_empty() {
            return Err(FabricError::EmptyModuleId);
        }
        if telemetry.battery_percent.is_some_and(|value| value > 100) {
            return Err(FabricError::InvalidBattery);
        }

        match telemetry.attachment {
            AttachmentState::Docked => self.resolve_docked(spec, profile),
            AttachmentState::Detached => self.resolve_detached(spec, telemetry, profile),
        }
    }

    fn resolve_docked(
        &self,
        spec: &ModuleSpec,
        profile: FabricProfile,
    ) -> Result<ModuleRoute, FabricError> {
        if !spec.supports_dock_bus {
            return Ok(muted(spec, RouteReason::UnsupportedDock));
        }

        let role = match profile {
            FabricProfile::Soundbar | FabricProfile::Cinema => Some(spec.dock_role.clone()),
            FabricProfile::StereoPair => spec.stereo_role.clone(),
            FabricProfile::Multiroom => None,
        };

        match role {
            Some(role) => Ok(ModuleRoute {
                module_id: spec.id.clone(),
                transport: ModuleTransport::DockBus,
                role: Some(role),
                reason: RouteReason::DockedSoundbar,
            }),
            None => Ok(muted(spec, RouteReason::ProfileDoesNotUseModule)),
        }
    }

    fn resolve_detached(
        &self,
        spec: &ModuleSpec,
        telemetry: ModuleTelemetry,
        profile: FabricProfile,
    ) -> Result<ModuleRoute, FabricError> {
        if !spec.supports_network_audio {
            return Ok(muted(spec, RouteReason::UnsupportedNetwork));
        }

        if spec.has_battery
            && telemetry
                .battery_percent
                .is_some_and(|value| value < self.policy.minimum_battery_percent)
        {
            return Ok(muted(spec, RouteReason::BatteryTooLow));
        }

        let (role, reason, requires_cinema_sync) = match profile {
            FabricProfile::Soundbar => {
                return Ok(muted(spec, RouteReason::ProfileDoesNotUseModule));
            }
            FabricProfile::Cinema => (
                spec.detached_cinema_role.clone(),
                RouteReason::DetachedCinema,
                true,
            ),
            FabricProfile::StereoPair => (
                spec.stereo_role.clone(),
                RouteReason::DetachedStereo,
                true,
            ),
            FabricProfile::Multiroom => (
                spec.stereo_role
                    .clone()
                    .or_else(|| Some(spec.dock_role.clone())),
                RouteReason::DetachedMultiroom,
                false,
            ),
        };

        let Some(role) = role else {
            return Ok(muted(spec, RouteReason::NoRoleForProfile));
        };

        if requires_cinema_sync {
            match telemetry.sync {
                SyncState::Locked {
                    offset_micros,
                    jitter_micros,
                    drift_ppm,
                } if self.sync_within_policy(offset_micros, jitter_micros, drift_ppm) => {}
                SyncState::Locked { .. } => {
                    return Ok(muted(spec, RouteReason::SyncOutsideCinemaPolicy));
                }
                SyncState::Unknown | SyncState::Fault => {
                    return Ok(muted(spec, RouteReason::SyncNotLocked));
                }
            }
        }

        Ok(ModuleRoute {
            module_id: spec.id.clone(),
            transport: ModuleTransport::Network,
            role: Some(role),
            reason,
        })
    }

    fn sync_within_policy(&self, offset_micros: u32, jitter_micros: u32, drift_ppm: i32) -> bool {
        offset_micros <= self.policy.max_offset_micros
            && jitter_micros <= self.policy.max_jitter_micros
            && i64::from(drift_ppm).abs() <= i64::from(self.policy.max_drift_ppm)
    }
}

fn muted(spec: &ModuleSpec, reason: RouteReason) -> ModuleRoute {
    ModuleRoute {
        module_id: spec.id.clone(),
        transport: ModuleTransport::Muted,
        role: None,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn left_module() -> ModuleSpec {
        ModuleSpec {
            id: "wing-left".to_owned(),
            dock_role: ChannelRole::FrontLeft,
            detached_cinema_role: Some(ChannelRole::SurroundLeft),
            stereo_role: Some(ChannelRole::FrontLeft),
            supports_dock_bus: true,
            supports_network_audio: true,
            has_battery: true,
        }
    }

    fn locked() -> SyncState {
        SyncState::Locked {
            offset_micros: 120,
            jitter_micros: 90,
            drift_ppm: -18,
        }
    }

    #[test]
    fn docked_module_uses_wired_bus_without_wireless_lock() {
        let fabric = SpeakerFabric::new(FabricPolicy::default()).unwrap();
        let route = fabric
            .resolve(
                &left_module(),
                ModuleTelemetry {
                    attachment: AttachmentState::Docked,
                    battery_percent: None,
                    sync: SyncState::Unknown,
                },
                FabricProfile::Soundbar,
            )
            .unwrap();

        assert!(route.active());
        assert_eq!(route.transport, ModuleTransport::DockBus);
        assert_eq!(route.role, Some(ChannelRole::FrontLeft));
        assert_eq!(route.reason, RouteReason::DockedSoundbar);
    }

    #[test]
    fn detaching_soundbar_wing_remaps_it_to_surround() {
        let fabric = SpeakerFabric::new(FabricPolicy::default()).unwrap();
        let route = fabric
            .resolve(
                &left_module(),
                ModuleTelemetry {
                    attachment: AttachmentState::Detached,
                    battery_percent: Some(82),
                    sync: locked(),
                },
                FabricProfile::Cinema,
            )
            .unwrap();

        assert!(route.active());
        assert_eq!(route.transport, ModuleTransport::Network);
        assert_eq!(route.role, Some(ChannelRole::SurroundLeft));
        assert_eq!(route.reason, RouteReason::DetachedCinema);
    }

    #[test]
    fn redocking_restores_soundbar_role_automatically() {
        let fabric = SpeakerFabric::new(FabricPolicy::default()).unwrap();
        let spec = left_module();

        let detached = fabric
            .resolve(
                &spec,
                ModuleTelemetry {
                    attachment: AttachmentState::Detached,
                    battery_percent: Some(55),
                    sync: locked(),
                },
                FabricProfile::Cinema,
            )
            .unwrap();
        assert_eq!(detached.role, Some(ChannelRole::SurroundLeft));

        let docked = fabric
            .resolve(
                &spec,
                ModuleTelemetry {
                    attachment: AttachmentState::Docked,
                    battery_percent: Some(56),
                    sync: SyncState::Fault,
                },
                FabricProfile::Cinema,
            )
            .unwrap();
        assert_eq!(docked.transport, ModuleTransport::DockBus);
        assert_eq!(docked.role, Some(ChannelRole::FrontLeft));
    }

    #[test]
    fn detached_cinema_fails_closed_without_clock_lock() {
        let fabric = SpeakerFabric::new(FabricPolicy::default()).unwrap();
        let route = fabric
            .resolve(
                &left_module(),
                ModuleTelemetry {
                    attachment: AttachmentState::Detached,
                    battery_percent: Some(80),
                    sync: SyncState::Unknown,
                },
                FabricProfile::Cinema,
            )
            .unwrap();

        assert!(!route.active());
        assert_eq!(route.transport, ModuleTransport::Muted);
        assert_eq!(route.reason, RouteReason::SyncNotLocked);
    }

    #[test]
    fn detached_cinema_fails_closed_when_jitter_exceeds_policy() {
        let fabric = SpeakerFabric::new(FabricPolicy::default()).unwrap();
        let route = fabric
            .resolve(
                &left_module(),
                ModuleTelemetry {
                    attachment: AttachmentState::Detached,
                    battery_percent: Some(80),
                    sync: SyncState::Locked {
                        offset_micros: 200,
                        jitter_micros: 900,
                        drift_ppm: 10,
                    },
                },
                FabricProfile::Cinema,
            )
            .unwrap();

        assert!(!route.active());
        assert_eq!(route.reason, RouteReason::SyncOutsideCinemaPolicy);
    }

    #[test]
    fn multiroom_does_not_pretend_to_require_cinema_grade_sync() {
        let fabric = SpeakerFabric::new(FabricPolicy::default()).unwrap();
        let route = fabric
            .resolve(
                &left_module(),
                ModuleTelemetry {
                    attachment: AttachmentState::Detached,
                    battery_percent: Some(70),
                    sync: SyncState::Unknown,
                },
                FabricProfile::Multiroom,
            )
            .unwrap();

        assert!(route.active());
        assert_eq!(route.transport, ModuleTransport::Network);
        assert_eq!(route.reason, RouteReason::DetachedMultiroom);
    }

    #[test]
    fn low_battery_mutes_detached_module() {
        let fabric = SpeakerFabric::new(FabricPolicy::default()).unwrap();
        let route = fabric
            .resolve(
                &left_module(),
                ModuleTelemetry {
                    attachment: AttachmentState::Detached,
                    battery_percent: Some(2),
                    sync: locked(),
                },
                FabricProfile::Cinema,
            )
            .unwrap();

        assert_eq!(route.reason, RouteReason::BatteryTooLow);
        assert!(!route.active());
    }
}
