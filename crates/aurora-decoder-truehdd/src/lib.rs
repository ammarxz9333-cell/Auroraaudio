//! Experimental TrueHD adapter boundary.
//!
//! No TrueHD/MLP decoder is bundled. Compressed input is rejected rather than
//! replaced with synthetic PCM or fabricated object metadata.

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};

#[derive(Debug, Default, Clone)]
pub struct TruehddDecoderAdapter { configured: bool }

impl TruehddDecoderAdapter {
    pub fn new() -> Self { Self::default() }
}

impl Decoder for TruehddDecoderAdapter {
    fn info(&self) -> DecoderInfo {
        DecoderInfo { name: "aurora-truehd-adapter", production_ready: false, maturity: "unavailable-external-decoder-required" }
    }
    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        if output_format.sample_rate == 0 || output_format.channel_count == 0 {
            return Err(DecoderError::UnsupportedInput("invalid output format"));
        }
        self.configured = true;
        Ok(())
    }
    fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        if input.is_empty() { return Ok(None); }
        if !self.configured { return Err(DecoderError::Unavailable("TrueHD adapter is not configured")); }
        Err(DecoderError::Unavailable("Aurora has no bundled TrueHD/MLP decoder"))
    }
    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_unavailable_instead_of_fabricating_audio() {
        let decoder = TruehddDecoderAdapter::new();
        assert!(!decoder.info().production_ready);
        assert_eq!(decoder.info().maturity, "unavailable-external-decoder-required");
    }
}
