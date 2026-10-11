//! Control-plane planning for detachable Aurora speaker modules.
//!
//! This crate owns topology, role assignment, dock/wireless route selection,
//! and synchronization admission. It intentionally does not implement Wi-Fi,
//! AVB, charging, DSP, or amplifier drivers. A module is admitted to a
//! cinema plan only when the selected route has explicit synchronization
//! evidence; otherwise planning fails closed.

pub mod endpoint;\n\nuse std::collections::{BTreeMap, BTreeSet};

use aurora_core::{ChannelRole, StandardLayout};
use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport,
    NetworkClockDiscipline, NetworkStreamConfig, NetworkTimingPolicy, NetworkTransportError,
    AURORA_NETWORK_MEDIA_RATE,
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

/// Prepared worker-side fanout from Aurora's canonical 7.1.4 PCM bus to the
/// arbitrary per-module network routes admitted by the modular fabric.
///
/// This is the data-plane bridge for detachable pods such as SL+TRL and
/// SR+TRR. It performs only bounded copies into buffers allocated during
/// preparation; it does not perform network I/O.
pub struct PreparedModuleNetworkFanout {
    routes: Vec<NetworkRouteSpec>,
    input_format: NetworkAudioFormat,
    buffers: Vec<Vec<f32>>,
    current_sequence: Option<u64>,
    current_timestamp: Option<MediaTimestamp>,
    next_sequence: Option<u64>,
    next_timestamp: Option<MediaTimestamp>,
}

impl PreparedModuleNetworkFanout {
    /// Validates route shape and allocates all endpoint buffers before playback.
    pub fn prepare(routes: Vec<NetworkRouteSpec>) -> Result<Self, NetworkFanoutError> {
        let first = routes.first().ok_or(NetworkFanoutError::NoRoutes)?;
        let block_frames = first.stream.format.block_frames;
        if block_frames == 0 {
            return Err(NetworkFanoutError::InvalidRoute);
        }

        let mut seen_modules = BTreeSet::<&str>::new();
        let mut seen_sources = [false; 12];
        let mut buffers = Vec::with_capacity(routes.len());

        for route in &routes {
            if route.module_id.is_empty() || !seen_modules.insert(route.module_id.as_str()) {
                return Err(NetworkFanoutError::InvalidRoute);
            }
            if route.source_channel_indices.is_empty()
                || route.source_channel_indices.len() != route.stream.format.channels
                || route.stream.format.sample_rate != AURORA_NETWORK_MEDIA_RATE
                || route.stream.format.block_frames != block_frames
                || !matches!(
                    route.stream.clock_discipline,
                    NetworkClockDiscipline::PtpFollower
                        | NetworkClockDiscipline::AdaptiveRateFollower
                )
            {
                return Err(NetworkFanoutError::InvalidRoute);
            }

            for &source in &route.source_channel_indices {
                if source >= 12 || seen_sources[source] {
                    return Err(NetworkFanoutError::InvalidRoute);
                }
                seen_sources[source] = true;
            }

            let samples = route
                .stream
                .format
                .samples_per_block()
                .ok_or(NetworkFanoutError::InvalidRoute)?;
            buffers.push(vec![0.0; samples]);
        }

        Ok(Self {
            routes,
            input_format: NetworkAudioFormat {
                sample_rate: AURORA_NETWORK_MEDIA_RATE,
                channels: 12,
                block_frames,
            },
            buffers,
            current_sequence: None,
            current_timestamp: None,
            next_sequence: None,
            next_timestamp: None,
        })
    }

    /// Copies one canonical 12-channel block into all admitted module routes.
    ///
    /// Sequence and presentation timestamp must be exactly continuous inside one
    /// stream epoch. Call reset before accepting an intentional discontinuity.
    pub fn split(&mut self, block: &NetworkAudioBlock<'_>) -> Result<(), NetworkFanoutError> {
        block
            .validate()
            .map_err(|_| NetworkFanoutError::InvalidInput)?;
        if block.format != self.input_format {
            return Err(NetworkFanoutError::InvalidInput);
        }

        if let Some(expected) = self.next_sequence {
            if block.sequence != expected {
                return Err(NetworkFanoutError::TimelineDiscontinuity);
            }
        }
        if let Some(expected) = self.next_timestamp {
            if block.timestamp != expected {
                return Err(NetworkFanoutError::TimelineDiscontinuity);
            }
        }

        for frame in 0..self.input_format.block_frames {
            let input_base = frame * self.input_format.channels;
            for (route_index, route) in self.routes.iter().enumerate() {
                let output_channels = route.source_channel_indices.len();
                let output_base = frame * output_channels;
                let output = &mut self.buffers[route_index];
                for (lane, &source) in route.source_channel_indices.iter().enumerate() {
                    output[output_base + lane] = block.samples[input_base + source];
                }
            }
        }

        self.current_sequence = Some(block.sequence);
        self.current_timestamp = Some(block.timestamp);
        self.next_sequence = Some(
            block
                .sequence
                .checked_add(1)
                .ok_or(NetworkFanoutError::TimelineDiscontinuity)?,
        );
        self.next_timestamp = Some(
            block
                .timestamp
                .checked_advance(block.format.block_frames)
                .map_err(|_| NetworkFanoutError::TimelineDiscontinuity)?,
        );
        Ok(())
    }

    /// Borrows one routed module block after split.
    pub fn module_block(
        &self,
        route_index: usize,
    ) -> Result<NetworkAudioBlock<'_>, NetworkFanoutError> {
        let route = self
            .routes
            .get(route_index)
            .ok_or(NetworkFanoutError::InvalidRouteIndex)?;
        let samples = self
            .buffers
            .get(route_index)
            .ok_or(NetworkFanoutError::InvalidRouteIndex)?;
        let sequence = self
            .current_sequence
            .ok_or(NetworkFanoutError::NoBlockReady)?;
        let timestamp = self
            .current_timestamp
            .ok_or(NetworkFanoutError::NoBlockReady)?;

        Ok(NetworkAudioBlock {
            sequence,
            timestamp,
            format: route.stream.format,
            samples,
        })
    }

    /// Returns the immutable route spec paired with module_block.
    pub fn route(&self, route_index: usize) -> Result<&NetworkRouteSpec, NetworkFanoutError> {
        self.routes
            .get(route_index)
            .ok_or(NetworkFanoutError::InvalidRouteIndex)
    }

    /// Starts a new explicit stream epoch without reallocating route buffers.
    pub fn reset(&mut self) {
        self.current_sequence = None;
        self.current_timestamp = None;
        self.next_sequence = None;
        self.next_timestamp = None;
    }
}

/// Fail-closed errors for modular network PCM fanout.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum NetworkFanoutError {
    #[error("modular network fanout requires at least one route")]
    NoRoutes,
    #[error("invalid modular network route")]
    InvalidRoute,
    #[error("invalid canonical 7.1.4 input block")]
    InvalidInput,
    #[error("invalid modular network route index")]
    InvalidRouteIndex,
    #[error("no modular network block is ready")]
    NoBlockReady,
    #[error("modular network sequence/timestamp discontinuity")]
    TimelineDiscontinuity,
}

/// Worker-side transport coordinator for arbitrary detachable module routes.
///
/// All module transports share the same Aurora input timeline. Any preparation,
/// start, or submission failure invalidates the complete set rather than
/// allowing one detached speaker to continue on a stale epoch.
pub struct ModuleNetworkTransportSet {
    fanout: PreparedModuleNetworkFanout,
    transports: Vec<Box<dyn NetworkAudioTransport>>,
    lifecycle: ModuleNetworkLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModuleNetworkLifecycle {
    New,
    Prepared,
    Started,
}

impl ModuleNetworkTransportSet {
    /// Builds a transport set after validating route count and capabilities.
    pub fn new(
        routes: Vec<NetworkRouteSpec>,
        transports: Vec<Box<dyn NetworkAudioTransport>>,
    ) -> Result<Self, ModuleNetworkTransportSetError> {
        if routes.len() != transports.len() {
            return Err(ModuleNetworkTransportSetError::TransportCount);
        }

        let fanout = PreparedModuleNetworkFanout::prepare(routes)
            .map_err(ModuleNetworkTransportSetError::Fanout)?;

        for (route_index, transport) in transports.iter().enumerate() {
            let route = fanout
                .route(route_index)
                .map_err(ModuleNetworkTransportSetError::Fanout)?;
            let capabilities = transport.capabilities();
            if capabilities.max_channels < route.stream.format.channels
                || !capabilities.scheduled_playout
                || (route.stream.clock_discipline == NetworkClockDiscipline::AdaptiveRateFollower
                    && !capabilities.adaptive_rate_matching)
            {
                return Err(ModuleNetworkTransportSetError::Capability { route_index });
            }
        }

        Ok(Self {
            fanout,
            transports,
            lifecycle: ModuleNetworkLifecycle::New,
        })
    }

    /// Prepares every backend with the exact stream config admitted by fabric.
    pub fn prepare(&mut self) -> Result<(), ModuleNetworkTransportSetError> {
        if self.lifecycle == ModuleNetworkLifecycle::Started {
            return Err(ModuleNetworkTransportSetError::InvalidLifecycle);
        }

        for route_index in 0..self.transports.len() {
            let config = self
                .fanout
                .route(route_index)
                .map_err(ModuleNetworkTransportSetError::Fanout)?
                .stream;
            if let Err(source) = self.transports[route_index].prepare(config) {
                for prepared in 0..route_index {
                    self.transports[prepared].reset();
                }
                self.fanout.reset();
                self.lifecycle = ModuleNetworkLifecycle::New;
                return Err(ModuleNetworkTransportSetError::Endpoint {
                    route_index,
                    source,
                });
            }
        }

        self.fanout.reset();
        self.lifecycle = ModuleNetworkLifecycle::Prepared;
        Ok(())
    }

    /// Starts all module transports atomically with rollback on partial start.
    pub fn start(&mut self) -> Result<(), ModuleNetworkTransportSetError> {
        if self.lifecycle != ModuleNetworkLifecycle::Prepared {
            return Err(ModuleNetworkTransportSetError::InvalidLifecycle);
        }

        for route_index in 0..self.transports.len() {
            if let Err(source) = self.transports[route_index].start() {
                for started in 0..route_index {
                    let _ = self.transports[started].stop();
                    self.transports[started].reset();
                }
                self.fanout.reset();
                return Err(ModuleNetworkTransportSetError::Endpoint {
                    route_index,
                    source,
                });
            }
        }

        self.fanout.reset();
        self.lifecycle = ModuleNetworkLifecycle::Started;
        Ok(())
    }

    /// Routes one canonical 7.1.4 block and submits every module block.
    pub fn submit(
        &mut self,
        block: &NetworkAudioBlock<'_>,
    ) -> Result<(), ModuleNetworkTransportSetError> {
        if self.lifecycle != ModuleNetworkLifecycle::Started {
            return Err(ModuleNetworkTransportSetError::InvalidLifecycle);
        }

        self.fanout
            .split(block)
            .map_err(ModuleNetworkTransportSetError::Fanout)?;

        for route_index in 0..self.transports.len() {
            let module_block = self
                .fanout
                .module_block(route_index)
                .map_err(ModuleNetworkTransportSetError::Fanout)?;
            if let Err(source) = self.transports[route_index].submit(module_block) {
                self.abort_stream();
                return Err(ModuleNetworkTransportSetError::Endpoint {
                    route_index,
                    source,
                });
            }
        }
        Ok(())
    }

    /// Stops every active module transport and retains prepared capacity.
    pub fn stop(&mut self) -> Result<(), ModuleNetworkTransportSetError> {
        if self.lifecycle != ModuleNetworkLifecycle::Started {
            return Err(ModuleNetworkTransportSetError::InvalidLifecycle);
        }

        let mut first_error = None;
        for route_index in 0..self.transports.len() {
            if let Err(source) = self.transports[route_index].stop() {
                if first_error.is_none() {
                    first_error = Some(ModuleNetworkTransportSetError::Endpoint {
                        route_index,
                        source,
                    });
                }
            }
            self.transports[route_index].reset();
        }
        self.fanout.reset();
        self.lifecycle = ModuleNetworkLifecycle::Prepared;

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Clears transport and media-timeline state without changing capacities.
    pub fn reset(&mut self) {
        for transport in &mut self.transports {
            transport.reset();
        }
        self.fanout.reset();
    }

    fn abort_stream(&mut self) {
        for transport in &mut self.transports {
            let _ = transport.stop();
            transport.reset();
        }
        self.fanout.reset();
        self.lifecycle = ModuleNetworkLifecycle::New;
    }
}

/// Fail-closed errors for the detachable module transport set.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ModuleNetworkTransportSetError {
    #[error("module network transport count does not match route count")]
    TransportCount,
    #[error("module network fanout failed: {0}")]
    Fanout(NetworkFanoutError),
    #[error("module network transport has incompatible capabilities at route {route_index}")]
    Capability { route_index: usize },
    #[error("module network endpoint {route_index} failed: {source}")]
    Endpoint {
        route_index: usize,
        source: NetworkTransportError,
    },
    #[error("invalid module network transport lifecycle")]
    InvalidLifecycle,
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

/// Convenience preset for two docked two-lane Atmos wing modules.
///
/// While attached to the bar, the left wing carries FL+TFL and the right wing
/// carries FR+TFR over the shared dock clock. The same physical pods can later
/// be reassigned by detachable_atmos_rear_intent after moving behind the listener.
pub fn docked_atmos_wing_intent(left: &str, right: &str) -> DeploymentIntent {
    DeploymentIntent {
        mode: DeploymentMode::Cinema,
        targets: vec![
            ModuleTarget {
                module_id: left.to_owned(),
                roles: vec![ChannelRole::FrontLeft, ChannelRole::TopFrontLeft],
            },
            ModuleTarget {
                module_id: right.to_owned(),
                roles: vec![ChannelRole::FrontRight, ChannelRole::TopFrontRight],
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

/// One ordered control command for a detachable endpoint handover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointHandoverCommand {
    /// Silence the endpoint before changing route, clock domain or speaker role.
    Mute { module_id: String, epoch: u64 },
    /// Install the next admitted assignment while the endpoint remains muted.
    Prepare {
        assignment: ModuleAssignment,
        epoch: u64,
    },
    /// Arm the prepared route for one future Aurora media-frame boundary.
    Arm {
        module_id: String,
        epoch: u64,
        start_frame: u64,
    },
    /// Make the armed assignment active. Audio remains gated until start_frame.
    Commit { module_id: String, epoch: u64 },
}

/// Deterministic make-before-play handover across all changed modules.
///
/// Commands are intentionally grouped globally as mute -> prepare -> arm ->
/// commit. No changed endpoint is allowed to begin the new route while another
/// changed endpoint is still playing the old topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoverPlan {
    pub epoch: u64,
    pub start_frame: u64,
    pub commands: Vec<EndpointHandoverCommand>,
}

/// Errors returned by handover planning or endpoint command application.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HandoverError {
    #[error("handover epoch must be non-zero")]
    InvalidEpoch,
    #[error("handover command addressed the wrong module")]
    WrongModule,
    #[error("handover command uses a stale or unexpected epoch")]
    EpochMismatch,
    #[error("handover command arrived out of order")]
    InvalidPhase,
}

/// Build a deterministic handover from an optional active plan to a new plan.
///
/// Unchanged assignments are left running. Changed/new assignments are muted,
/// prepared, armed to the same future media frame, then committed. Assignments
/// removed from the next plan receive only Mute and remain fail-closed.
pub fn build_handover_plan(
    previous: Option<&FabricPlan>,
    next: &FabricPlan,
    epoch: u64,
    start_frame: u64,
) -> Result<HandoverPlan, HandoverError> {
    if epoch == 0 {
        return Err(HandoverError::InvalidEpoch);
    }

    let mut ids = BTreeSet::<String>::new();
    if let Some(previous) = previous {
        for assignment in &previous.assignments {
            ids.insert(assignment.module_id.clone());
        }
    }
    for assignment in &next.assignments {
        ids.insert(assignment.module_id.clone());
    }

    fn lookup<'a>(plan: Option<&'a FabricPlan>, id: &str) -> Option<&'a ModuleAssignment> {
        plan.and_then(|plan| {
            plan.assignments
                .iter()
                .find(|assignment| assignment.module_id == id)
        })
    }

    let mut changed = Vec::<(String, Option<ModuleAssignment>)>::new();
    for id in ids {
        let old = lookup(previous, &id);
        let new = lookup(Some(next), &id);
        if old != new {
            changed.push((id, new.cloned()));
        }
    }

    let mut commands = Vec::new();
    for (module_id, _) in &changed {
        commands.push(EndpointHandoverCommand::Mute {
            module_id: module_id.clone(),
            epoch,
        });
    }
    for (_, assignment) in &changed {
        if let Some(assignment) = assignment {
            commands.push(EndpointHandoverCommand::Prepare {
                assignment: assignment.clone(),
                epoch,
            });
        }
    }
    for (module_id, assignment) in &changed {
        if assignment.is_some() {
            commands.push(EndpointHandoverCommand::Arm {
                module_id: module_id.clone(),
                epoch,
                start_frame,
            });
        }
    }
    for (module_id, assignment) in &changed {
        if assignment.is_some() {
            commands.push(EndpointHandoverCommand::Commit {
                module_id: module_id.clone(),
                epoch,
            });
        }
    }

    Ok(HandoverPlan {
        epoch,
        start_frame,
        commands,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndpointHandoverPhase {
    Idle,
    Muted,
    Prepared,
    Armed,
}

/// Endpoint-side fail-closed state machine for applying handover commands.
///
/// The physical endpoint firmware can mirror this contract. A stale epoch or
/// out-of-order Prepare/Arm/Commit is rejected and cannot unmute the speaker.
#[derive(Debug, Clone)]
pub struct EndpointHandoverState {
    module_id: String,
    last_epoch: u64,
    phase: EndpointHandoverPhase,
    pending: Option<ModuleAssignment>,
    active: Option<ModuleAssignment>,
    armed_start_frame: Option<u64>,
    muted: bool,
}

impl EndpointHandoverState {
    pub fn new(module_id: impl Into<String>) -> Self {
        Self {
            module_id: module_id.into(),
            last_epoch: 0,
            phase: EndpointHandoverPhase::Idle,
            pending: None,
            active: None,
            armed_start_frame: None,
            muted: true,
        }
    }

    pub fn apply(&mut self, command: &EndpointHandoverCommand) -> Result<(), HandoverError> {
        match command {
            EndpointHandoverCommand::Mute { module_id, epoch } => {
                self.require_module(module_id)?;
                if *epoch == 0 || *epoch <= self.last_epoch {
                    return Err(HandoverError::EpochMismatch);
                }
                self.last_epoch = *epoch;
                self.phase = EndpointHandoverPhase::Muted;
                self.pending = None;
                self.armed_start_frame = None;
                self.muted = true;
                Ok(())
            }
            EndpointHandoverCommand::Prepare { assignment, epoch } => {
                self.require_module(&assignment.module_id)?;
                self.require_epoch(*epoch)?;
                if self.phase != EndpointHandoverPhase::Muted {
                    return Err(HandoverError::InvalidPhase);
                }
                self.pending = Some(assignment.clone());
                self.phase = EndpointHandoverPhase::Prepared;
                Ok(())
            }
            EndpointHandoverCommand::Arm {
                module_id,
                epoch,
                start_frame,
            } => {
                self.require_module(module_id)?;
                self.require_epoch(*epoch)?;
                if self.phase != EndpointHandoverPhase::Prepared {
                    return Err(HandoverError::InvalidPhase);
                }
                self.armed_start_frame = Some(*start_frame);
                self.phase = EndpointHandoverPhase::Armed;
                Ok(())
            }
            EndpointHandoverCommand::Commit { module_id, epoch } => {
                self.require_module(module_id)?;
                self.require_epoch(*epoch)?;
                if self.phase != EndpointHandoverPhase::Armed {
                    return Err(HandoverError::InvalidPhase);
                }
                self.active = self.pending.take();
                self.phase = EndpointHandoverPhase::Idle;
                self.muted = false;
                Ok(())
            }
        }
    }

    pub fn is_muted(&self) -> bool {
        self.muted
    }

    pub fn active_assignment(&self) -> Option<&ModuleAssignment> {
        self.active.as_ref()
    }

    pub fn armed_start_frame(&self) -> Option<u64> {
        self.armed_start_frame
    }

    pub fn last_epoch(&self) -> u64 {
        self.last_epoch
    }

    fn require_module(&self, module_id: &str) -> Result<(), HandoverError> {
        if self.module_id == module_id {
            Ok(())
        } else {
            Err(HandoverError::WrongModule)
        }
    }

    fn require_epoch(&self, epoch: u64) -> Result<(), HandoverError> {
        if epoch != 0 && epoch == self.last_epoch {
            Ok(())
        } else {
            Err(HandoverError::EpochMismatch)
        }
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
    fn same_two_lane_pods_are_front_plus_top_front_while_docked() {
        let modules = [
            module(
                "left",
                ModuleAttachment::Docked {
                    slot_id: "left-edge".to_owned(),
                },
                ModuleSyncStatus::docked_locked(),
            ),
            module(
                "right",
                ModuleAttachment::Docked {
                    slot_id: "right-edge".to_owned(),
                },
                ModuleSyncStatus::docked_locked(),
            ),
        ];
        let plan = FabricPlanner::with_defaults()
            .plan(&modules, &docked_atmos_wing_intent("left", "right"))
            .unwrap();

        assert_eq!(
            plan.assignments[0].roles,
            vec![ChannelRole::FrontLeft, ChannelRole::TopFrontLeft]
        );
        assert_eq!(
            plan.assignments[1].roles,
            vec![ChannelRole::FrontRight, ChannelRole::TopFrontRight]
        );
        assert!(plan
            .assignments
            .iter()
            .all(|assignment| assignment.route == ModuleRoute::DockBus));
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

#[cfg(test)]
mod handover_tests {
    use super::*;

    fn assignment(
        id: &str,
        route: ModuleRoute,
        roles: Vec<ChannelRole>,
        clock_source: ModuleClockSource,
    ) -> ModuleAssignment {
        ModuleAssignment {
            module_id: id.to_owned(),
            route,
            clock_source,
            roles,
        }
    }

    #[test]
    fn dock_to_wireless_role_change_is_muted_prepared_armed_then_committed() {
        let previous = FabricPlan {
            mode: DeploymentMode::Cinema,
            assignments: vec![assignment(
                "left-pod",
                ModuleRoute::DockBus,
                vec![ChannelRole::FrontLeft, ChannelRole::TopFrontLeft],
                ModuleClockSource::DockRecovered,
            )],
        };
        let next = FabricPlan {
            mode: DeploymentMode::Cinema,
            assignments: vec![assignment(
                "left-pod",
                ModuleRoute::WirelessNetwork,
                vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft],
                ModuleClockSource::Ptp,
            )],
        };

        let handover = build_handover_plan(Some(&previous), &next, 7, 96_000).unwrap();
        assert_eq!(handover.commands.len(), 4);
        assert!(matches!(
            handover.commands[0],
            EndpointHandoverCommand::Mute { epoch: 7, .. }
        ));
        assert!(matches!(
            handover.commands[1],
            EndpointHandoverCommand::Prepare { epoch: 7, .. }
        ));
        assert!(matches!(
            handover.commands[2],
            EndpointHandoverCommand::Arm {
                epoch: 7,
                start_frame: 96_000,
                ..
            }
        ));
        assert!(matches!(
            handover.commands[3],
            EndpointHandoverCommand::Commit { epoch: 7, .. }
        ));

        let mut endpoint = EndpointHandoverState::new("left-pod");
        for command in &handover.commands {
            endpoint.apply(command).unwrap();
        }
        assert!(!endpoint.is_muted());
        assert_eq!(endpoint.last_epoch(), 7);
        assert_eq!(endpoint.armed_start_frame(), Some(96_000));
        assert_eq!(
            endpoint.active_assignment().unwrap().roles,
            vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft]
        );
    }

    #[test]
    fn all_changed_modules_are_muted_before_any_prepare() {
        let previous = FabricPlan {
            mode: DeploymentMode::Cinema,
            assignments: vec![
                assignment(
                    "left",
                    ModuleRoute::DockBus,
                    vec![ChannelRole::FrontLeft],
                    ModuleClockSource::DockRecovered,
                ),
                assignment(
                    "right",
                    ModuleRoute::DockBus,
                    vec![ChannelRole::FrontRight],
                    ModuleClockSource::DockRecovered,
                ),
            ],
        };
        let next = FabricPlan {
            mode: DeploymentMode::Cinema,
            assignments: vec![
                assignment(
                    "left",
                    ModuleRoute::WirelessNetwork,
                    vec![ChannelRole::SurroundLeft],
                    ModuleClockSource::Ptp,
                ),
                assignment(
                    "right",
                    ModuleRoute::WirelessNetwork,
                    vec![ChannelRole::SurroundRight],
                    ModuleClockSource::Ptp,
                ),
            ],
        };

        let handover = build_handover_plan(Some(&previous), &next, 2, 48_000).unwrap();
        assert!(handover.commands[..2]
            .iter()
            .all(|command| matches!(command, EndpointHandoverCommand::Mute { .. })));
        assert!(handover.commands[2..4]
            .iter()
            .all(|command| matches!(command, EndpointHandoverCommand::Prepare { .. })));
    }

    #[test]
    fn stale_epoch_and_out_of_order_commit_fail_closed() {
        let mut endpoint = EndpointHandoverState::new("rear");
        endpoint
            .apply(&EndpointHandoverCommand::Mute {
                module_id: "rear".to_owned(),
                epoch: 3,
            })
            .unwrap();

        assert_eq!(
            endpoint.apply(&EndpointHandoverCommand::Commit {
                module_id: "rear".to_owned(),
                epoch: 3,
            }),
            Err(HandoverError::InvalidPhase)
        );
        assert!(endpoint.is_muted());

        assert_eq!(
            endpoint.apply(&EndpointHandoverCommand::Mute {
                module_id: "rear".to_owned(),
                epoch: 2,
            }),
            Err(HandoverError::EpochMismatch)
        );
        assert!(endpoint.is_muted());
    }

    #[test]
    fn removed_module_receives_mute_only_and_stays_muted() {
        let previous = FabricPlan {
            mode: DeploymentMode::Cinema,
            assignments: vec![assignment(
                "rear",
                ModuleRoute::WirelessNetwork,
                vec![ChannelRole::SurroundLeft],
                ModuleClockSource::Ptp,
            )],
        };
        let next = FabricPlan {
            mode: DeploymentMode::Cinema,
            assignments: vec![],
        };

        let handover = build_handover_plan(Some(&previous), &next, 9, 144_000).unwrap();
        assert_eq!(handover.commands.len(), 1);
        assert!(matches!(
            handover.commands[0],
            EndpointHandoverCommand::Mute { epoch: 9, .. }
        ));

        let mut endpoint = EndpointHandoverState::new("rear");
        endpoint.apply(&handover.commands[0]).unwrap();
        assert!(endpoint.is_muted());
        assert!(endpoint.active_assignment().is_none());
    }
}
