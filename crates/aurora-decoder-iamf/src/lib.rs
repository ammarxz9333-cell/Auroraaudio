//! Aurora IAMF decoder adapter.
//!
//! The preferred runtime is the pinned pure-Rust `iamf-rs` implementation.
//! It stays isolated behind Aurora's decoder boundary so the reference
//! `libiamf` and `iamf-tools` implementations can remain independent
//! conformance oracles rather than runtime dependencies.

#[cfg(feature = "iamf-rs-native")]
mod native;

#[cfg(feature = "iamf-rs-native")]
pub use native::IamfDecoderAdapter;

#[cfg(not(feature = "iamf-rs-native"))]
mod disabled {
    use aurora_core::AudioFormat;
    use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};

    #[derive(Debug, Default, Clone)]
    pub struct IamfDecoderAdapter {
        configured_format: Option<AudioFormat>,
    }

    impl IamfDecoderAdapter {
        pub fn new() -> Self {
            Self::default()
        }
    }

    impl Decoder for IamfDecoderAdapter {
        fn info(&self) -> DecoderInfo {
            DecoderInfo {
                name: "Aurora iamf-rs adapter (disabled)",
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
                "native IAMF backend requires the iamf-rs-native feature",
            ))
        }

        fn reset(&mut self) {
            let configured = self.configured_format;
            *self = Self::default();
            self.configured_format = configured;
        }
    }
}

#[cfg(not(feature = "iamf-rs-native"))]
pub use disabled::IamfDecoderAdapter;

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_decoder_api::Decoder;

    #[cfg(not(feature = "iamf-rs-native"))]
    #[test]
    fn disabled_adapter_reports_feature_boundary_truthfully() {
        let adapter = IamfDecoderAdapter::new();
        assert_eq!(adapter.info().maturity, "feature-disabled");
    }
}
