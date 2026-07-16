//! Cavern renderer adapter boundary.
//!
//! Cavern is disabled by default pending license review. This crate contains no
//! Cavern source and only implements the Aurora-owned renderer trait boundary.

use aurora_core::{Listener, Speaker};
use aurora_renderer_api::{
    RenderObject, Renderer, RendererError, RendererScratch, RendererScratchSize, SpeakerGain,
};

/// Cavern renderer adapter placeholder.
#[derive(Debug, Default, Clone)]
pub struct CavernRendererAdapter {
    configured: bool,
}

impl CavernRendererAdapter {
    /// Creates a disabled-by-default Cavern adapter boundary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns why this adapter is disabled by default.
    pub fn disabled_reason(&self) -> &'static str {
        "disabled by default pending Cavern license review"
    }
}

impl Renderer for CavernRendererAdapter {
    fn configure(
        &mut self,
        _layout: Vec<Speaker>,
        _sample_rate: u32,
        _block_size: usize,
        _max_objects: usize,
    ) -> Result<(), RendererError> {
        self.configured = true;
        Ok(())
    }

    fn required_scratch_size(&self) -> Result<RendererScratchSize, RendererError> {
        if !self.configured {
            return Err(RendererError::NotConfigured);
        }
        Ok(RendererScratchSize { float_count: 0 })
    }

    fn render_gains(
        &mut self,
        _listener: &Listener,
        _objects: &[RenderObject],
        _output_gains: &mut [SpeakerGain],
        _scratch: &mut RendererScratch,
    ) -> Result<(), RendererError> {
        if !self.configured {
            return Err(RendererError::NotConfigured);
        }
        Err(RendererError::Unavailable(
            "Cavern adapter is disabled pending license review",
        ))
    }

    fn reset(&mut self) {}

    fn latency_frames(&self) -> usize {
        0
    }

    fn output_channel_count(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cavern_adapter_is_disabled_by_default() {
        let adapter = CavernRendererAdapter::new();

        assert!(adapter.disabled_reason().contains("license review"));
    }
}
