//! Aurora TrueHD adapter.
//!
//! The default workspace build keeps the native backend disabled so Aurora can
//! retain its Rust 1.85 baseline. Enabling `native-truehd` uses the pinned
//! upstream `truehd` library with its Rust >=1.88 toolchain and exposes both
//! ordinary PCM decoding and an object-preserving Spatial IR path.

#[cfg(feature = "native-truehd")]
mod native;

#[cfg(feature = "native-truehd")]
pub use native::TruehddDecoderAdapter;

#[cfg(not(feature = "native-truehd"))]
mod disabled {
    use aurora_core::AudioFormat;
    use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
    use aurora_spatial_ir::SpatialDecodedFrame;

    /// Feature-disabled adapter retaining the stable Aurora API surface.
    #[derive(Debug, Default, Clone)]
    pub struct TruehddDecoderAdapter {
        configured_format: Option<AudioFormat>,
    }

    impl TruehddDecoderAdapter {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn decode_spatial_chunk(
            &mut self,
            _input: &[u8],
        ) -> Result<Option<SpatialDecodedFrame>, DecoderError> {
            Err(DecoderError::Unavailable(
                "native TrueHD backend requires the native-truehd feature and Rust >=1.88",
            ))
        }

        pub fn recovered_error_count(&self) -> u64 {
            0
        }
    }

    impl Decoder for TruehddDecoderAdapter {
        fn info(&self) -> DecoderInfo {
            DecoderInfo {
                name: "Aurora native TrueHD adapter (disabled)",
                production_ready: false,
                maturity: "feature-disabled",
            }
        }

        fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
            self.configured_format = Some(output_format);
            Ok(())
        }

        fn decode_chunk(&mut self, _input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
            Err(DecoderError::Unavailable(
                "native TrueHD backend requires the native-truehd feature and Rust >=1.88",
            ))
        }

        fn reset(&mut self) {
            let configured = self.configured_format;
            *self = Self::default();
            self.configured_format = configured;
        }
    }
}

#[cfg(not(feature = "native-truehd"))]
pub use disabled::TruehddDecoderAdapter;

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_decoder_api::Decoder;

    #[cfg(not(feature = "native-truehd"))]
    #[test]
    fn disabled_adapter_reports_feature_boundary_truthfully() {
        let adapter = TruehddDecoderAdapter::new();
        let info = adapter.info();
        assert_eq!(info.maturity, "feature-disabled");
        assert!(!info.production_ready);
    }
}
