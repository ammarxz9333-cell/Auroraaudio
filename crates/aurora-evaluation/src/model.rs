use aurora_core::Vector3;
use serde::{Deserialize, Serialize};

/// Current machine-readable evaluation schema.
pub const EVALUATION_SCHEMA_VERSION: u16 = 1;
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

/// Pass/fail state used throughout machine-readable evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    /// Criterion passed.
    Pass,
    /// Criterion failed.
    Fail,
    /// Criterion was not observed by this run.
    NotObserved,
}

/// Configurable thresholds that turn regressions into failed evaluations.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluationThresholds {
    /// Maximum absolute error from unit gain power.
    pub max_gain_power_error: f32,
    /// Maximum gain change for one channel between adjacent blocks.
    pub max_gain_discontinuity: f32,
    /// Maximum delay change in samples between adjacent blocks.
    pub max_delay_discontinuity_samples: f32,
    /// Maximum adjacent rendered-sample delta.
    pub max_audio_discontinuity: f32,
    /// Maximum allowed p99 renderer processing cost in nanoseconds.
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
    /// Validation thresholds.
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
    /// Deterministic FNV-1a hash of canonical evaluation configuration JSON.
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

/// Maximum observed discontinuities from deterministic generated output.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiscontinuityMetrics {
    /// Largest adjacent-block gain change.
    pub maximum_gain_delta: f32,
    /// Largest adjacent-block delay change in samples.
    pub maximum_delay_delta_samples: f32,
    /// Largest adjacent rendered-sample change.
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
    /// Deterministic FNV-1a checksum over planar `f32` bytes.
    pub audio_checksum_fnv1a64: String,
}

/// Host-observed processing-cost percentiles.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Number of timed renderer calls.
    pub samples: usize,
    /// p50 renderer-call duration in nanoseconds.
    pub renderer_p50_ns: u64,
    /// p95 renderer-call duration in nanoseconds.
    pub renderer_p95_ns: u64,
    /// p99 renderer-call duration in nanoseconds.
    pub renderer_p99_ns: u64,
    /// Maximum renderer-call duration in nanoseconds.
    pub renderer_max_ns: u64,
    /// Explicit truth source; always `host_api_observation`.
    pub truth_source: String,
}

/// Bounded memory-accounting evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryMetrics {
    /// Deterministically accounted primary retained and processing payloads.
    pub bounded_working_set_bytes: u64,
    /// Output audio allocation in bytes.
    pub output_audio_bytes: u64,
    /// Trajectory storage allocation in bytes.
    pub trajectory_bytes: u64,
    /// Timing sample storage allocation in bytes.
    pub timing_bytes: u64,
    /// Peak process-memory observation when supplied by an external profiler.
    pub peak_process_memory_bytes: Option<u64>,
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
    /// Truth source for the supplied result.
    pub truth_source: String,
}

/// One explicit validation criterion.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValidationFinding {
    /// Stable finding identifier.
    pub id: String,
    /// Pass/fail state.
    pub status: EvidenceStatus,
    /// Observed numeric value where applicable.
    pub observed: Option<f64>,
    /// Configured limit where applicable.
    pub limit: Option<f64>,
}

/// Aggregate validation result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValidationSummary {
    /// Overall status; any failed finding makes this fail.
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
    /// Host-observed processing cost.
    pub performance: PerformanceMetrics,
    /// Deterministic capacity accounting.
    pub memory: MemoryMetrics,
    /// Optional steady-state allocation observation.
    pub allocations: AllocationObservation,
    /// Discontinuity metrics.
    pub discontinuity: DiscontinuityMetrics,
    /// Fixed-direction probe results.
    pub probes: Vec<ProbeResult>,
    /// Block/channel trajectories.
    pub trajectory: Vec<GainTrajectoryPoint>,
    /// Validation-hook evidence.
    pub hooks: Vec<HookEvidence>,
    /// Threshold validation summary.
    pub validation: ValidationSummary,
}

impl EvaluationReport {
    /// Serializes the complete report as stable pretty JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Returns a compact summary suitable for CI dashboards.
    pub fn summary(&self) -> EvaluationSummary {
        EvaluationSummary {
            schema_version: self.schema_version,
            renderer_id: self.renderer.renderer_id.clone(),
            configuration_hash_fnv1a64: self.renderer.configuration_hash_fnv1a64.clone(),
            audio_checksum_fnv1a64: self.audio.audio_checksum_fnv1a64.clone(),
            validation_status: self.validation.status,
            renderer_p50_ns: self.performance.renderer_p50_ns,
            renderer_p95_ns: self.performance.renderer_p95_ns,
            renderer_p99_ns: self.performance.renderer_p99_ns,
            bounded_working_set_bytes: self.memory.bounded_working_set_bytes,
        }
    }
}

/// Compact machine-readable evaluation summary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvaluationSummary {
    /// Evaluation schema version.
    pub schema_version: u16,
    /// Stable renderer identifier.
    pub renderer_id: String,
    /// Configuration hash.
    pub configuration_hash_fnv1a64: String,
    /// Rendered-audio checksum.
    pub audio_checksum_fnv1a64: String,
    /// Overall validation status.
    pub validation_status: EvidenceStatus,
    /// Host-observed p50 cost.
    pub renderer_p50_ns: u64,
    /// Host-observed p95 cost.
    pub renderer_p95_ns: u64,
    /// Host-observed p99 cost.
    pub renderer_p99_ns: u64,
    /// Deterministically accounted primary working-set payload.
    pub bounded_working_set_bytes: u64,
}

/// Complete report plus planar rendered audio for WAV artifact generation.
#[derive(Clone, Debug, PartialEq)]
pub struct EvaluationBundle {
    /// Machine-readable report.
    pub report: EvaluationReport,
    /// Planar rendered output.
    pub audio_channels: Vec<Vec<f32>>,
}
