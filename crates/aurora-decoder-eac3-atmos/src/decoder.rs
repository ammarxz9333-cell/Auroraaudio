//! E-AC-3/JOC decoder adapter boundary.
//!
//! Aurora does not ship a proprietary E-AC-3 or JOC decoder. This adapter
//! rejects compressed input until a separately reviewed decoder is connected.

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};

#[derive(Debug, Default)]
pub struct Eac3AtmosDecoder { configured: bool }

impl Eac3AtmosDecoder {
    pub fn new() -> Self { Self::default() }
    pub fn signal_discontinuity(&mut self) {}
}

impl Decoder for Eac3AtmosDecoder {
    fn info(&self) -> DecoderInfo {
        DecoderInfo { name: "aurora-eac3-joc-adapter", production_ready: false, maturity: "unavailable-external-decoder-required" }
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
        if !self.configured { return Err(DecoderError::Unavailable("E-AC-3 adapter is not configured")); }
        Err(DecoderError::Unavailable("Aurora has no bundled E-AC-3/JOC decoder; connect a reviewed external decoder"))
    }
    fn reset(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::{AudioFormat, SampleType};
    #[test]
    fn never_synthesizes_pcm_for_compressed_input() {
        let mut decoder = Eac3AtmosDecoder::new();
        decoder.configure(AudioFormat { sample_rate: 48_000, channel_count: 6, sample_type: SampleType::F32, block_size: 1536 }).unwrap();
        assert!(matches!(decoder.decode_chunk(&[0x0b, 0x77, 0, 0, 0, 0]), Err(DecoderError::Unavailable(_))));
        assert!(!decoder.info().production_ready);
    }
}
