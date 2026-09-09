//! Aurora's top-level best-of-breed decoder front door.
//!
//! `aurora-decoder-engine` remains the stable core for AC-3/E-AC-3/JOC,
//! AC-4, DTS and broad compatibility fallback. This crate composes that core
//! with format-specialist immersive backends that are stronger when selected:
//! native IAMF (`iamf-rs`) and optional native TrueHD/Atmos (`truehd`).

#![forbid(unsafe_code)]

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use aurora_decoder_engine::catalog::CodecId;
use aurora_decoder_engine::{AuroraDecoderEngine, EngineConfig};
use aurora_spatial_ir::SpatialDecodedFrame;

#[cfg(feature = "iamf")]
use aurora_decoder_iamf::IamfDecoderAdapter;
#[cfg(feature = "native-truehd")]
use aurora_decoder_truehdd::TruehddDecoderAdapter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveRoute {
    Core,
    IamfRust,
    TrueHdNative,
}

pub struct AuroraSuperDecoder {
    config: EngineConfig,
    core: AuroraDecoderEngine,
    #[cfg(feature = "iamf")]
    iamf: IamfDecoderAdapter,
    #[cfg(feature = "native-truehd")]
    truehd: TruehddDecoderAdapter,
    active_route: ActiveRoute,
}

impl AuroraSuperDecoder {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            core: AuroraDecoderEngine::new(config),
            config,
            #[cfg(feature = "iamf")]
            iamf: IamfDecoderAdapter::new(),
            #[cfg(feature = "native-truehd")]
            truehd: TruehddDecoderAdapter::new(),
            active_route: ActiveRoute::Core,
        }
    }

    pub fn active_route(&self) -> ActiveRoute {
        self.active_route
    }

    pub fn core(&self) -> &AuroraDecoderEngine {
        &self.core
    }

    /// Decode using an explicit container/transport codec identity. This is the
    /// preferred API for streaming systems because fragmented packets may not
    /// contain enough sync bytes for reliable probing.
    pub fn decode_chunk_for(
        &mut self,
        codec: CodecId,
        input: &[u8],
    ) -> Result<Option<DecodedFrame>, DecoderError> {
        match codec {
            CodecId::Iamf => self.decode_iamf(input),
            CodecId::TrueHd | CodecId::TrueHdAtmos => self.decode_truehd(input),
            _ => {
                self.active_route = ActiveRoute::Core;
                self.core.decode_chunk(input)
            }
        }
    }

    /// Object-preserving immersive path. Formats are admitted here only when
    /// the selected backend exposes pre-render signal identity plus metadata;
    /// rendered speaker PCM is never relabeled as objects.
    pub fn decode_spatial_chunk_for(
        &mut self,
        codec: CodecId,
        input: &[u8],
    ) -> Result<Option<SpatialDecodedFrame>, DecoderError> {
        match codec {
            CodecId::Ac4 => {
                self.active_route = ActiveRoute::Core;
                self.core.decode_spatial_access_unit(codec, input)
            }
            CodecId::TrueHd | CodecId::TrueHdAtmos => self.decode_truehd_spatial(input),
            CodecId::Iamf => Err(DecoderError::UnsupportedInput(
                "IAMF pre-render scene export is not admitted yet; the native runtime currently exposes rendered speaker PCM only",
            )),
            _ => Err(DecoderError::UnsupportedInput(
                "selected codec has no admitted object-preserving Aurora Spatial IR path",
            )),
        }
    }

    #[cfg(feature = "iamf")]
    fn decode_iamf(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        self.active_route = ActiveRoute::IamfRust;
        self.iamf.decode_chunk(input)
    }

    #[cfg(not(feature = "iamf"))]
    fn decode_iamf(&mut self, _input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        Err(DecoderError::Unavailable(
            "Aurora super decoder was built without the IAMF backend",
        ))
    }

    #[cfg(feature = "native-truehd")]
    fn decode_truehd(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        self.active_route = ActiveRoute::TrueHdNative;
        self.truehd.decode_chunk(input)
    }

    #[cfg(not(feature = "native-truehd"))]
    fn decode_truehd(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        // The core still retains FFmpeg compatibility routing when the Rust
        // 1.88 native TrueHD feature is unavailable.
        self.active_route = ActiveRoute::Core;
        self.core.decode_chunk(input)
    }

    #[cfg(feature = "native-truehd")]
    fn decode_truehd_spatial(
        &mut self,
        input: &[u8],
    ) -> Result<Option<SpatialDecodedFrame>, DecoderError> {
        self.active_route = ActiveRoute::TrueHdNative;
        self.truehd.decode_spatial_chunk(input)
    }

    #[cfg(not(feature = "native-truehd"))]
    fn decode_truehd_spatial(
        &mut self,
        _input: &[u8],
    ) -> Result<Option<SpatialDecodedFrame>, DecoderError> {
        Err(DecoderError::Unavailable(
            "object-preserving TrueHD requires the native-truehd feature and Rust >=1.88",
        ))
    }
}

impl Decoder for AuroraSuperDecoder {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "Aurora Super Decoder",
            production_ready: false,
            maturity: "best-of-breed-composite-v1",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.core.configure(output_format)?;
        #[cfg(feature = "iamf")]
        self.iamf.configure(output_format)?;
        #[cfg(feature = "native-truehd")]
        self.truehd.configure(output_format)?;
        Ok(())
    }

    fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        match self.config.codec_hint {
            Some(codec) => self.decode_chunk_for(codec, input),
            None => {
                self.active_route = ActiveRoute::Core;
                self.core.decode_chunk(input)
            }
        }
    }

    fn reset(&mut self) {
        self.core.reset();
        #[cfg(feature = "iamf")]
        self.iamf.reset();
        #[cfg(feature = "native-truehd")]
        self.truehd.reset();
        self.active_route = ActiveRoute::Core;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::SampleType;

    fn format(channels: usize) -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: channels,
            sample_type: SampleType::F32,
            block_size: 40,
        }
    }

    #[test]
    fn default_super_decoder_keeps_core_as_safe_route() {
        let mut decoder = AuroraSuperDecoder::new(EngineConfig::default());
        decoder.configure(format(12)).unwrap();
        assert_eq!(decoder.active_route(), ActiveRoute::Core);
    }

    #[cfg(feature = "iamf")]
    #[test]
    fn explicit_iamf_route_is_selected_without_codec_sniffing() {
        let mut decoder = AuroraSuperDecoder::new(EngineConfig::default());
        decoder.configure(format(12)).unwrap();
        let _ = decoder.decode_chunk_for(CodecId::Iamf, &[]);
        assert_eq!(decoder.active_route(), ActiveRoute::IamfRust);
    }

    #[test]
    fn spatial_path_refuses_to_fake_iamf_objects() {
        let mut decoder = AuroraSuperDecoder::new(EngineConfig::default());
        decoder.configure(format(12)).unwrap();
        assert!(matches!(
            decoder.decode_spatial_chunk_for(CodecId::Iamf, &[]),
            Err(DecoderError::UnsupportedInput(_))
        ));
    }
}
