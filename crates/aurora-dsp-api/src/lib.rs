//! DSP processing boundary for future Phase 0 milestones.

use aurora_core::AudioBlock;
use thiserror::Error;

/// Errors returned by DSP engines.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DspError {
    /// The DSP engine has not received a valid configuration.
    #[error("DSP engine is not configured")]
    NotConfigured,
}

/// DSP processing abstraction used after rendering.
pub trait DspEngine {
    /// Configures the DSP engine for channel count, sample rate, and block size.
    fn configure(
        &mut self,
        channel_count: usize,
        sample_rate: u32,
        block_size: usize,
    ) -> Result<(), DspError>;

    /// Processes one mutable audio block in place.
    fn process(&mut self, audio_block: &mut AudioBlock) -> Result<(), DspError>;

    /// Clears internal processing state.
    fn reset(&mut self);

    /// Returns DSP latency in frames.
    fn latency_frames(&self) -> usize;
}
