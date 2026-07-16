//! IAMF decoder adapter boundary.
//!
//! This crate does not contain libiamf source. The intended integration path is
//! an out-of-process libiamf-compatible decoder invoked by an Aurora adapter.

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};

/// Preferred open immersive-audio decoder adapter placeholder.
#[derive(Debug, Default, Clone)]
pub struct IamfDecoderAdapter {
    configured_format: Option<AudioFormat>,
}

impl IamfDecoderAdapter {
    /// Creates an IAMF adapter boundary.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Decoder for IamfDecoderAdapter {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "libiamf out-of-process adapter",
            production_ready: false,
            maturity: "preferred-open-planned",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.configured_format = Some(output_format);
        Ok(())
    }

    fn decode_chunk(&mut self, _input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        Err(DecoderError::Unavailable(
            "libiamf process integration is not enabled in Milestone 0D",
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
    fn iamf_adapter_reports_preferred_open_status() {
        let adapter = IamfDecoderAdapter::new();
        let info = adapter.info();

        assert_eq!(info.maturity, "preferred-open-planned");
        assert!(!info.production_ready);
    }
}
