//! Stable, allocation-free renderer boundary for Aurora implementations.

pub mod head_pose;
pub mod head_pose_transform;
pub mod pcm;
pub use head_pose::*;
pub use head_pose_transform::*;
pub use pcm::*;

use aurora_core::{Listener, Speaker, Vector3};
use thiserror::Error;

/// Compact object state consumed by a renderer during one processing block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderObject {
    /// Object position in meters.
    pub position: Vector3,
    /// Linear amplitude applied after spatial gain calculation.
    pub gain: f32,
}

/// Per-speaker render result for one object at one point in time.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpeakerGain {
    /// Configured speaker index this result applies to.
    pub speaker_index: usize,
    /// Linear amplitude gain.
    pub gain: f32,
    /// Distance from source object to speaker in meters.
    pub distance_meters: f32,
    /// Propagation plus speaker delay in samples.
    pub delay_samples: f32,
}

/// Scratch storage requirements reported by a configured renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererScratchSize {
    /// Number of temporary floating-point values required.
    pub float_count: usize,
}

/// Caller-owned renderer scratch storage allocated before processing starts.
#[derive(Debug, Clone, PartialEq)]
pub struct RendererScratch {
    floats: Vec<f32>,
}

impl RendererScratch {
    /// Allocates scratch storage matching a renderer's reported requirement.
    pub fn new(size: RendererScratchSize) -> Self {
        Self {
            floats: vec![0.0; size.float_count],
        }
    }

    /// Returns mutable floating-point scratch storage.
    pub fn floats_mut(&mut self) -> &mut [f32] {
        &mut self.floats
    }

    /// Returns the fixed floating-point capacity for allocation tests.
    pub fn float_capacity(&self) -> usize {
        self.floats.capacity()
    }
}

/// Errors returned by renderer implementations.
#[derive(Debug, Error, PartialEq)]
pub enum RendererError {
    /// The renderer has not received a valid configuration.
    #[error("renderer has not been configured")]
    NotConfigured,
    /// The configured layout has no enabled speakers.
    #[error("layout must contain at least one enabled speaker")]
    NoEnabledSpeakers,
    /// More objects were supplied than the configured steady-state maximum.
    #[error("renderer supports at most {maximum} objects, got {actual}")]
    TooManyObjects {
        /// Configured object limit.
        maximum: usize,
        /// Number of objects supplied.
        actual: usize,
    },
    /// Caller-owned output storage has the wrong length.
    #[error("renderer output needs {required} gains, got {actual}")]
    OutputBufferSize {
        /// Required flattened gain count.
        required: usize,
        /// Supplied gain count.
        actual: usize,
    },
    /// Caller-owned scratch storage is too small.
    #[error("renderer scratch needs {required} floats, got {actual}")]
    ScratchBufferSize {
        /// Required floating-point scratch count.
        required: usize,
        /// Supplied floating-point scratch count.
        actual: usize,
    },
    /// A setup-time renderer configuration is invalid.
    #[error("invalid renderer configuration: {0}")]
    InvalidConfiguration(String),
    /// An optional renderer is unavailable by policy or build configuration.
    #[error("renderer unavailable: {0}")]
    Unavailable(&'static str),
}

/// Setup-time capabilities of a configured renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RendererCapabilities {
    dynamic_delay_values: bool,
}

impl RendererCapabilities {
    /// Creates renderer capabilities.
    pub const fn new(dynamic_delay_values: bool) -> Self {
        Self {
            dynamic_delay_values,
        }
    }

    /// Returns whether per-speaker delay values emitted by `render_gains` must be applied dynamically.
    pub const fn dynamic_delay_values(self) -> bool {
        self.dynamic_delay_values
    }
}

/// Replaceable renderer abstraction used by offline and real-time pipelines.
pub trait Renderer: Send {
    /// Reports configured renderer capabilities used by realtime materialization.
    fn capabilities(&self) -> RendererCapabilities {
        RendererCapabilities::default()
    }

    /// Configures layout and fixed processing limits before rendering starts.
    fn configure(
        &mut self,
        layout: Vec<Speaker>,
        sample_rate: u32,
        block_size: usize,
        max_objects: usize,
    ) -> Result<(), RendererError>;

    /// Reports scratch storage needed by the current configuration.
    fn required_scratch_size(&self) -> Result<RendererScratchSize, RendererError>;

    /// Writes object-major, speaker-minor gains into caller-owned storage.
    ///
    /// Implementations must not allocate, resize containers, format strings, or
    /// acquire blocking locks during this call.
    fn render_gains(
        &mut self,
        listener: &Listener,
        objects: &[RenderObject],
        output_gains: &mut [SpeakerGain],
        scratch: &mut RendererScratch,
    ) -> Result<(), RendererError>;

    /// Clears renderer history without changing configured capacities.
    fn reset(&mut self);

    /// Returns renderer latency in frames.
    fn latency_frames(&self) -> usize;

    /// Returns the configured enabled speaker count.
    fn output_channel_count(&self) -> usize;
}
