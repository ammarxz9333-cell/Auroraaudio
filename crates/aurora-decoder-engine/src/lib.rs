//! Proprietary Aurora decoder orchestration engine.
//!
//! This crate owns backend policy, deterministic routing, capability truth and
//! failover. Codec implementations remain isolated adapters and retain their
//! own licenses. See `LICENSE` and `THIRD_PARTY.md`.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod evidence;
mod native_ac4;
mod native_dts;
pub mod policy;
pub mod spatial_ir;
pub mod telemetry;

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use aurora_decoder_open::{OpenDecoderConfig, UniversalOpenDecoder};

use crate::catalog::{BackendId, CodecId, DecoderCatalog};
use crate::native_ac4::{looks_like_ac4_sync, NativeAc4Decoder};
use crate::native_dts::{looks_like_dts_sync, NativeDtsDecoder};
use crate::policy::{BackendDecision, DecoderPolicy};
use crate::telemetry::EngineTelemetry;

#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    pub open_decoder: OpenDecoderConfig,
    /// Explicit transport/container hint. Hints select an admitted native
    /// backend before byte probing and are especially useful when a stream is
    /// delivered in fragments smaller than its codec sync word.
    pub codec_hint: Option<CodecId>,
    pub policy: DecoderPolicy,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            open_decoder: OpenDecoderConfig::default(),
            codec_hint: None,
            policy: DecoderPolicy::default(),
        }
    }
}

/// Aurora-owned front door for all codec backends.
///
/// The engine owns routing while codec implementations stay replaceable. Native
/// codecs are promoted only after they are wired and their evidence gates are
/// explicit in the catalog.
pub struct AuroraDecoderEngine {
    config: EngineConfig,
    catalog: DecoderCatalog,
    open: UniversalOpenDecoder,
    ac4: NativeAc4Decoder,
    dts: NativeDtsDecoder,
    active: Option<BackendDecision>,
    active_codec: Option<CodecId>,
    telemetry: EngineTelemetry,
}

impl AuroraDecoderEngine {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            open: UniversalOpenDecoder::new(config.open_decoder),
            ac4: NativeAc4Decoder::new(),
            dts: NativeDtsDecoder::new(),
            catalog: DecoderCatalog::default(),
            config,
            active: None,
            active_codec: None,
            telemetry: EngineTelemetry::default(),
        }
    }

    pub fn catalog(&self) -> &DecoderCatalog {
        &self.catalog
    }

    pub fn active_backend(&self) -> Option<BackendDecision> {
        self.active
    }

    pub fn ranked_candidates(&self, codec: CodecId) -> Vec<BackendDecision> {
        self.config.policy.rank(&self.catalog, codec, false)
    }

    /// Return a point-in-time health snapshot. The cumulative counters stay on
    /// the engine across stream resets; callers can explicitly clear them with
    /// `reset_telemetry` when starting a new measurement interval.
    pub fn telemetry(&self) -> EngineTelemetry {
        let mut snapshot = self.telemetry;
        snapshot.ac4_dropped_bytes = self.ac4.dropped_bytes();
        snapshot.dts_dropped_bytes = self.dts.dropped_bytes();
        snapshot.active_backend = self.active.map(|decision| decision.backend.id);
        snapshot.active_codec = self.active_codec;
        snapshot
    }

    pub fn reset_telemetry(&mut self) {
        self.telemetry = EngineTelemetry::default();
    }

    fn refresh_decision(&mut self, codec: CodecId) -> Result<(), DecoderError> {
        let previous = self.active.map(|decision| decision.backend.id);
        self.active = self
            .config
            .policy
            .rank(&self.catalog, codec, true)
            .into_iter()
            .next();
        let next = self.active.map(|decision| decision.backend.id);
        self.telemetry.observe_selection(previous, next);
        if self.active.is_none() && codec != CodecId::Unknown {
            return Err(DecoderError::UnsupportedInput(
                "Aurora policy found no integrated backend admitted for this codec",
            ));
        }
        self.active_codec = Some(codec);
        Ok(())
    }

    fn requested_codec(&self, input: &[u8]) -> CodecId {
        if let Some(codec) = self.config.codec_hint {
            return codec;
        }
        if looks_like_ac4_sync(input) {
            return CodecId::Ac4;
        }
        if looks_like_dts_sync(input) {
            return CodecId::Dts;
        }
        if let Some(codec) = self.open.detected_codec() {
            return CodecId::from(codec);
        }
        CodecId::from(aurora_decoder_open::sniff::probe(input).codec)
    }

    fn finish_decode(
        &mut self,
        result: Result<Option<DecodedFrame>, DecoderError>,
    ) -> Result<Option<DecodedFrame>, DecoderError> {
        match &result {
            Ok(Some(frame)) => self.telemetry.observe_frame(frame),
            Ok(None) => {}
            Err(error) => self.telemetry.observe_error(error),
        }
        result
    }

    fn decision_error(
        &mut self,
        error: DecoderError,
    ) -> Result<Option<DecodedFrame>, DecoderError> {
        self.telemetry.observe_error(&error);
        Err(error)
    }
}

impl Decoder for AuroraDecoderEngine {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "Aurora Decoder Engine",
            production_ready: false,
            maturity: "proprietary-policy-engine-native-ac4-dts-evidence-telemetry-v1",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.open.configure(output_format)?;
        self.ac4.configure(output_format);
        self.dts.configure(output_format);
        Ok(())
    }

    fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        self.telemetry.observe_input(input);

        if input.is_empty() {
            let result = match self.active_codec {
                Some(CodecId::Ac4) => self.ac4.poll(),
                Some(CodecId::Dts) => self.dts.poll(),
                _ => self.open.decode_chunk(input),
            };
            return self.finish_decode(result);
        }

        let requested = self.requested_codec(input);
        if requested == CodecId::Ac4 {
            if self.active_codec != Some(CodecId::Ac4) {
                if let Err(error) = self.refresh_decision(CodecId::Ac4) {
                    return self.decision_error(error);
                }
            }
            let packetized_raw = self.config.codec_hint == Some(CodecId::Ac4)
                && !looks_like_ac4_sync(input);
            let result = self.ac4.push(input, packetized_raw);
            return self.finish_decode(result);
        }
        if requested == CodecId::Dts {
            if self.active_codec != Some(CodecId::Dts) {
                if let Err(error) = self.refresh_decision(CodecId::Dts) {
                    return self.decision_error(error);
                }
            }
            let result = self.dts.push(input);
            return self.finish_decode(result);
        }

        let frame = match self.open.decode_chunk(input) {
            Ok(frame) => frame,
            Err(error) => return self.finish_decode(Err(error)),
        };
        if let Some(codec) = self.open.detected_codec() {
            let codec = CodecId::from(codec);
            let changed = self
                .active
                .map(|decision| !decision.backend.supports(codec))
                .unwrap_or(true)
                || self.active_codec != Some(codec);
            if changed {
                if let Err(error) = self.refresh_decision(codec) {
                    return self.decision_error(error);
                }
            }
        }
        self.finish_decode(Ok(frame))
    }

    fn reset(&mut self) {
        self.open.reset();
        self.ac4.reset();
        self.dts.reset();
        self.active = None;
        self.active_codec = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::BackendId;
    use aurora_core::SampleType;

    #[test]
    fn joc_prefers_openjoc_over_bed_decoder() {
        let engine = AuroraDecoderEngine::new(EngineConfig::default());
        let ranked = engine.ranked_candidates(CodecId::Eac3Joc);
        assert_eq!(ranked.first().map(|d| d.backend.id), Some(BackendId::OpenJoc));
    }

    #[test]
    fn truehd_catalog_prefers_native_candidate_but_active_route_stays_integrated() {
        let engine = AuroraDecoderEngine::new(EngineConfig::default());
        let ranked = engine.ranked_candidates(CodecId::TrueHd);
        assert_eq!(ranked.first().map(|d| d.backend.id), Some(BackendId::TrueHdNative));
        let active = engine.config.policy.rank(engine.catalog(), CodecId::TrueHd, true);
        assert_eq!(active.first().map(|d| d.backend.id), Some(BackendId::FfmpegWorker));
    }

    #[test]
    fn ac4_is_an_integrated_native_route() {
        let engine = AuroraDecoderEngine::new(EngineConfig::default());
        let active = engine.config.policy.rank(engine.catalog(), CodecId::Ac4, true);
        assert_eq!(active.first().map(|d| d.backend.id), Some(BackendId::OxideAc4));
    }

    #[test]
    fn dts_core_is_an_integrated_native_route() {
        let engine = AuroraDecoderEngine::new(EngineConfig::default());
        let active = engine.config.policy.rank(engine.catalog(), CodecId::Dts, true);
        assert_eq!(active.first().map(|d| d.backend.id), Some(BackendId::OxideDtsCore));
    }

    #[test]
    fn dts_hd_stays_on_compatibility_fallback() {
        let engine = AuroraDecoderEngine::new(EngineConfig::default());
        let active = engine.config.policy.rank(engine.catalog(), CodecId::DtsHd, true);
        assert_eq!(active.first().map(|d| d.backend.id), Some(BackendId::FfmpegWorker));
    }

    #[test]
    fn engine_keeps_aurora_40_frame_contract() {
        let mut engine = AuroraDecoderEngine::new(EngineConfig::default());
        engine
            .configure(AudioFormat {
                sample_rate: 48_000,
                channel_count: 12,
                sample_type: SampleType::F32,
                block_size: 40,
            })
            .unwrap();
        assert_eq!(engine.telemetry().input_chunks, 0);
    }

    #[test]
    fn telemetry_counts_policy_rejection_without_fabricating_output() {
        let config = EngineConfig {
            codec_hint: Some(CodecId::Dts),
            policy: DecoderPolicy::product(),
            ..EngineConfig::default()
        };
        let mut engine = AuroraDecoderEngine::new(config);
        engine
            .configure(AudioFormat {
                sample_rate: 48_000,
                channel_count: 6,
                sample_type: SampleType::F32,
                block_size: 40,
            })
            .unwrap();
        let error = engine.decode_chunk(&[0x7F, 0xFE, 0x80, 0x01]).unwrap_err();
        assert!(matches!(error, DecoderError::UnsupportedInput(_)));
        let telemetry = engine.telemetry();
        assert_eq!(telemetry.input_chunks, 1);
        assert_eq!(telemetry.unsupported_errors, 1);
        assert_eq!(telemetry.frames_emitted, 0);
    }
}
