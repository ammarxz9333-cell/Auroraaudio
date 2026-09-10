//! Proprietary Aurora decoder orchestration engine.
//!
//! This crate owns backend policy, deterministic routing, capability truth and
//! failover. Codec implementations remain isolated adapters and retain their
//! own licenses. See `LICENSE` and `THIRD_PARTY.md`.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod evidence;
mod native_ac4;
pub mod native_ac4_spatial;
mod native_dts;
pub mod policy;
pub mod scene_timeline;
pub mod spatial_ir;
pub mod telemetry;

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use aurora_decoder_open::{OpenDecoderConfig, UniversalOpenDecoder};

use crate::catalog::{BackendId, CodecId, DecoderCatalog};
use crate::native_ac4::{looks_like_ac4_sync, NativeAc4Decoder};
use crate::native_ac4_spatial::NativeAc4SpatialDecoder;
use crate::native_dts::{looks_like_dts_sync, NativeDtsDecoder};
use crate::policy::{BackendDecision, DecoderPolicy};
use crate::spatial_ir::SpatialDecodedFrame;
use crate::telemetry::EngineTelemetry;

#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    pub open_decoder: OpenDecoderConfig,
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

/// Read-only status of the open E-AC-3/JOC lane.
/// `speaker_render_active` is live-only; render details are the latest
/// successful OpenJOC observation in the current decoder epoch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JocDecoderStatus {
    pub codec_classified_joc: bool,
    pub speaker_render_active: bool,
    pub layout_name: Option<String>,
    pub channel_count: Option<usize>,
    pub latency_samples: Option<usize>,
    pub object_count: Option<u16>,
    pub complexity_index: Option<u8>,
    pub fallback_reason: Option<String>,
}

pub struct AuroraDecoderEngine {
    config: EngineConfig,
    catalog: DecoderCatalog,
    open: UniversalOpenDecoder,
    ac4: NativeAc4Decoder,
    ac4_spatial: NativeAc4SpatialDecoder,
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
            ac4_spatial: NativeAc4SpatialDecoder::new(),
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

    pub fn telemetry(&self) -> EngineTelemetry {
        let mut snapshot = self.telemetry;
        snapshot.ac4_dropped_bytes = self.ac4.dropped_bytes();
        snapshot.dts_dropped_bytes = self.dts.dropped_bytes();
        snapshot.active_backend = self.active.map(|decision| decision.backend.id);
        snapshot.active_codec = self.active_codec;
        snapshot
    }

    pub fn joc_status(&self) -> JocDecoderStatus {
        let active_render = self.open.joc_render_info();
        let observed_render = active_render.or_else(|| self.open.last_joc_render_info());
        JocDecoderStatus {
            codec_classified_joc: self.open.detected_codec().map(CodecId::from)
                == Some(CodecId::Eac3Joc),
            speaker_render_active: active_render.is_some(),
            layout_name: observed_render.map(|info| info.layout_name.clone()),
            channel_count: observed_render.map(|info| info.channel_count),
            latency_samples: observed_render.map(|info| info.latency_samples),
            object_count: observed_render.and_then(|info| info.object_count),
            complexity_index: observed_render.and_then(|info| info.complexity_index),
            fallback_reason: self.open.last_joc_error().map(str::to_owned),
        }
    }

    pub fn reset_telemetry(&mut self) {
        self.telemetry = EngineTelemetry::default();
    }

    pub fn flush_pending(&mut self) -> Result<(), DecoderError> {
        let result = match self.active_codec {
            Some(CodecId::Ac4) => self.ac4.finish_pending(),
            Some(CodecId::Dts) => self.dts.finish_pending(),
            _ => self.open.flush_packets(),
        };
        if let Err(error) = &result {
            self.telemetry.observe_error(error);
        }
        result
    }

    /// Decode one complete E-AC-3 access unit whose boundary was authenticated
    /// by the outer transport (the direct-eARC IEC61937 parser). This preserves
    /// the no-lookahead OpenJOC path while keeping engine policy and telemetry in
    /// sync with the ordinary `Decoder::decode_chunk` front door.
    pub fn decode_complete_eac3_access_unit(
        &mut self,
        input: &[u8],
    ) -> Result<Option<DecodedFrame>, DecoderError> {
        self.telemetry.observe_input(input);
        let frame = match self.open.decode_complete_eac3_access_unit(input) {
            Ok(frame) => frame,
            Err(error) => return self.finish_decode(Err(error)),
        };

        if let Some(codec) = self.open.detected_codec() {
            let codec = CodecId::from(codec);
            if !matches!(codec, CodecId::Eac3 | CodecId::Eac3Joc) {
                return self.decision_error(DecoderError::UnsupportedInput(
                    "transport-bounded E-AC-3 front door selected a non-E-AC-3 codec",
                ));
            }
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

    pub fn decode_spatial_access_unit(
        &mut self,
        codec: CodecId,
        input: &[u8],
    ) -> Result<Option<SpatialDecodedFrame>, DecoderError> {
        self.telemetry.observe_input(input);
        if codec != CodecId::Ac4 {
            let error = DecoderError::UnsupportedInput(
                "Aurora Spatial IR front door currently admits native AC-4 A-JOC only",
            );
            self.telemetry.observe_error(&error);
            return Err(error);
        }
        if self.active_codec != Some(CodecId::Ac4) {
            if let Err(error) = self.refresh_decision(CodecId::Ac4) {
                self.telemetry.observe_error(&error);
                return Err(error);
            }
        }
        let result = self.ac4_spatial.decode_access_unit(input);
        match &result {
            Ok(Some(frame)) => self.telemetry.observe_spatial_frame(frame),
            Ok(None) => {}
            Err(error) => self.telemetry.observe_error(error),
        }
        result
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
            maturity: "proprietary-policy-engine-native-ac4-dts-spatial-ir-evidence-telemetry-v1",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.open.configure(output_format)?;
        self.ac4.configure(output_format);
        self.ac4_spatial.configure(output_format.sample_rate)?;
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
        self.ac4_spatial.reset();
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

    fn format(channels: usize) -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: channels,
            sample_type: SampleType::F32,
            block_size: 40,
        }
    }

    #[test]
    fn joc_prefers_openjoc_over_bed_decoder() {
        let engine = AuroraDecoderEngine::new(EngineConfig::default());
        let ranked = engine.ranked_candidates(CodecId::Eac3Joc);
        assert_eq!(ranked.first().map(|d| d.backend.id), Some(BackendId::OpenJoc));
    }

    #[test]
    fn empty_engine_has_no_joc_claim() {
        let engine = AuroraDecoderEngine::new(EngineConfig::default());
        assert_eq!(engine.joc_status(), JocDecoderStatus::default());
    }

    #[test]
    fn empty_engine_flush_is_a_noop() {
        let mut engine = AuroraDecoderEngine::new(EngineConfig::default());
        engine.flush_pending().unwrap();
    }

    #[test]
    fn finite_dts_flush_rejects_truncated_sync_prefix() {
        let mut engine = AuroraDecoderEngine::new(EngineConfig {
            codec_hint: Some(CodecId::Dts),
            ..EngineConfig::default()
        });
        engine.configure(format(12)).unwrap();
        assert!(engine.decode_chunk(&[0x7F, 0xFE]).unwrap().is_none());

        let error = engine.flush_pending().unwrap_err();
        assert!(error.to_string().contains("truncated DTS syncword"));
        assert_eq!(engine.telemetry().native_decode_errors, 1);
    }

    #[test]
    fn finite_ac4_flush_rejects_truncated_sync_frame() {
        let mut engine = AuroraDecoderEngine::new(EngineConfig {
            codec_hint: Some(CodecId::Ac4),
            ..EngineConfig::default()
        });
        engine.configure(format(12)).unwrap();
        assert!(engine.decode_chunk(&[0xAC, 0x40]).unwrap().is_none());

        let error = engine.flush_pending().unwrap_err();
        assert!(error.to_string().contains("truncated AC-4 sync frame"));
        assert_eq!(engine.telemetry().native_decode_errors, 1);
    }

    #[test]
    fn bounded_eac3_front_door_rejects_empty_payload_and_counts_error() {
        let mut engine = AuroraDecoderEngine::new(EngineConfig::default());
        engine.configure(format(12)).unwrap();
        assert!(engine.decode_complete_eac3_access_unit(&[]).is_err());
        let telemetry = engine.telemetry();
        assert_eq!(telemetry.poll_calls, 1);
        assert_eq!(telemetry.unsupported_errors, 1);
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
        engine.configure(format(12)).unwrap();
        assert_eq!(engine.telemetry().input_chunks, 0);
    }

    #[test]
    fn spatial_front_door_rejects_non_ac4_without_fabricating_output() {
        let mut engine = AuroraDecoderEngine::new(EngineConfig::default());
        engine.configure(format(12)).unwrap();
        let error = engine
            .decode_spatial_access_unit(CodecId::Dts, &[0x7f, 0xfe, 0x80, 0x01])
            .unwrap_err();
        assert!(matches!(error, DecoderError::UnsupportedInput(_)));
        let telemetry = engine.telemetry();
        assert_eq!(telemetry.input_chunks, 1);
        assert_eq!(telemetry.unsupported_errors, 1);
        assert_eq!(telemetry.frames_emitted, 0);
    }

    #[test]
    fn empty_spatial_ac4_access_unit_does_not_fabricate_a_frame() {
        let mut engine = AuroraDecoderEngine::new(EngineConfig::default());
        engine.configure(format(12)).unwrap();
        assert!(engine
            .decode_spatial_access_unit(CodecId::Ac4, &[])
            .unwrap()
            .is_none());
        let telemetry = engine.telemetry();
        assert_eq!(telemetry.poll_calls, 1);
        assert_eq!(telemetry.frames_emitted, 0);
    }

    #[test]
    fn telemetry_counts_policy_rejection_without_fabricating_output() {
        let config = EngineConfig {
            codec_hint: Some(CodecId::Dts),
            policy: DecoderPolicy::product(),
            ..EngineConfig::default()
        };
        let mut engine = AuroraDecoderEngine::new(config);
        engine.configure(format(6)).unwrap();
        let error = engine.decode_chunk(&[0x7F, 0xFE, 0x80, 0x01]).unwrap_err();
        assert!(matches!(error, DecoderError::UnsupportedInput(_)));
        let telemetry = engine.telemetry();
        assert_eq!(telemetry.input_chunks, 1);
        assert_eq!(telemetry.unsupported_errors, 1);
        assert_eq!(telemetry.frames_emitted, 0);
    }
}
