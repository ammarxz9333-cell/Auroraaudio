use aurora_dsp_basic::BasicDspError;
use aurora_renderer_api::RendererError;
use thiserror::Error;

/// Structured failures returned by the evaluation framework.
#[derive(Debug, Error)]
pub enum EvaluationError {
    /// Evaluation configuration is outside published bounds.
    #[error("invalid evaluation configuration: {0}")]
    InvalidConfiguration(&'static str),
    /// A bounded collection exceeded its published maximum.
    #[error("evaluation {field} count {actual} exceeds maximum {maximum}")]
    LimitExceeded {
        /// Bounded field name.
        field: &'static str,
        /// Observed count.
        actual: usize,
        /// Published maximum.
        maximum: usize,
    },
    /// Renderer execution failed.
    #[error("renderer evaluation failed: {0}")]
    Renderer(#[from] RendererError),
    /// Delay processing failed.
    #[error("evaluation delay processing failed: {0}")]
    Dsp(#[from] BasicDspError),
    /// Deterministic JSON serialization failed.
    #[error("evaluation JSON serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    /// Checked capacity arithmetic overflowed.
    #[error("evaluation capacity arithmetic overflowed")]
    CapacityOverflow,
}
