//! Control-plane planning for detachable Aurora speaker modules.
//!
//! This crate owns topology, role assignment, dock/wireless route selection,
//! and synchronization admission. It intentionally does not implement Wi-Fi,
//! AVB, charging, DSP, or amplifier drivers. A module is admitted to a
//! cinema plan only when the selected route has explicit synchronization
//! evidence; otherwise planning fails closed.

use std::collections::BTreeMap;

use aurora_core::{ChannelRole, StandardLayout};
use aurora_realtime_audio_api::{
    NetworkAudioFormat, NetworkClockDiscipline, NetworkStreamConfig, NetworkTimingPolicy,
    NetworkTransportError, AURORA_NETWORK_MEDIA_RATE,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Physical attachment of one speaker module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ModuleAttachment {
    /// Module is mechanically/electrically attached to a known dock slot.
    Docked { slot_id: String },
    /// Module is detached and must use a non-dock transport if active.
    Detached,
}

/// Active media route to one module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModuleRoute {
    /// Shared wired dock PCM/clock bus.
    DockBus,
    /// Scheduled wired network audio.
    WiredNetwork,
    /// Scheduled wireless network audio.
    WirelessNetwork,
    /// Module is intentionally inactive.
    Muted,
}

/// Clock source disciplining a module's playout timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModuleClockSource {
    /// Clock recovered from the wired dock audio bus.
    DockRecovered,
    /// PTP/gPTP disciplined network clock.
    Ptp,
    /// Peer clock followed by one bounded adaptive-rate controller.
    AdaptivePeer,
    /// No trustworthy media-clock relationship.
    Unlocked,
}

/// Current synchronization evidence reported by the endpoint/control plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleSyncStatus {
    /// Whether the endpoint clock is currently locked.
    pub locked: bool,
    /// Whether the transport can schedule future playout timestamps.
    pub scheduled_playout: bool,
    /// Estimated absolute endpoint skew against the Aurora media timeline.
    pub estimated_skew_micros: Option<u32>,
    /// Selected clock relationship.
    pub clock_source: ModuleClockSource,
}

impl ModuleSyncStatus {
    /// Deterministic docked state for a module driven from the shared dock clock.
    pub const fn docked_locked() -> Self {
        Self {
            locked: true,
            scheduled_playout: true,
            estimated_skew_micros: Some(0),
            clock_source: ModuleClockSource::DockRecovered,
        }
    }
}

/// Static abilities of one detachable module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakerModuleCapabilities {
    /// Maximum independent rendered lanes the module can reproduce.
    pub output_lanes: usize,
    /// Whether the module can consume the shared dock PCM/clock bus.
    pub dock_pcm: bool,
    /// Whether the module can receive scheduled wired network audio.
    pub wired_network_audio: bool,
    /// Whether the module can receive scheduled wireless network audio.
    pub wireless_network_audio: bool,
    /// Whether the module contains a battery and can operate detached.
    pub battery_powered: bool,
}

/// Runtime state for one physical speaker module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakerModuleState {
    /// Stable hardware identity.
    pub module_id: String,
    /// Static module capabilities.
    pub capabilities: SpeakerModuleCapabilities,
    /// Current physical attachment.
    pub attachment: ModuleAttachment,
    /// Current synchronization evidence.
    pub sync: ModuleSyncStatus,
    /// Whether control plane considers the endpoint reachable/healthy.
    pub online: bool,
}

/// Deployment class controls timing strictness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentMode {
    /// Shared immersive movie/TV playback.
    Cinema,
    /// Two-channel or front-stage music playback.
    Stereo,
    /// One module operating by itself.
    Standalone,
    /// Synchronized or loosely synchronized whole-home playback.
    Multiroom,
}

/// Explicit desired roles for one module.
///
/// Aurora never guesses room placement merely because a module was detached.
/// The app, calibration layer, or a stored preset must declare the target roles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleTarget {
    /// Module to activate.
    pub module_id: String,
    /// Logical speaker roles rendered to this module.
    pub roles: Vec<ChannelRole>,
}

/// Desired deployment across currently known modules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentIntent {
    /// Timing/admission class.
    pub mode: DeploymentMode,
    /// Explicit module-role assignments.
    pub targets: Vec<ModuleTarget>,
}

/// One admitted route in a materialized plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleAssignment {
    /// Stable module identity.
    pub module_id: String,
    /// Route selected from physical state/capabilities.
    pub route: ModuleRoute,
    /// Clock relationship admitted for this assignment.
    pub clock_source: ModuleClockSource,
    /// Logical roles carried by the route.
    pub roles: Vec<ChannelRole>,
}

/// Complete fail-closed deployment plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FabricPlan {
    /// Deployment mode.
    pub mode: DeploymentMode,
    /// Deterministic assignments in intent order.
    pub assignments: Vec<ModuleAssignment>,
}

/// One detached module materialized onto Aurora's timestamped network-audio boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkRouteSpec {
    /// Stable physical module identity.
    pub module_id: String,
    /// Source indexes from Aurora's canonical 7.1.4 rendered PCM bus.
    pub source_channel_indices: Vec<usize>,
    /// Prepared network stream shape and clock-discipline requirement.
    pub stream: NetworkStreamConfig,
}

/// Failure while translating an admitted fabric plan into network streams.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum NetworkRouteError {
    /// Network output requires a non-zero block size.
    #[error("network route block size must be greater than zero")]
    InvalidBlockSize,
    /// A requested role is not representable on Aurora's canonical 7.1.4 bus.
    #[error("network route contains a role outside canonical 7.1.4")]
    UnsupportedRole,
    /// Network route has no valid network clock discipline.
    #[error("network route has an unsupported clock source")]
    UnsupportedClockSource,
    /// Shared network timing policy is invalid.
    #[error("invalid network timing policy")]
    Timing(#[from] NetworkTransportError),
}

/// Converts admitted detached assignments into network worker streams.
///
/// DockBus assignments are intentionally skipped because they remain on the
/// synchronous local dock transport. The returned source indexes can feed the
/// existing Aurora network worker/fanout layer without reinterpreting speaker
/// semantics.
pub fn materialize_network_routes(
    plan: &FabricPlan,
    block_frames: usize,
    timing: NetworkTimingPolicy,
) -> Result<Vec<NetworkRouteSpec>, NetworkRouteError> {
    if block_frames == 0 {
        return Err(NetworkRouteError::InvalidBlockSize);
    }
    timing.validate()?;

    let canonical = StandardLayout::SevenOneFour.canonical_roles();
    let mut routes = Vec::new();
    for assignment in &plan.assignments {
        if !matches!(
            assignment.route,
            ModuleRoute::WiredNetwork | ModuleRoute::WirelessNetwork
        ) {
            continue;
        }

        let clock_discipline = match assignment.clock_source {
            ModuleClockSource::Ptp => NetworkClockDiscipline::PtpFollower,
            ModuleClockSource::AdaptivePeer => NetworkClockDiscipline::AdaptiveRateFollower,
            ModuleClockSource::DockRecovered | ModuleClockSource::Unlocked => {
                return Err(NetworkRouteError::UnsupportedClockSource);
            }
        };

        let mut source_channel_indices = Vec::with_capacity(assignment.roles.len());
        for role in &assignment.roles {
            let index = canonical
                .iter()
                .position(|candidate| candidate == role)
                .ok_or(NetworkRouteError::UnsupportedRole)?;
            source_channel_indices.push(index);
        }

        routes.push(NetworkRouteSpec {
            module_id: assignment.module_id.clone(),
            source_channel_indices,
            stream: NetworkStreamConfig {
                format: NetworkAudioFormat {
                    sample_rate: AURORA_NETWORK_MEDIA_RATE,
                    channels: assignment.roles.len(),
                    block_frames,
                },
                clock_discipline,
                timing,
            },
        });
    }
    Ok(routes)
}

/// Stateful control-plane session for hot dock/undock reconciliation.
///
/// The session never keeps a stale active plan after inventory/synchronization
/// changes. If replanning fails, `current_plan()` becomes `None`; the audio
/// control plane must mute/deactivate the affected routes until a new valid
/// plan is admitted.
#[derive(Debug, Clone)]
pub struct FabricSession {
    planner: FabricPlanner,
    inventory: BTreeMap<String, SpeakerModuleState>,
    intent: Option<DeploymentIntent>,
    current_plan: Option<FabricPlan>,
}

impl FabricSession {
    /// Creates an empty session with explicit policy.
    pub fn new(policy: FabricPolicy) -> Self {
        Self {
            planner: FabricPlanner::new(policy),
            inventory: BTreeMap::new(),
            intent: None,
            current_plan: None,
        }
    }

    /// Creates an empty session with default Aurora policy.
    pub fn with_defaults() -> Self {
        Self::new(FabricPolicy::default())
    }

    /// Replaces the requested deployment and immediately reconciles it.
    pub fn set_intent(&mut self, intent: DeploymentIntent) -> Result<&FabricPlan, FabricError> {
        self.intent = Some(intent);
        self.reconcile()
    }

    /// Inserts or replaces one observed physical-module state, then reconciles.
    ///
    /// If no deployment intent has been selected yet, the inventory is updated
    /// and no plan is activated.
    pub fn upsert_module(
        &mut self,
        module: SpeakerModuleState,
    ) -> Result<Option<&FabricPlan>, FabricError> {
        if module.module_id.is_empty() {
            self.current_plan = None;
            return Err(FabricError::InvalidModuleIdentity);
        }
        self.inventory.insert(module.module_id.clone(), module);
        if self.intent.is_none() {
            self.current_plan = None;
            return Ok(None);
        }
        self.reconcile().map(Some)
    }

    /// Removes a module that disappeared from discovery/control-plane state.
    ///
    /// Any active plan is cleared before replanning, so a disappearing endpoint
    /// cannot remain logically active.
    pub fn remove_module(&mut self, module_id: &str) -> Result<Option<&FabricPlan>, FabricError> {
        self.inventory.remove(module_id);
        self.current_plan = None;
        if self.intent.is_none() {
            return Ok(None);
        }
        self.reconcile().map(Some)
    }

    /// Returns the currently admitted plan, or `None` while fail-closed.
    pub fn current_plan(&self) -> Option<&FabricPlan> {
        self.current_plan.as_ref()
    }

    /// Returns one immutable module snapshot.
    pub fn module(&self, module_id: &str) -> Option<&SpeakerModuleState> {
        self.inventory.get(module_id)
    }

    fn reconcile(&mut self) -> Result<&FabricPlan, FabricError> {
        let Some(intent) = self.intent.as_ref() else {
            self.current_plan = None;
            unreachable!("reconcile requires an intent");
        };
        let modules = self.inventory.values().cloned().collect::<Vec<_>>();
        match self.planner.plan(&modules, intent) {
            Ok(plan) => {
                self.current_plan = Some(plan);
                Ok(self.current_plan.as_ref().expect("plan was just installed"))
            }
            Err(error) => {
                self.current_plan = None;
                Err(error)
            }
        }
    }
}

/// Synchronization and routing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FabricPolicy {
    /// Maximum admitted endpoint skew for Cinema.
    pub maximum_cinema_skew_micros: u32,
    /// Maximum admitted endpoint skew for Stereo.
    pub maximum_stereo_skew_micros: u32,
    /// Maximum admitted endpoint skew for Multiroom.
    pub maximum_multiroom_skew_micros: u32,
}

impl Default for FabricPolicy {
    fn default() -> Self {
        Self {
            maximum_cinema_skew_micros: 1_000,
            maximum_stereo_skew_micros: 1_000,
            maximum_multiroom_skew_micros: 10_000,
        }
    }
}

/// Planner failure. All synchronization ambiguity is rejected rather than
/// silently falling back to an unsynchronized route.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FabricError {
    /// Stable module identifier is empty or duplicated.
    #[error("invalid or duplicate speaker module id")]
    InvalidModuleIdentity,
    /// Intent references an unknown module.
    #[error("deployment references unknown module: {0}")]
    UnknownModule(String),
    /// Module is offline.
    #[error("speaker module is offline: {0}")]
    Offline(String),
    /// Requested role list is empty or exceeds module output lanes.
    #[error("invalid output-lane assignment for module: {0}")]
    LaneCapacity(String),
    /// The same canonical role is assigned more than once.
    #[error("speaker role is assigned more than once")]
    DuplicateRole,
    /// Docked module cannot use the dock route safely.
    #[error("docked module cannot use the dock PCM/clock route: {0}")]
    DockRouteUnavailable(String),
    /// Detached module has no admitted transport.
    #[error("detached module has no admitted audio transport: {0}")]
    DetachedRouteUnavailable(String),
    /// Synchronization evidence is insufficient for requested deployment.
    #[error("module synchronization is insufficient: {0}")]
    SynchronizationInsufficient(String),
    /// Standalone deployment must contain exactly one module.
    #[error("standalone deployment requires exactly one module")]
    InvalidStandaloneIntent,
}

/// Stateless deterministic topology planner.
#[derive(Debug, Clone, Copy)]
pub struct FabricPlanner {
    policy: FabricPolicy,
}

impl FabricPlanner {
    /// Creates a planner with explicit timing policy.
    pub const fn new(policy: FabricPolicy) -> Self {
        Self { policy }
    }

    /// Creates a planner with Aurora's conservative defaults.
    pub fn with_defaults() -> Self {
        Self::new(FabricPolicy::default())
    }

    /// Materializes one topology. No I/O or discovery occurs here.
    pub fn plan(
        &self,
        modules: &[SpeakerModuleState],
        intent: &DeploymentIntent,
    ) -> Result<FabricPlan, FabricError> {
        if intent.mode == DeploymentMode::Standalone && intent.targets.len() != 1 {
            return Err(FabricError::InvalidStandaloneIntent);
        }

        let mut inventory = BTreeMap::<&str, &SpeakerModuleState>::new();
        for module in modules {
            if module.module_id.is_empty() || inventory.insert(&module.module_id, module).is_some()
            {
                return Err(FabricError::InvalidModuleIdentity);
            }
        }

        let mut roles = Vec::<ChannelRole>::new();
        let mut assignments = Vec::with_capacity(intent.targets.len());
        for target in &intent.targets {
            let module = inventory
                .get(target.module_id.as_str())
                .copied()
                .ok_or_else(|| FabricError::UnknownModule(target.module_id.clone()))?;
            if !module.online {
                return Err(FabricError::Offline(module.module_id.clone()));
            }
            if target.roles.is_empty() || target.roles.len() > module.capabilities.output_lanes {
                return Err(FabricError::LaneCapacity(module.module_id.clone()));
            }
            for role in &target.roles {
                if roles.contains(role) {
                    return Err(FabricError::DuplicateRole);
                }
                roles.push(role.clone());
            }

            let route = self.select_route(module, intent.mode)?;
            assignments.push(ModuleAssignment {
                module_id: module.module_id.clone(),
                route,
                clock_source: module.sync.clock_source,
                roles: target.roles.clone(),
            });
        }

        Ok(FabricPlan {
            mode: intent.mode,
            assignments,
        })
    }

    fn select_route(
        &self,
        module: &SpeakerModuleState,
        mode: DeploymentMode,
    ) -> Result<ModuleRoute, FabricError> {
        match &module.attachment {
            ModuleAttachment::Docked { .. } => {
                if !module.capabilities.dock_pcm
                    || !module.sync.locked
                    || module.sync.clock_source != ModuleClockSource::DockRecovered
                {
                    return Err(FabricError::DockRouteUnavailable(module.module_id.clone()));
                }
                Ok(ModuleRoute::DockBus)
            }
            ModuleAttachment::Detached => {
                if !module.capabilities.battery_powered && mode != DeploymentMode::Multiroom {
                    return Err(FabricError::DetachedRouteUnavailable(
                        module.module_id.clone(),
                    ));
                }
                let route = if module.capabilities.wired_network_audio {
                    ModuleRoute::WiredNetwork
                } else if module.capabilities.wireless_network_audio {
                    ModuleRoute::WirelessNetwork
                } else {
                    return Err(FabricError::DetachedRouteUnavailable(
                        module.module_id.clone(),
                    ));
                };
                self.validate_detached_sync(module, mode)?;
                Ok(route)
            }
        }
    }

    fn validate_detached_sync(
        &self,
        module: &SpeakerModuleState,
        mode: DeploymentMode,
    ) -> Result<(), FabricError> {
        if !module.sync.locked
            || !module.sync.scheduled_playout
            || module.sync.clock_source == ModuleClockSource::Unlocked
        {
            return Err(FabricError::SynchronizationInsufficient(
                module.module_id.clone(),
            ));
        }
        let skew = module
            .sync
            .estimated_skew_micros
            .ok_or_else(|| FabricError::SynchronizationInsufficient(module.module_id.clone()))?;
        let maximum = match mode {
            DeploymentMode::Cinema => self.policy.maximum_cinema_skew_micros,
            DeploymentMode::Stereo => self.policy.maximum_stereo_skew_micros,
            DeploymentMode::Standalone => u32::MAX,
            DeploymentMode::Multiroom => self.policy.maximum_multiroom_skew_micros,
        };
        if skew > maximum {
            return Err(FabricError::SynchronizationInsufficient(
                module.module_id.clone(),
            ));
        }
        Ok(())
    }
}

/// Convenience preset: one left and one right detached module become surrounds.
pub fn detached_rear_intent(left: &str, right: &str) -> DeploymentIntent {
    DeploymentIntent {
        mode: DeploymentMode::Cinema,
        targets: vec![
            ModuleTarget {
                module_id: left.to_owned(),
                roles: vec![ChannelRole::SurroundLeft],
            },
            ModuleTarget {
                module_id: right.to_owned(),
                roles: vec![ChannelRole::SurroundRight],
            },
        ],
    }
}

/// Convenience preset for a two-module stereo pair.
pub fn stereo_pair_intent(left: &str, right: &str) -> DeploymentIntent {
    DeploymentIntent {
        mode: DeploymentMode::Stereo,
        targets: vec![
            ModuleTarget {
                module_id: left.to_owned(),
                roles: vec![ChannelRole::FrontLeft],
            },
            ModuleTarget {
                module_id: right.to_owned(),
                roles: vec![ChannelRole::FrontRight],
            },
        ],
    }
}

/// Convenience preset for two detachable two-lane Atmos pods.
///
/// Each rear pod carries one horizontal surround lane and one up-firing/top
/// lane. A left/right pair therefore becomes SL+TRL and SR+TRR after being
/// moved behind the listener. The same physical drivers may be assigned
/// different front/top-front roles by a docked soundbar preset.
pub fn detachable_atmos_rear_intent(left: &str, right: &str) -> DeploymentIntent {
    DeploymentIntent {
        mode: DeploymentMode::Cinema,
        targets: vec![
            ModuleTarget {
                module_id: left.to_owned(),
                roles: vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft],
            },
            ModuleTarget {
                module_id: right.to_owned(),
                roles: vec![ChannelRole::SurroundRight, ChannelRole::TopRearRight],
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(
        id: &str,
        attachment: ModuleAttachment,
        sync: ModuleSyncStatus,
    ) -> SpeakerModuleState {
        SpeakerModuleState {
            module_id: id.to_owned(),
            capabilities: SpeakerModuleCapabilities {
                output_lanes: 2,
                dock_pcm: true,
                wired_network_audio: false,
                wireless_network_audio: true,
                battery_powered: true,
            },
            attachment,
            sync,
            online: true,
        }
    }

    fn ptp(skew: u32) -> ModuleSyncStatus {
        ModuleSyncStatus {
            locked: true,
            scheduled_playout: true,
            estimated_skew_micros: Some(skew),
            clock_source: ModuleClockSource::Ptp,
        }
    }

    #[test]
    fn docked_modules_use_shared_dock_clock() {
        let modules = [
            module(
                "left",
                ModuleAttachment::Docked {
                    slot_id: "dock-left".to_owned(),
                },
                ModuleSyncStatus::docked_locked(),
            ),
            module(
                "right",
                ModuleAttachment::Docked {
                    slot_id: "dock-right".to_owned(),
                },
                ModuleSyncStatus::docked_locked(),
            ),
        ];
        let plan = FabricPlanner::with_defaults()
            .plan(&modules, &stereo_pair_intent("left", "right"))
            .unwrap();
        assert!(plan
            .assignments
            .iter()
            .all(|assignment| assignment.route == ModuleRoute::DockBus));
    }

    #[test]
    fn same_modules_can_detach_and_become_synchronized_rears() {
        let modules = [
            module("left", ModuleAttachment::Detached, ptp(180)),
            module("right", ModuleAttachment::Detached, ptp(220)),
        ];
        let plan = FabricPlanner::with_defaults()
            .plan(&modules, &detached_rear_intent("left", "right"))
            .unwrap();
        assert_eq!(plan.assignments[0].route, ModuleRoute::WirelessNetwork);
        assert_eq!(plan.assignments[0].roles, vec![ChannelRole::SurroundLeft]);
        assert_eq!(plan.assignments[1].roles, vec![ChannelRole::SurroundRight]);
    }

    #[test]
    fn detached_rears_materialize_to_canonical_network_bus_indexes() {
        let modules = [
            module("left", ModuleAttachment::Detached, ptp(180)),
            module("right", ModuleAttachment::Detached, ptp(220)),
        ];
        let plan = FabricPlanner::with_defaults()
            .plan(&modules, &detached_rear_intent("left", "right"))
            .unwrap();
        let timing = NetworkTimingPolicy {
            target_latency_frames: 480,
            minimum_latency_frames: 240,
            maximum_latency_frames: 960,
            maximum_rate_correction_ppm: 250.0,
        };
        let routes = materialize_network_routes(&plan, 48, timing).unwrap();

        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].source_channel_indices, vec![4]);
        assert_eq!(routes[1].source_channel_indices, vec![5]);
        assert_eq!(
            routes[0].stream.clock_discipline,
            NetworkClockDiscipline::PtpFollower
        );
        assert_eq!(routes[0].stream.format.channels, 1);
        assert_eq!(routes[0].stream.format.sample_rate, 48_000);
    }

    #[test]
    fn cinema_rejects_unlocked_detached_module() {
        let mut bad_sync = ptp(100);
        bad_sync.locked = false;
        let modules = [
            module("left", ModuleAttachment::Detached, bad_sync),
            module("right", ModuleAttachment::Detached, ptp(100)),
        ];
        assert!(matches!(
            FabricPlanner::with_defaults().plan(
                &modules,
                &detached_rear_intent("left", "right")
            ),
            Err(FabricError::SynchronizationInsufficient(id)) if id == "left"
        ));
    }

    #[test]
    fn cinema_rejects_excessive_wireless_skew() {
        let modules = [
            module("left", ModuleAttachment::Detached, ptp(1_001)),
            module("right", ModuleAttachment::Detached, ptp(100)),
        ];
        assert!(matches!(
            FabricPlanner::with_defaults().plan(
                &modules,
                &detached_rear_intent("left", "right")
            ),
            Err(FabricError::SynchronizationInsufficient(id)) if id == "left"
        ));
    }

    #[test]
    fn duplicate_speaker_role_fails_closed() {
        let modules = [
            module("a", ModuleAttachment::Detached, ptp(100)),
            module("b", ModuleAttachment::Detached, ptp(100)),
        ];
        let intent = DeploymentIntent {
            mode: DeploymentMode::Cinema,
            targets: vec![
                ModuleTarget {
                    module_id: "a".to_owned(),
                    roles: vec![ChannelRole::SurroundLeft],
                },
                ModuleTarget {
                    module_id: "b".to_owned(),
                    roles: vec![ChannelRole::SurroundLeft],
                },
            ],
        };
        assert_eq!(
            FabricPlanner::with_defaults().plan(&modules, &intent),
            Err(FabricError::DuplicateRole)
        );
    }

    #[test]
    fn module_lane_capacity_is_enforced() {
        let mut one_lane = module("mono", ModuleAttachment::Detached, ptp(100));
        one_lane.capabilities.output_lanes = 1;
        let intent = DeploymentIntent {
            mode: DeploymentMode::Stereo,
            targets: vec![ModuleTarget {
                module_id: "mono".to_owned(),
                roles: vec![ChannelRole::FrontLeft, ChannelRole::FrontRight],
            }],
        };
        assert!(matches!(
            FabricPlanner::with_defaults().plan(&[one_lane], &intent),
            Err(FabricError::LaneCapacity(id)) if id == "mono"
        ));
    }

    #[test]
    fn detached_cinema_requires_scheduled_playout() {
        let mut sync = ptp(100);
        sync.scheduled_playout = false;
        let modules = [module("rear", ModuleAttachment::Detached, sync)];
        let intent = DeploymentIntent {
            mode: DeploymentMode::Cinema,
            targets: vec![ModuleTarget {
                module_id: "rear".to_owned(),
                roles: vec![ChannelRole::SurroundLeft],
            }],
        };
        assert!(matches!(
            FabricPlanner::with_defaults().plan(&modules, &intent),
            Err(FabricError::SynchronizationInsufficient(_))
        ));
    }

    #[test]
    fn hot_undock_reconciles_roles_and_sync_loss_mutes_plan() {
        let mut session = FabricSession::with_defaults();
        session
            .upsert_module(module("left", ModuleAttachment::Detached, ptp(120)))
            .unwrap();
        session
            .upsert_module(module("right", ModuleAttachment::Detached, ptp(140)))
            .unwrap();

        let plan = session
            .set_intent(detached_rear_intent("left", "right"))
            .unwrap();
        assert_eq!(plan.assignments.len(), 2);
        assert_eq!(plan.assignments[0].route, ModuleRoute::WirelessNetwork);

        let mut lost_sync = module("left", ModuleAttachment::Detached, ptp(120));
        lost_sync.sync.locked = false;
        assert!(matches!(
            session.upsert_module(lost_sync),
            Err(FabricError::SynchronizationInsufficient(id)) if id == "left"
        ));
        assert!(session.current_plan().is_none());

        let recovered = session
            .upsert_module(module("left", ModuleAttachment::Detached, ptp(90)))
            .unwrap()
            .expect("plan restored after sync recovery");
        assert_eq!(recovered.assignments.len(), 2);
    }

    #[test]
    fn disappearing_module_clears_active_plan() {
        let mut session = FabricSession::with_defaults();
        session
            .upsert_module(module("left", ModuleAttachment::Detached, ptp(100)))
            .unwrap();
        session
            .upsert_module(module("right", ModuleAttachment::Detached, ptp(100)))
            .unwrap();
        session
            .set_intent(detached_rear_intent("left", "right"))
            .unwrap();
        assert!(session.current_plan().is_some());

        assert!(matches!(
            session.remove_module("right"),
            Err(FabricError::UnknownModule(id)) if id == "right"
        ));
        assert!(session.current_plan().is_none());
    }

    #[test]
    fn detachable_two_lane_pods_become_surround_plus_top_rear() {
        let modules = [
            module("left", ModuleAttachment::Detached, ptp(140)),
            module("right", ModuleAttachment::Detached, ptp(160)),
        ];
        let plan = FabricPlanner::with_defaults()
            .plan(&modules, &detachable_atmos_rear_intent("left", "right"))
            .unwrap();

        assert_eq!(
            plan.assignments[0].roles,
            vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft]
        );
        assert_eq!(
            plan.assignments[1].roles,
            vec![ChannelRole::SurroundRight, ChannelRole::TopRearRight]
        );
        assert!(plan
            .assignments
            .iter()
            .all(|assignment| assignment.route == ModuleRoute::WirelessNetwork));
    }

    #[test]
    fn detachable_atmos_pods_map_to_canonical_seven_one_four_indexes() {
        let modules = [
            module("left", ModuleAttachment::Detached, ptp(140)),
            module("right", ModuleAttachment::Detached, ptp(160)),
        ];
        let plan = FabricPlanner::with_defaults()
            .plan(&modules, &detachable_atmos_rear_intent("left", "right"))
            .unwrap();
        let timing = NetworkTimingPolicy {
            target_latency_frames: 480,
            minimum_latency_frames: 240,
            maximum_latency_frames: 960,
            maximum_rate_correction_ppm: 250.0,
        };
        let routes = materialize_network_routes(&plan, 48, timing).unwrap();

        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].source_channel_indices, vec![4, 10]);
        assert_eq!(routes[1].source_channel_indices, vec![5, 11]);
        assert_eq!(routes[0].stream.format.channels, 2);
        assert_eq!(routes[1].stream.format.channels, 2);
    }

    #[test]
    fn standalone_allows_one_explicit_module_only() {
        let modules = [module("portable", ModuleAttachment::Detached, ptp(50))];
        let intent = DeploymentIntent {
            mode: DeploymentMode::Standalone,
            targets: vec![ModuleTarget {
                module_id: "portable".to_owned(),
                roles: vec![ChannelRole::FrontCenter],
            }],
        };
        assert!(FabricPlanner::with_defaults()
            .plan(&modules, &intent)
            .is_ok());

        let invalid = DeploymentIntent {
            mode: DeploymentMode::Standalone,
            targets: vec![],
        };
        assert_eq!(
            FabricPlanner::with_defaults().plan(&modules, &invalid),
            Err(FabricError::InvalidStandaloneIntent)
        );
    }
}
