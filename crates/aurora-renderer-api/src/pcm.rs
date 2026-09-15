//! Additive block-PCM renderer boundary for renderers that consume object audio.
//!
//! Aurora's existing [`crate::Renderer`] contract intentionally computes
//! speaker gains only. Some independent renderers, including libspatialaudio,
//! own interpolation/decorrelation state and therefore consume object PCM and
//! emit speaker PCM directly. This module models that distinct capability
//! without weakening or overloading the gain-renderer contract.

use aurora_core::{Listener, Speaker};
use thiserror::Error;

use crate::RenderObject;

/// Borrowed PCM and spatial state for one object in a processing block.
#[derive(Debug, Clone, Copy)]
pub struct ObjectPcmBlock<'a> {
    /// Spatial state valid for this block.
    pub object: RenderObject,
    /// Mono object PCM. Length must equal the configured block size.
    pub samples: &'a [f32],
}

/// Errors returned by direct PCM renderer implementations.
#[derive(Debug, Error, PartialEq)]
pub enum PcmRendererError {
    /// The renderer has not received a valid configuration.
    #[error("PCM renderer has not been configured")]
    NotConfigured,
    /// A setup-time renderer configuration is invalid or unsupported.
    #[error("invalid PCM renderer configuration: {0}")]
    InvalidConfiguration(String),
    /// Runtime listener orientation is finite but unsupported by the backend.
    /// This fixed variant exists so callback-side rejection does not format or
    /// allocate an error string.
    #[error("PCM renderer listener orientation is unsupported")]
    UnsupportedListenerOrientation,
    /// More objects were supplied than the configured steady-state maximum.
    #[error("PCM renderer supports at most {maximum} objects, got {actual}")]
    TooManyObjects {
        /// Configured object limit.
        maximum: usize,
        /// Number of objects supplied.
        actual: usize,
    },
    /// One object PCM buffer does not match the configured block size.
    #[error("object {object_index} PCM needs {required} frames, got {actual}")]
    InputBlockSize {
        /// Object index in the current call.
        object_index: usize,
        /// Configured frame count.
        required: usize,
        /// Supplied frame count.
        actual: usize,
    },
    /// Output contains the wrong number of planar speaker channels.
    #[error("PCM renderer output needs {required} channels, got {actual}")]
    OutputChannelCount {
        /// Configured speaker count.
        required: usize,
        /// Supplied output channel count.
        actual: usize,
    },
    /// One output channel does not match the configured block size.
    #[error("output channel {channel} needs {required} frames, got {actual}")]
    OutputBlockSize {
        /// Output channel index.
        channel: usize,
        /// Configured frame count.
        required: usize,
        /// Supplied frame count.
        actual: usize,
    },
    /// Runtime object/listener state contains NaN or infinity.
    #[error("PCM renderer runtime metadata must be finite")]
    NonFiniteMetadata,
    /// External renderer/backend returned a worker-side failure.
    #[error("PCM renderer backend failed")]
    BackendFault,
}

/// Replaceable block renderer for object PCM -> planar speaker PCM.
///
/// This contract is additive to [`crate::Renderer`]. Implementations allocate
/// and validate all fixed storage during [`configure`](Self::configure).
/// [`render_pcm`](Self::render_pcm) must not allocate, resize containers,
/// format strings, acquire blocking locks, perform filesystem/network I/O, or
/// spawn processes/threads.
pub trait ObjectPcmRenderer: Send {
    /// Configures layout and fixed processing limits before rendering starts.
    fn configure(
        &mut self,
        layout: Vec<Speaker>,
        sample_rate: u32,
        block_size: usize,
        max_objects: usize,
    ) -> Result<(), PcmRendererError>;

    /// Renders one block into caller-owned planar speaker buffers.
    fn render_pcm(
        &mut self,
        listener: &Listener,
        objects: &[ObjectPcmBlock<'_>],
        output: &mut [&mut [f32]],
    ) -> Result<(), PcmRendererError>;

    /// Clears renderer history without changing configured capacities.
    fn reset(&mut self);

    /// Returns renderer algorithmic latency in frames.
    fn latency_frames(&self) -> usize;

    /// Returns the configured output channel count.
    fn output_channel_count(&self) -> usize;
}
