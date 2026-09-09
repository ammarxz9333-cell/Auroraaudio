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
use aurora_spatial_ir_v2::SpatialDecodedFrame as SpatialDecodedFrameV2;

#[cfg(feature = "iamf")]
use aurora_decoder_iamf::IamfDecoderAdapter;
#[cfg(feature = "native-truehd")]
use aurora_decoder_truehdd::{TruehddDecoderAdapter, TruehddV2DecoderAdapter};

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
    #[cfg(feature = "native-truehd")]
    truehd_v2: TruehddV2DecoderAdapter,
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
            #[cfg(feature = "native-truehd")]
            truehd_v2: TruehddV2DecoderAdapter::new(),
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

    /// Legacy object-preserving V1 path retained during migration.
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
                "selected codec has no admitted object-preserving Aurora Spatial IR V1 path",
            )),
        }
    }

    /// Rich object-preserving Spatial IR V2 path.
    ///
    /// AC-4 currently upgrades losslessly from its V1 scene contract. TrueHD
    /// uses the dedicated native V2 adapter so distance, extent, zone, screen,
    /// snap, headphone/dialogue intent and trim bypass are not discarded.
    pub fn decode_spatial_v2_chunk_for(
        &mut self,
        codec: CodecId,
        input: &[u8],
    ) -> Result<Option<SpatialDecodedFrameV2>, DecoderError> {
        match codec {
            CodecId::Ac4 => {
                self.active_route = ActiveRoute::Core;
                self.core
                    .decode_spatial_access_unit(codec, input)
                    .map(|frame| frame.map(Into::into))
            }
            CodecId::TrueHd | CodecId::TrueHdAtmos => self.decode_truehd_spatial_v2(input),
            CodecId::Iamf => Err(DecoderError::UnsupportedInput(
                "IAMF Spatial IR V2 pre-render scene export is not admitted yet",
            )),
            _ => Err(DecoderError::UnsupportedInput(
                "selected codec has no admitted object-preserving Aurora Spatial IR V2 path",
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

    #[cfg(feature = "native-truehd")]
    fn decode_truehd_spatial_v2(
        &mut self,
        input: &[u8],
    ) -> Result<Option<SpatialDecodedFrameV2>, DecoderError> {
        self.active_route = ActiveRoute::TrueHdNative;
        self.truehd_v2.decode_spatial_chunk_v2(input)
    }

    #[cfg(not(feature = "native-truehd"))]
    fn decode_truehd_spatial_v2(
        &mut self,
        _input: &[u8],
    ) -> Result<Option<SpatialDecodedFrameV2>, DecoderError> {
        Err(DecoderError::Unavailable(
            "rich TrueHD Spatial IR V2 requires the native-truehd feature and Rust >=1.88",
        ))
    }
}

impl Decoder for AuroraSuperDecoder {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "Aurora Super Decoder",
            production_ready: false,
            maturity: "best-of-breed-composite-spatial-ir-v2-experimental",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.core.configure(output_format)?;
        #[cfg(feature = "iamf")]
        self.iamf.configure(output_format)?;
        #[cfg(feature = "native-truehd")]
        self.truehd.configure(output_format)?;
        #[cfg(feature = "native-truehd")]
        self.truehd_v2.configure(output_format)?;
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
        #[cfg(feature = "native-truehd")]
        self.truehd_v2.reset();
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
    fn spatial_v1_path_refuses_to_fake_iamf_objects() {
        let mut decoder = AuroraSuperDecoder::new(EngineConfig::default());
        decoder.configure(format(12)).unwrap();
        assert!(matches!(
            decoder.decode_spatial_chunk_for(CodecId::Iamf, &[]),
            Err(DecoderError::UnsupportedInput(_))
        ));
    }

    #[test]
    fn spatial_v2_path_refuses_to_fake_iamf_objects() {
        let mut decoder = AuroraSuperDecoder::new(EngineConfig::default());
        decoder.configure(format(12)).unwrap();
        assert!(matches!(
            decoder.decode_spatial_v2_chunk_for(CodecId::Iamf, &[]),
            Err(DecoderError::UnsupportedInput(_))
        ));
    }

    #[test]
    fn ac4_v2_front_door_uses_lossless_v1_upgrade() {
        let mut decoder = AuroraSuperDecoder::new(EngineConfig::default());
        decoder.configure(format(12)).unwrap();
        assert!(decoder
            .decode_spatial_v2_chunk_for(CodecId::Ac4, &[])
            .unwrap()
            .is_none());
        assert_eq!(decoder.active_route(), ActiveRoute::Core);
    }
}
