//! Aurora TrueHD adapter.
//!
//! The default workspace build keeps the native backend disabled so Aurora can
//! retain its Rust 1.85 baseline. Enabling `native-truehd` uses the pinned
//! upstream `truehd` library with its Rust >=1.88 toolchain and exposes both
//! ordinary PCM decoding and object-preserving Spatial IR paths.

#[cfg(feature = "native-truehd")]
mod native;
#[cfg(feature = "native-truehd")]
mod native_v2;

#[cfg(feature = "native-truehd")]
pub use native::TruehddDecoderAdapter;
#[cfg(feature = "native-truehd")]
pub use native_v2::TruehddV2DecoderAdapter;

#[cfg(not(feature = "native-truehd"))]
mod disabled {
    use aurora_core::AudioFormat;
    use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
    use aurora_spatial_ir::SpatialDecodedFrame;
    use aurora_spatial_ir_v2::SpatialDecodedFrame as SpatialDecodedFrameV2;

    /// Feature-disabled adapter retaining the stable Aurora V1 API surface.
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

    /// Feature-disabled V2 adapter. Kept separate so callers cannot accidentally
    /// downgrade rich metadata into V1 when the native feature is unavailable.
    #[derive(Debug, Default, Clone)]
    pub struct TruehddV2DecoderAdapter {
        configured_format: Option<AudioFormat>,
    }

    impl TruehddV2DecoderAdapter {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
            self.configured_format = Some(output_format);
            Ok(())
        }

        pub fn decode_spatial_chunk_v2(
            &mut self,
            _input: &[u8],
        ) -> Result<Option<SpatialDecodedFrameV2>, DecoderError> {
            Err(DecoderError::Unavailable(
                "TrueHD Spatial IR V2 requires the native-truehd feature and Rust >=1.88",
            ))
        }

        pub fn recovered_error_count(&self) -> u64 {
            0
        }

        pub fn reset(&mut self) {
            let configured = self.configured_format;
            *self = Self::default();
            self.configured_format = configured;
        }
    }
}

#[cfg(not(feature = "native-truehd"))]
pub use disabled::{TruehddDecoderAdapter, TruehddV2DecoderAdapter};

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

    #[cfg(not(feature = "native-truehd"))]
    #[test]
    fn disabled_v2_adapter_refuses_to_fabricate_rich_metadata() {
        let mut adapter = TruehddV2DecoderAdapter::new();
        assert!(matches!(
            adapter.decode_spatial_chunk_v2(&[]),
            Err(DecoderError::Unavailable(_))
        ));
    }
}
