//! Experimental truehdd decoder adapter boundary.
//!
//! This crate does not vendor or link truehdd. Any future integration must be
//! offline-only, out-of-process, and reviewed for licensing and commercial risk.

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};

/// Experimental offline-only truehdd adapter placeholder.
#[derive(Debug, Default, Clone)]
pub struct TruehddDecoderAdapter {
    configured_format: Option<AudioFormat>,
}

impl TruehddDecoderAdapter {
    /// Creates a truehdd adapter boundary.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Decoder for TruehddDecoderAdapter {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "truehdd out-of-process adapter",
            production_ready: false,
            maturity: "experimental-offline-only",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.configured_format = Some(output_format);
        Ok(())
    }

    fn decode_chunk(&mut self, _input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        Err(DecoderError::Unavailable(
            "truehdd integration is experimental, offline-only, and disabled",
        ))
    }

    fn reset(&mut self) {
        self.configured_format = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_decoder_api::Decoder;

    #[test]
    fn truehdd_adapter_is_experimental_offline_only() {
        let adapter = TruehddDecoderAdapter::new();
        let info = adapter.info();

        assert_eq!(info.maturity, "experimental-offline-only");
        assert!(!info.production_ready);
    }
}
