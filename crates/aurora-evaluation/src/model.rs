use aurora_core::Vector3;
use serde::{Deserialize, Serialize};

/// Current machine-readable evaluation schema.
pub const EVALUATION_SCHEMA_VERSION: u16 = 2;
/// Maximum output channels retained by one evaluation.
pub const MAX_CHANNELS: usize = 32;
/// Maximum input frames retained by one evaluation.
pub const MAX_FRAMES: usize = 2_880_000;
/// Maximum processing blocks retained by one evaluation.
pub const MAX_BLOCKS: usize = 65_536;
/// Maximum fixed-direction probes in one fixture.
pub const MAX_PROBES: usize = 32;
/// Maximum external validation-hook results.
pub const MAX_HOOKS: usize = 64;
/// Maximum UTF-8 bytes in a supplied string.
pub const MAX_STRING_BYTES: usize = 512;

/// Fixed-direction deterministic renderer probe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProbeDefinition {
    /// Stable probe identifier.
    pub id: String,
    /// Source position in meters.
    pub position: Vector3,
}

/// Versioned deterministic evaluation fixture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluationFixture {
    /// Fixture schema version.
    pub schema_version: u16,
    /// Stable scenario identifier.
    pub scenario: String,
    /// Canonically ordered fixed-direction probes.
    pub probes: Vec<ProbeDefinition>,
}

/// Evidence state used throughout machine-readable reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    /// All required criteria represented by this status passed.
    Pass,
    /// At least one required criterion failed.
    Fail,
    /// The criterion was not observed by this run.
    NotObserved,
}

/// Configurable thresholds used by deterministic checks and advisory host observations.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluationThresholds {
    /// Maximum absolute error from unit gain power.
    pub max_gain_power_error: f32,
    /// Maximum gain change for one channel between adjacent blocks.
    pub max_gain_discontinuity: f32,
    /// Maximum delay change in samples between adjacent blocks.
    pub max_delay_discontinuity_samples: f32,
    /// Advisory maximum adjacent rendered-sample delta.
    ///
    /// This is a signal-slope proxy and is not a general click detector.
    pub max_audio_discontinuity: f32,
    /// Advisory maximum host-observed `render_gains` p99 cost in nanoseconds.
    pub max_renderer_p99_ns: u64,
}

impl Default for EvaluationThresholds {
    fn default() -> Self {
        Self {
            max_gain_power_error: 0.001,
            max_gain_discontinuity: 1.0,
            max_delay_discontinuity_samples: 64.0,
            max_audio_discontinuity: 1.0,
            max_renderer_p99_ns: 5_000_000,
        }
    }
}

/// Immutable setup values for one renderer evaluation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluationConfig {
    /// Stable renderer identifier.
    pub renderer_id: String,
    /// Stable evaluation scenario identifier.
    pub scenario_id: String,
    /// Input sample rate.
    pub sample_rate: u32,
    /// Processing block size.
    pub block_size: usize,
    /// Maximum delay accepted by the existing delay processor.
    pub max_delay_samples: f32,
    /// Whether captured renderer delays are applied to the WAV output.
    pub apply_delays: bool,
    /// Validation and advisory thresholds.
    pub thresholds: EvaluationThresholds,
    /// Commit evaluated by this run.
    pub commit_sha: String,
    /// Reproducible command supplied by the caller.
    pub command: String,
    /// Fixed-direction deterministic probes.
    pub probes: Vec<ProbeDefinition>,
    /// Optional validation evidence from other Aurora-owned subsystems.
    pub hooks: Vec<HookEvidence>,
    /// Optional observed steady-state allocation count.
    pub steady_state_allocations: Option<u64>,
}

/// Renderer identity and configuration provenance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RendererMetadata {
    /// Stable renderer identifier.
    pub renderer_id: String,
    /// Enabled output channel count.
    pub output_channels: usize,
    /// Deterministic FNV-1a regression fingerprint of canonical configuration JSON.
    ///
    /// This is not a cryptographic integrity digest.
    pub configuration_hash_fnv1a64: String,
}

/// Reproduction and truth-source metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    /// Stable evaluation scenario identifier.
    pub scenario_id: String,
    /// Commit supplied or resolved by the control-thread caller.
    pub commit_sha: String,
    /// Exact reproducible command.
    pub command: String,
    /// Truth source for deterministic correctness evidence.
    pub correctness_truth_source: String,
    /// Truth source for host timing evidence.
    pub performance_truth_source: String,
}

/// One block/channel gain and delay trajectory point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GainTrajectoryPoint {
    /// Zero-based processing block index.
    pub block_index: usize,
    /// Midpoint frame sampled for renderer state.
    pub frame_index: usize,
    /// Output channel index.
    pub channel_index: usize,
    /// Linear gain.
    pub gain: f32,
    /// Renderer-provided delay in samples.
    pub delay_samples: f32,
}

/// Result of one fixed-direction probe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
    /// Stable probe identifier.
    pub id: String,
    /// Source position in meters.
    pub position: Vector3,
    /// Channel gains in configured output order.
    pub gains: Vec<f32>,
    /// Channel delays in configured output order.
    pub delays_samples: Vec<f32>,
}

/// Maximum observed changes from deterministic generated output.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiscontinuityMetrics {
    /// Largest adjacent-block gain change.
    pub maximum_gain_delta: f32,
    /// Largest adjacent-block delay change in samples.
    pub maximum_delay_delta_samples: f32,
    /// Largest adjacent rendered-sample change.
    ///
    /// This is a signal-slope proxy, not proof of click-free output.
    pub maximum_audio_sample_delta: f32,
}

/// Deterministic rendered-audio facts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioMetrics {
    /// Frames rendered.
    pub frames: usize,
    /// Output channels rendered.
    pub channels: usize,
    /// Peak absolute value per channel.
    pub peak_per_channel: Vec<f32>,
    /// Whether any generated sample was non-finite.
    pub contains_non_finite: bool,
    /// Whether any generated sample exceeded unit magnitude.
    pub clipping_detected: bool,
    /// Deterministic FNV-1a regression fingerprint over planar `f32` bytes.
    ///
    /// This is not a cryptographic integrity digest.
    pub audio_checksum_fnv1a64: String,
}

/// Host-observed processing-cost percentiles.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Number of timed `render_gains` calls.
    pub samples: usize,
    /// p50 `render_gains` call duration in nanoseconds.
    pub renderer_p50_ns: u64,
    /// p95 `render_gains` call duration in nanoseconds.
    pub renderer_p95_ns: u64,
    /// p99 `render_gains` call duration in nanoseconds.
    pub renderer_p99_ns: u64,
    /// Maximum `render_gains` call duration in nanoseconds.
    pub renderer_max_ns: u64,
    /// Advisory p99 threshold supplied by the caller.
    pub advisory_p99_limit_ns: u64,
    /// Whether the host observation was within the advisory threshold.
    pub advisory_threshold_met: bool,
    /// Explicit truth source; always `host_api_observation`.
    pub truth_source: String,
}

/// Bounded partial memory-capacity accounting evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryMetrics {
    /// Accounted primary payload bytes.
    ///
    /// This is not process working set, allocator overhead, renderer state, or peak RSS.
    pub bounded_working_set_bytes: u64,
    /// Output audio allocation in bytes.
    pub output_audio_bytes: u64,
    /// Trajectory storage allocation in bytes.
    pub trajectory_bytes: u64,
    /// Timing sample storage allocation in bytes.
    pub timing_bytes: u64,
    /// Peak process-memory observation when supplied by an external profiler.
    pub peak_process_memory_bytes: Option<u64>,
    /// Coverage classification; currently `partial_primary_payloads`.
    pub coverage: String,
    /// Truth source; this is capacity accounting, not process RSS.
    pub truth_source: String,
}

/// Optional allocation observation from a caller-provided allocation audit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AllocationObservation {
    /// Observation state.
    pub status: EvidenceStatus,
    /// Observed allocation count, if an audit was supplied.
    pub steady_state_allocations: Option<u64>,
    /// Truth source for the observation.
    pub truth_source: Option<String>,
}

/// Reported, configured latency evidence that is not a physical measurement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LatencyEvidence {
    /// Renderer-reported latency in frames.
    pub renderer_latency_frames: usize,
    /// Maximum captured delay rounded up to frames.
    pub configured_delay_latency_frames: usize,
    /// Explicit evidence classification.
    pub classification: String,
}

/// Validation hook category for future subsystem evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookCategory {
    /// Renderer validation supplied by another harness component.
    Renderer,
    /// Realtime-safety validation supplied by an allocation or callback test.
    RealtimeSafety,
    /// Transport validation supplied by accepted transport tests.
    Transport,
    /// Schema/protocol compatibility validation.
    Compatibility,
}

/// Bounded external validation-hook result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HookEvidence {
    /// Stable hook identifier.
    pub id: String,
    /// Evidence category.
    pub category: HookCategory,
    /// Hook result.
    pub status: EvidenceStatus,
    /// Whether this observation is required for aggregate PASS.
    pub required: bool,
    /// Truth source for the supplied result.
    pub truth_source: String,
}

/// One explicit validation criterion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValidationFinding {
    /// Stable finding identifier.
    pub id: String,
    /// Finding state.
    pub status: EvidenceStatus,
    /// Whether this finding is required for aggregate PASS.
    pub required: bool,
    /// Observed numeric value where applicable.
    pub observed: Option<f64>,
    /// Configured limit where applicable.
    pub limit: Option<f64>,
}

/// Aggregate validation result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValidationSummary {
    /// Overall required-evidence status.
    ///
    /// `fail` means a required finding failed. `not_observed` means no required
    /// finding failed but at least one required finding was not observed.
    pub status: EvidenceStatus,
    /// Canonically ordered findings.
    pub findings: Vec<ValidationFinding>,
}

/// Complete versioned machine-readable renderer evaluation report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluationReport {
    /// Evaluation schema version.
    pub schema_version: u16,
    /// Renderer metadata.
    pub renderer: RendererMetadata,
    /// Reproducibility and truth-source metadata.
    pub provenance: Provenance,
    /// Deterministic audio facts.
    pub audio: AudioMetrics,
    /// Reported/configured latency facts.
    pub latency: LatencyEvidence,
    /// Host-observed processing cost. Excluded from deterministic summaries.
    pub performance: PerformanceMetrics,
    /// Partial deterministic capacity accounting.
    pub memory: MemoryMetrics,
    /// Optional steady-state allocation observation.
    pub allocations: AllocationObservation,
    /// Deterministic change metrics.
    pub discontinuity: DiscontinuityMetrics,
    /// Fixed-direction probe results.
    pub probes: Vec<ProbeResult>,
    /// Block/channel trajectories.
    pub trajectory: Vec<GainTrajectoryPoint>,
    /// Validation-hook evidence.
    pub hooks: Vec<HookEvidence>,
    /// Required deterministic validation summary.
    pub validation: ValidationSummary,
}

impl EvaluationReport {
    /// Serializes the complete report as stable pretty JSON for one run.
    ///
    /// The complete report contains host timing and is therefore not byte-stable
    /// across hosts or repeated runs.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Returns a deterministic compact summary suitable for regression comparison.
    pub fn summary(&self) -> EvaluationSummary {
        EvaluationSummary {
            schema_version: self.schema_version,
            renderer_id: self.renderer.renderer_id.clone(),
            configuration_hash_fnv1a64: self.renderer.configuration_hash_fnv1a64.clone(),
            audio_checksum_fnv1a64: self.audio.audio_checksum_fnv1a64.clone(),
            validation_status: self.validation.status,
            accounted_primary_payload_bytes: self.memory.bounded_working_set_bytes,
        }
    }
}

/// Compact deterministic machine-readable evaluation summary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvaluationSummary {
    /// Evaluation schema version.
    pub schema_version: u16,
    /// Stable renderer identifier.
    pub renderer_id: String,
    /// Configuration regression fingerprint.
    pub configuration_hash_fnv1a64: String,
    /// Rendered-audio regression fingerprint.
    pub audio_checksum_fnv1a64: String,
    /// Overall required deterministic validation status.
    pub validation_status: EvidenceStatus,
    /// Deterministically accounted partial primary payload.
    pub accounted_primary_payload_bytes: u64,
}

/// Complete report plus planar rendered audio for WAV artifact generation.
#[derive(Clone, Debug, PartialEq)]
pub struct EvaluationBundle {
    /// Machine-readable report.
    pub report: EvaluationReport,
    /// Planar rendered output.
    pub audio_channels: Vec<Vec<f32>>,
}
