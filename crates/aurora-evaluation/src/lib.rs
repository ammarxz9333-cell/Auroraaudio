//! Control-thread evaluation for Aurora renderers.
//!
//! This crate consumes the accepted [`aurora_renderer_api::Renderer`] boundary
//! without changing renderer or DSP behavior. It owns bounded evidence models,
//! trajectory capture, deterministic validation, partial capacity accounting,
//! and separately classified host-observed renderer-call timing. Filesystem
//! access and artifact placement remain CLI concerns.
//!
//! Evaluation is not callback code. Host timing is advisory, excluded from the
//! deterministic summary and required aggregate, and never represents physical
//! latency. FNV-1a values are regression fingerprints, not cryptographic
//! integrity digests.

mod error;
mod model;
mod runner;

pub use error::EvaluationError;
pub use model::{
    AllocationObservation, AudioMetrics, DiscontinuityMetrics, EvaluationBundle, EvaluationConfig,
    EvaluationFixture, EvaluationReport, EvaluationSummary, EvaluationThresholds, EvidenceStatus,
    GainTrajectoryPoint, HookCategory, HookEvidence, LatencyEvidence, MemoryMetrics,
    PerformanceMetrics, ProbeDefinition, ProbeResult, Provenance, RendererMetadata,
    ValidationFinding, ValidationSummary, EVALUATION_SCHEMA_VERSION, MAX_BLOCKS, MAX_CHANNELS,
    MAX_FRAMES, MAX_HOOKS, MAX_PROBES, MAX_STRING_BYTES,
};
pub use runner::{evaluate_renderer, fnv1a64_hex};
