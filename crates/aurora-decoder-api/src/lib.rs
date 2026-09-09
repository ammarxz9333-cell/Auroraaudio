//! Aurora-owned decoder boundary for external immersive-audio decoders.

use aurora_core::{AudioBlock, AudioFormat, AudioObject};
use thiserror::Error;

/// Decoded audio and object metadata for one offline chunk.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedFrame {
    /// Decoded PCM samples.
    pub audio: AudioBlock,
    /// Object metadata associated with the decoded block.
    pub objects: Vec<AudioObject>,
}

/// Decoder metadata reported by an adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoderInfo {
    /// Human-readable adapter name.
    pub name: &'static str,
    /// Whether this adapter is intended for production use.
    pub production_ready: bool,
    /// Short maturity label such as `experimental` or `preferred-open`.
    pub maturity: &'static str,
}

/// Errors returned by decoder adapters.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecoderError {
    /// Adapter feature is disabled or unavailable.
    #[error("decoder adapter is unavailable: {0}")]
    Unavailable(&'static str),
    /// Input format is unsupported by this adapter.
    #[error("unsupported decoder input: {0}")]
    UnsupportedInput(&'static str),
    /// A native/in-process codec backend rejected or failed to decode input.
    #[error("native decoder failed: {0}")]
    Decode(String),
    /// External decoder process failed.
    #[error("external decoder process failed: {0}")]
    ExternalProcess(String),
}

/// Aurora-owned decoder trait implemented by third-party adapter crates.
pub trait Decoder {
    /// Returns static adapter metadata.
    fn info(&self) -> DecoderInfo;

    /// Configures decoder output format.
    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError>;

    /// Decodes one offline input chunk.
    fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError>;

    /// Clears decoder state.
    fn reset(&mut self);
}
