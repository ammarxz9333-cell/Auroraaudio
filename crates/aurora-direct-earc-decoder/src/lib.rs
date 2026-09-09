//! Direct eARC carrier-to-decoder bridge for Aurora.
//!
//! This crate joins two already separate Aurora-owned boundaries:
//! canonical IEC 61937 transport parsing and the decoder policy engine. It does
//! not infer Atmos/JOC from the eARC transport type and it does not modify the
//! encoded payload before handing it to the decoder engine.

#![forbid(unsafe_code)]

use std::time::Instant;

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError};
use aurora_decoder_engine::{AuroraDecoderEngine, EngineConfig};
use aurora_iec61937::{BurstParser, CodecFilter, TransportCodec};

const MAX_READY_FRAMES_PER_BURST: usize = 4096;

/// Result of processing one arbitrary chunk of canonical IEC 61937 carrier.
#[derive(Debug, Default)]
pub struct DirectEarcDecodeBatch {
    /// Decoded frames emitted by the existing Aurora decoder boundary.
    pub frames: Vec<DecodedFrame>,
    /// Number of complete IEC 61937 bursts consumed from this call.
    pub bursts: usize,
    /// Number of transport-format transitions observed in this call.
    pub format_changes: usize,
    /// True when the caller marked this input chunk as a real source discontinuity.
    pub discontinuity: bool,
    /// Transport classes observed in accepted bursts, in order.
    pub transport_codecs: Vec<TransportCodec>,
}

/// Read-only IEC61937 parser health used during direct-eARC bring-up.
///
/// `discarded_bytes` includes ordinary non-preamble carrier bytes such as idle
/// padding, so it must not be interpreted by itself as an eARC unlock. A rising
/// `malformed_headers` count is stronger evidence that a Pa/Pb candidate was
/// followed by an invalid length/header. `iec61937_locked` is a parser-level
/// observation only: it becomes true after a valid IEC61937 burst and is cleared
/// only by an explicit transport discontinuity/reset. It is not a claim about
/// the physical HDMI/eARC electrical link, and it never carries JOC state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectEarcTransportTelemetry {
    /// Incomplete canonical carrier bytes currently retained by the parser.
    pub pending_carrier_bytes: usize,
    /// Non-preamble bytes discarded while searching for the next Pa/Pb sync.
    pub discarded_bytes: u64,
    /// Pa/Pb candidates rejected because their payload length was impossible.
    pub malformed_headers: u64,
    /// True after at least one valid IEC61937 burst in the current observation epoch.
    pub iec61937_locked: bool,
    /// Transport observation epoch. Incremented on explicit reset/discontinuity.
    pub observation_epoch: u64,
    /// Total valid IEC61937 bursts observed since decoder creation.
    pub total_bursts: u64,
    /// Valid bursts observed since the current parser lock was acquired.
    pub bursts_since_lock: u64,
    /// Total compressed transport-format transitions observed.
    pub total_format_changes: u64,
    /// Number of parser lock acquisitions after the first successful lock.
    pub relocks: u64,
    /// Milliseconds since the most recent valid IEC61937 burst, when one exists.
    pub last_valid_burst_age_ms: Option<u64>,
}

/// Stateful direct-eARC front end.
///
/// The wrapper deliberately clears any fixed codec hint supplied in
/// [`EngineConfig`]. eARC type 0x15 only proves E-AC-3/DD+, not JOC. The existing
/// decoder sniffing/policy layer must make any stronger codec classification from
/// the native payload itself.
pub struct DirectEarcDecoder {
    parser: BurstParser,
    engine: AuroraDecoderEngine,
    observation_epoch: u64,
    iec61937_locked: bool,
    ever_locked: bool,
    total_bursts: u64,
    bursts_since_lock: u64,
    total_format_changes: u64,
    relocks: u64,
    last_valid_burst: Option<Instant>,
}

impl DirectEarcDecoder {
    /// Creates a direct-eARC decoder using all IEC 61937 transport types.
    pub fn new(mut engine_config: EngineConfig) -> Self {
        engine_config.codec_hint = None;
        Self {
            parser: BurstParser::new(CodecFilter::All),
            engine: AuroraDecoderEngine::new(engine_config),
            observation_epoch: 1,
            iec61937_locked: false,
            ever_locked: false,
            total_bursts: 0,
            bursts_since_lock: 0,
            total_format_changes: 0,
            relocks: 0,
            last_valid_burst: None,
        }
    }

    /// Configures the downstream decoder output format.
    pub fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.engine.configure(output_format)
    }

    fn begin_new_epoch(&mut self) {
        self.parser.reset();
        self.engine.reset();
        self.observation_epoch = self.observation_epoch.saturating_add(1);
        self.iec61937_locked = false;
        self.bursts_since_lock = 0;
        self.last_valid_burst = None;
    }

    fn observe_valid_burst(&mut self) {
        if !self.iec61937_locked {
            if self.ever_locked {
                self.relocks = self.relocks.saturating_add(1);
            }
            self.ever_locked = true;
            self.iec61937_locked = true;
            self.bursts_since_lock = 0;
        }
        self.total_bursts = self.total_bursts.saturating_add(1);
        self.bursts_since_lock = self.bursts_since_lock.saturating_add(1);
        self.last_valid_burst = Some(Instant::now());
    }

    /// Drain every PCM frame already made ready by exactly one encoded input
    /// burst. This is required because one E-AC-3/JOC access unit can yield many
    /// Aurora 40-frame blocks. Leaving any queued frame behind before the next
    /// encoded burst would allow a decoder that returns pending output first to
    /// skip consuming that new burst.
    fn collect_ready_frames(
        &mut self,
        first: Option<DecodedFrame>,
        frames: &mut Vec<DecodedFrame>,
    ) -> Result<(), DecoderError> {
        let mut emitted = 0usize;
        if let Some(frame) = first {
            frames.push(frame);
            emitted = 1;
        }
        loop {
            if emitted >= MAX_READY_FRAMES_PER_BURST {
                return Err(DecoderError::Decode(
                    "decoder produced an unbounded ready-frame sequence for one IEC61937 burst"
                        .to_owned(),
                ));
            }
            match self.engine.decode_chunk(&[])? {
                Some(frame) => {
                    frames.push(frame);
                    emitted += 1;
                }
                None => return Ok(()),
            }
        }
    }

    /// Pushes arbitrary canonical S16_LE IEC 61937 carrier bytes.
    ///
    /// `discontinuity` must be true after a real ALSA xrun, eARC unlock/relock,
    /// capture-device restart, or equivalent source break. In that case both the
    /// transport parser and codec decoder are reset before any new bytes are used.
    pub fn push_carrier(
        &mut self,
        carrier: &[u8],
        discontinuity: bool,
    ) -> Result<DirectEarcDecodeBatch, DecoderError> {
        if discontinuity {
            self.begin_new_epoch();
        }

        let observations = self.parser.push(carrier);
        let mut batch = DirectEarcDecodeBatch {
            discontinuity,
            ..DirectEarcDecodeBatch::default()
        };

        for observation in observations {
            self.observe_valid_burst();
            batch.bursts += 1;
            batch.transport_codecs.push(observation.burst.codec);

            if observation.format_change.is_some() {
                // Never let buffered state from one compressed format leak into
                // another when the TV switches source or output mode.
                self.engine.reset();
                self.total_format_changes = self.total_format_changes.saturating_add(1);
                batch.format_changes += 1;
            }

            // Consume this encoded burst exactly once, then drain all PCM made
            // ready by it before another encoded burst can enter the decoder.
            let first = self.engine.decode_chunk(&observation.burst.payload)?;
            self.collect_ready_frames(first, &mut batch.frames)?;
        }

        Ok(batch)
    }

    /// Validates the finite carrier stream, flushes codec/framer/renderer state,
    /// and returns every remaining decoded PCM frame without accepting new input.
    pub fn finish(&mut self) -> Result<DirectEarcDecodeBatch, DecoderError> {
        self.parser.finish().map_err(|error| {
            DecoderError::Decode(format!(
                "IEC61937 end-of-stream validation failed: {error}"
            ))
        })?;
        self.engine.flush_pending()?;
        let mut batch = DirectEarcDecodeBatch::default();
        self.collect_ready_frames(None, &mut batch.frames)?;
        Ok(batch)
    }

    /// Explicitly resets transport and decoder state.
    pub fn reset(&mut self) {
        self.begin_new_epoch();
    }

    /// Provides read-only access to decoder policy/telemetry for diagnostics.
    pub fn engine(&self) -> &AuroraDecoderEngine {
        &self.engine
    }

    /// Provides mutable access for advanced product integration.
    pub fn engine_mut(&mut self) -> &mut AuroraDecoderEngine {
        &mut self.engine
    }

    /// Number of incomplete carrier bytes currently buffered.
    pub fn pending_carrier_bytes(&self) -> usize {
        self.parser.pending_bytes()
    }

    /// Snapshot of transport/parser health only. JOC status is available from
    /// `engine().joc_status()` and is intentionally kept out of this structure.
    pub fn transport_telemetry(&self) -> DirectEarcTransportTelemetry {
        let last_valid_burst_age_ms = self.last_valid_burst.map(|instant| {
            let millis = instant.elapsed().as_millis();
            millis.min(u128::from(u64::MAX)) as u64
        });
        DirectEarcTransportTelemetry {
            pending_carrier_bytes: self.parser.pending_bytes(),
            discarded_bytes: self.parser.discarded_bytes(),
            malformed_headers: self.parser.malformed_headers(),
            iec61937_locked: self.iec61937_locked,
            observation_epoch: self.observation_epoch,
            total_bursts: self.total_bursts,
            bursts_since_lock: self.bursts_since_lock,
            total_format_changes: self.total_format_changes,
            relocks: self.relocks,
            last_valid_burst_age_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::SampleType;

    fn format() -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: 12,
            sample_type: SampleType::F32,
            block_size: 40,
        }
    }

    #[test]
    fn empty_carrier_never_fabricates_output_or_transport_lock() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.configure(format()).unwrap();

        let batch = direct.push_carrier(&[], false).unwrap();

        assert_eq!(batch.bursts, 0);
        assert!(batch.frames.is_empty());
        assert_eq!(direct.pending_carrier_bytes(), 0);
        assert_eq!(
            direct.transport_telemetry(),
            DirectEarcTransportTelemetry {
                pending_carrier_bytes: 0,
                discarded_bytes: 0,
                malformed_headers: 0,
                iec61937_locked: false,
                observation_epoch: 1,
                total_bursts: 0,
                bursts_since_lock: 0,
                total_format_changes: 0,
                relocks: 0,
                last_valid_burst_age_ms: None,
            }
        );
        assert!(!direct.engine().joc_status().codec_classified_joc);
        assert!(direct.finish().unwrap().frames.is_empty());
    }

    #[test]
    fn discontinuity_clears_partial_transport_state_and_starts_new_epoch() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.configure(format()).unwrap();

        // First half of Pa/Pb preamble remains pending.
        let first = direct.push_carrier(&[0x72, 0xF8], false).unwrap();
        assert_eq!(first.bursts, 0);
        assert_eq!(direct.pending_carrier_bytes(), 2);

        let after = direct.push_carrier(&[], true).unwrap();
        assert!(after.discontinuity);
        assert_eq!(direct.pending_carrier_bytes(), 0);
        assert!(after.frames.is_empty());
        let health = direct.transport_telemetry();
        assert_eq!(health.observation_epoch, 2);
        assert!(!health.iec61937_locked);
        assert_eq!(health.bursts_since_lock, 0);
        assert!(!direct.engine().joc_status().codec_classified_joc);
    }

    #[test]
    fn carrier_padding_does_not_claim_or_clear_parser_lock() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.configure(format()).unwrap();

        direct.push_carrier(&[0, 0, 0, 0, 0, 0, 0, 0], false).unwrap();
        let health = direct.transport_telemetry();

        // The parser retains up to three bytes so a Pa/Pb preamble may straddle
        // the next read boundary; ordinary padding is accounted as discard only.
        assert_eq!(health.pending_carrier_bytes, 3);
        assert_eq!(health.discarded_bytes, 5);
        assert_eq!(health.malformed_headers, 0);
        assert!(!health.iec61937_locked);
        assert_eq!(health.observation_epoch, 1);
    }

    #[test]
    fn finish_accepts_idle_padding_and_clears_pending_transport_tail() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.configure(format()).unwrap();
        direct.push_carrier(&[0; 8], false).unwrap();
        assert_eq!(direct.pending_carrier_bytes(), 3);

        let final_batch = direct.finish().unwrap();

        assert!(final_batch.frames.is_empty());
        assert_eq!(direct.pending_carrier_bytes(), 0);
        assert_eq!(direct.transport_telemetry().discarded_bytes, 8);
    }

    #[test]
    fn finish_rejects_truncated_iec61937_preamble() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.configure(format()).unwrap();
        direct.push_carrier(&[0x72, 0xF8], false).unwrap();

        let error = direct.finish().unwrap_err();

        assert!(matches!(error, DecoderError::Decode(_)));
        assert!(error.to_string().contains("end-of-stream"));
        assert_eq!(direct.pending_carrier_bytes(), 2);
    }
}
