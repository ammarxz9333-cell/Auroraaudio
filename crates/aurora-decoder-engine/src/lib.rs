//! Proprietary Aurora decoder orchestration engine.
//!
//! This crate owns backend policy, deterministic routing, capability truth and
//! failover. Codec implementations remain isolated adapters and retain their
//! own licenses. See `LICENSE` and `THIRD_PARTY.md`.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod policy;

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use aurora_decoder_open::{OpenDecoderConfig, UniversalOpenDecoder};

use crate::catalog::{CodecId, DecoderCatalog};
use crate::policy::{BackendDecision, DecoderPolicy};

#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    pub open_decoder: OpenDecoderConfig,
    pub policy: DecoderPolicy,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            open_decoder: OpenDecoderConfig::default(),
            policy: DecoderPolicy::default(),
        }
    }
}

/// Aurora-owned front door for all codec backends.
///
/// The first implementation slice wraps the already-integrated open decoder
/// fabric while adding a deterministic policy layer. New native decoders are
/// promoted into the catalog only after conformance/evidence gates pass.
pub struct AuroraDecoderEngine {
    config: EngineConfig,
    catalog: DecoderCatalog,
    inner: UniversalOpenDecoder,
    active: Option<BackendDecision>,
}

impl AuroraDecoderEngine {
    pub fn new(config: EngineConfig) -> Self {
        Self {
            inner: UniversalOpenDecoder::new(config.open_decoder),
            catalog: DecoderCatalog::default(),
            config,
            active: None,
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

    fn refresh_decision(&mut self, codec: CodecId) -> Result<(), DecoderError> {
        self.active = self
            .config
            .policy
            .rank(&self.catalog, codec, true)
            .into_iter()
            .next();
        if self.active.is_none() && codec != CodecId::Unknown {
            return Err(DecoderError::UnsupportedInput(
                "Aurora policy found no integrated backend admitted for this codec",
            ));
        }
        Ok(())
    }
}

impl Decoder for AuroraDecoderEngine {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "Aurora Decoder Engine",
            production_ready: false,
            maturity: "proprietary-policy-engine-v1",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.inner.configure(output_format)
    }

    fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        if !input.is_empty() && self.inner.detected_codec().is_none() {
            let probe = aurora_decoder_open::sniff::probe(input);
            let codec = CodecId::from(probe.codec);
            if codec != CodecId::Unknown {
                self.refresh_decision(codec)?;
            }
        }

        let frame = self.inner.decode_chunk(input)?;

        if let Some(codec) = self.inner.detected_codec() {
            let codec = CodecId::from(codec);
            let changed = self
                .active
                .map(|decision| !decision.backend.supports(codec))
                .unwrap_or(true);
            if changed {
                self.refresh_decision(codec)?;
            }
        }

        Ok(frame)
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.active = None;
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
    }
}
