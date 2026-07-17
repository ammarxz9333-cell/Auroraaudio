use std::collections::BTreeSet;

use aurora_diagnostics::{Severity, TruthSource};
use serde::{Deserialize, Serialize};

/// Root schema metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaMetadata {
    /// Stable schema name.
    pub schema_name: String,
    /// Version represented by this document.
    pub schema_version: u16,
    /// Oldest reader version that may consume this document.
    pub minimum_reader_version: u16,
    /// Optional provenance omitted from canonical equality and serialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
}

/// Intended engine operating mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatingMode {
    /// Offline file processing.
    Offline,
    /// Deterministic simulation.
    Simulation,
    /// Future live operation intent; this crate does not activate it.
    LiveIntent,
}

/// Startup policy intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupBehavior {
    /// Require explicit start by the control plane.
    Manual,
    /// Start after all configuration validates.
    AfterValidation,
}

/// Shutdown policy intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownBehavior {
    /// Drain bounded pending work before stopping.
    Drain,
    /// Stop without accepting new work.
    Immediate,
}

/// Engine-level control-plane intent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineConfiguration {
    /// Stable logical engine identifier.
    pub id: String,
    /// Intended operating mode.
    pub operating_mode: OperatingMode,
    /// Intended startup behavior.
    pub startup_behavior: StartupBehavior,
    /// Intended shutdown behavior.
    pub shutdown_behavior: ShutdownBehavior,
    /// Stable reference to an externally defined recovery policy.
    pub recovery_policy: String,
}

/// Supported PCM sample representation intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleFormatIntent {
    /// 32-bit IEEE floating point.
    Float32,
    /// Signed 16-bit PCM.
    Pcm16,
    /// Signed packed 24-bit PCM.
    Pcm24,
    /// Signed 32-bit PCM.
    Pcm32,
}

/// Explicit format fallback behavior.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum FormatFallbackPolicy {
    /// Reject any non-exact format.
    Reject,
    /// Permit only the listed sample rates; no implicit fallback is allowed.
    AllowListedSampleRates {
        /// Explicit accepted alternatives.
        sample_rates: Vec<u32>,
    },
}

/// Requested audio format; this is not a negotiated device format.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioFormatIntent {
    /// Requested sample rate.
    pub sample_rate: u32,
    /// Requested channel count.
    pub channel_count: u16,
    /// Requested sample representation.
    pub sample_format: SampleFormatIntent,
    /// Preferred callback frames.
    pub callback_frames: u32,
    /// Explicit behavior when the request is unavailable.
    pub fallback_policy: FormatFallbackPolicy,
}

/// Audio backend selection intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendIntent {
    /// Use the deterministic Aurora virtual backend.
    Virtual,
    /// Use CPAL when a later control-plane integration resolves the intent.
    Cpal,
    /// Offline operation without a device backend.
    Offline,
}

/// Device direction.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceDirection {
    /// Capture device intent.
    Input,
    /// Playback device intent.
    Output,
}

/// Explicit behavior for selectors that may match multiple devices.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmbiguityPolicy {
    /// Reject any selector with more than one match.
    Reject,
    /// Require a stable identifier match.
    RequireStableIdentifier,
    /// Unsafe compatibility value rejected by validation.
    AllowFirst,
}

/// Device-selection intent that does not claim device existence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceSelectionIntent {
    /// Stable backend identifier when available.
    pub stable_id: Option<String>,
    /// Optional user-facing selector text.
    pub friendly_name: Option<String>,
    /// Intended backend.
    pub backend: BackendIntent,
    /// Intended stream direction.
    pub direction: DeviceDirection,
    /// Explicit ambiguity behavior.
    pub ambiguity_policy: AmbiguityPolicy,
}

/// Stable logical channel identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelIdentity {
    /// Stable channel identifier.
    pub id: String,
    /// Human-readable label.
    pub label: String,
}

/// One explicit input-to-output channel assignment.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelRoute {
    /// Referenced input channel identifier.
    pub input: String,
    /// Referenced output channel identifier.
    pub output: String,
}

/// Explicit routing graph intent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingConfiguration {
    /// Declared input identities.
    pub inputs: Vec<ChannelIdentity>,
    /// Declared output identities.
    pub outputs: Vec<ChannelIdentity>,
    /// Explicit assignments.
    pub routes: Vec<ChannelRoute>,
    /// Output IDs intentionally inactive.
    #[serde(default)]
    pub inactive_outputs: Vec<String>,
}

/// Layout vocabulary supported by Configuration System 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKind {
    /// Canonical FL/FR layout.
    Stereo,
    /// Canonical FL/FR/FC/LFE/SL/SR layout.
    Surround51,
    /// Canonical FL/FR/FC/LFE/SL/SR/SBL/SBR layout.
    Surround71,
    /// Explicit irregular horizontal layout.
    CustomHorizontal,
}

/// One speaker in a deterministic layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeakerConfiguration {
    /// Stable speaker identifier.
    pub id: String,
    /// Horizontal angle in degrees in `-180..=180`.
    pub azimuth_degrees: f32,
    /// Reserved elevation metadata; rendering remains unsupported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation_degrees: Option<f32>,
    /// Stable role such as `FL` or a custom label.
    pub role: String,
    /// Human-readable label.
    pub label: String,
    /// Whether the speaker participates in the intended layout.
    pub active: bool,
}

/// Speaker-layout intent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeakerLayoutConfiguration {
    /// Stable layout identifier.
    pub id: String,
    /// Layout vocabulary.
    pub kind: LayoutKind,
    /// Speakers normalized by stable identifier after validation.
    pub speakers: Vec<SpeakerConfiguration>,
    /// Reserved flag; `true` is rejected because elevation is out of scope.
    #[serde(default)]
    pub elevation_rendering: bool,
}

/// Existing renderer selection vocabulary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RendererConfiguration {
    /// Existing Basic inverse-distance renderer.
    Basic,
    /// Existing Phase 3A point-source VBAP renderer.
    PointSourceVbap,
    /// Existing Phase 3B horizontal spread renderer.
    HorizontalSpread {
        /// Normalized spread in `0.0..=1.0`.
        spread: f32,
    },
}

/// Bounded audio transport policy intent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BufferingPolicy {
    /// Target fill in frames.
    pub target_fill_frames: u32,
    /// Minimum fill in frames.
    pub minimum_fill_frames: u32,
    /// Maximum fill in frames.
    pub maximum_fill_frames: u32,
    /// Ring capacity in frames.
    pub ring_capacity_frames: u32,
    /// Require preallocated runtime storage.
    pub require_preallocated_storage: bool,
}

/// Structured diagnostics output intent.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsOutputMode {
    /// Deterministic JSON output.
    Json,
    /// Deterministic human-readable output.
    Human,
    /// Both formats on the control plane.
    Both,
}

/// Configuration redaction policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionMode {
    /// Remove sensitive and machine-specific identifiers.
    Strict,
    /// Preserve stable IDs but remove friendly names and sensitive values.
    StableIdentifiers,
}

/// Diagnostics configuration reusing Aurora's accepted taxonomy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsPolicy {
    /// Minimum retained severity.
    pub severity_threshold: Severity,
    /// Maximum retained events.
    pub retention_events: usize,
    /// Requested control-thread output format.
    pub output_mode: DiagnosticsOutputMode,
    /// Redaction behavior.
    pub redaction_mode: RedactionMode,
    /// Allowed truth sources, deterministically ordered after validation.
    pub allowed_truth_sources: BTreeSet<TruthSource>,
}

/// Deterministic simulation duration vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationDurationClass {
    /// Short PR smoke scenario.
    Smoke,
    /// Standard bounded campaign scenario.
    Standard,
    /// Accelerated long-duration scenario.
    Soak,
}

/// Bounded deterministic simulation-profile intent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulationProfile {
    /// Deterministic replay seed.
    pub seed: u64,
    /// Stable scenario family.
    pub scenario_family: String,
    /// Intended duration class.
    pub duration_class: SimulationDurationClass,
    /// Configured clock mismatch in parts per million.
    pub drift_ppm: f64,
    /// Configured maximum callback jitter in frames.
    pub jitter_frames: u32,
    /// Stable fault-profile identifier.
    pub fault_profile: String,
    /// Stable replay identifier independent of paths.
    pub replay_id: String,
}

/// Complete versioned Aurora control-plane configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuroraConfiguration {
    /// Schema metadata.
    pub schema: SchemaMetadata,
    /// Engine intent.
    pub engine: EngineConfiguration,
    /// Audio format intent.
    pub audio_format: AudioFormatIntent,
    /// Optional input device intent.
    pub input_device: Option<DeviceSelectionIntent>,
    /// Optional output device intent.
    pub output_device: Option<DeviceSelectionIntent>,
    /// Explicit routing intent.
    pub routing: RoutingConfiguration,
    /// Speaker layout intent.
    pub speaker_layout: SpeakerLayoutConfiguration,
    /// Renderer selection intent.
    pub renderer: RendererConfiguration,
    /// Bounded buffering policy.
    pub buffering: BufferingPolicy,
    /// Diagnostics policy.
    pub diagnostics: DiagnosticsPolicy,
    /// Optional deterministic simulation profile.
    pub simulation: Option<SimulationProfile>,
}
