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
const DEFAULT_PRESENTATION_RATE: u32 = 48_000;

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
    /// Exact Pa-to-Pa spacing in canonical carrier bytes for the latest two
    /// accepted bursts in this observation epoch. Unlike wall time, this is
    /// independent of read chunking and process scheduling.
    pub last_burst_spacing_bytes: Option<u64>,
    /// Smallest Pa-to-Pa carrier-byte spacing observed in the current epoch.
    pub min_burst_spacing_bytes: Option<u64>,
    /// Largest Pa-to-Pa carrier-byte spacing observed in the current epoch.
    pub max_burst_spacing_bytes: Option<u64>,
}

/// Stateful direct-eARC front end.
///
/// The wrapper deliberately clears any fixed codec hint supplied through either
/// layer of [`EngineConfig`]. eARC type 0x15 only proves E-AC-3/DD+, not JOC. The
/// decoder sniffing/policy layer must make any stronger codec classification from
/// the native payload itself and remain free to follow transport format changes.
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
    last_burst_carrier_offset: Option<u64>,
    last_burst_spacing_bytes: Option<u64>,
    min_burst_spacing_bytes: Option<u64>,
    max_burst_spacing_bytes: Option<u64>,
    presentation_sample_rate: u32,
    presentation_frames: u64,
}

impl DirectEarcDecoder {
    /// Creates a direct-eARC decoder using all IEC 61937 transport types.
    pub fn new(mut engine_config: EngineConfig) -> Self {
        engine_config.codec_hint = None;
        engine_config.open_decoder.codec_hint = None;
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
            last_burst_carrier_offset: None,
            last_burst_spacing_bytes: None,
            min_burst_spacing_bytes: None,
            max_burst_spacing_bytes: None,
            presentation_sample_rate: DEFAULT_PRESENTATION_RATE,
            presentation_frames: 0,
        }
    }

    /// Configures the downstream decoder output format.
    pub fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.engine.configure(output_format)?;
        self.presentation_sample_rate = output_format.sample_rate;
        self.presentation_frames = 0;
        Ok(())
    }

    fn begin_new_epoch(&mut self) {
        self.parser.reset();
        self.engine.reset();
        self.observation_epoch = self.observation_epoch.saturating_add(1);
        self.iec61937_locked = false;
        self.bursts_since_lock = 0;
        self.last_valid_burst = None;
        self.last_burst_carrier_offset = None;
        self.last_burst_spacing_bytes = None;
        self.min_burst_spacing_bytes = None;
        self.max_burst_spacing_bytes = None;
        self.presentation_frames = 0;
    }

    fn observe_valid_burst(&mut self, carrier_offset_bytes: u64) {
        if !self.iec61937_locked {
            if self.ever_locked {
                self.relocks = self.relocks.saturating_add(1);
            }
            self.ever_locked = true;
            self.iec61937_locked = true;
            self.bursts_since_lock = 0;
        }

        if let Some(previous) = self.last_burst_carrier_offset {
            let spacing = carrier_offset_bytes.saturating_sub(previous);
            self.last_burst_spacing_bytes = Some(spacing);
            self.min_burst_spacing_bytes = Some(
                self.min_burst_spacing_bytes
                    .map_or(spacing, |current| current.min(spacing)),
            );
            self.max_burst_spacing_bytes = Some(
                self.max_burst_spacing_bytes
                    .map_or(spacing, |current| current.max(spacing)),
            );
        }
        self.last_burst_carrier_offset = Some(carrier_offset_bytes);
        self.total_bursts = self.total_bursts.saturating_add(1);
        self.bursts_since_lock = self.bursts_since_lock.saturating_add(1);
        self.last_valid_burst = Some(Instant::now());
    }

    /// Re-stamps every decoder backend onto one direct-eARC presentation clock.
    /// Codec-format transitions may rebuild/reset the inner engine, but they are
    /// not transport discontinuities and therefore must not jump PTS back to zero.
    fn stamp_output_frame(&mut self, mut frame: DecodedFrame) -> DecodedFrame {
        let sample_rate = self.presentation_sample_rate.max(1);
        frame.audio.presentation_time_seconds =
            self.presentation_frames as f64 / f64::from(sample_rate);
        self.presentation_frames = self
            .presentation_frames
            .saturating_add(frame.audio.frame_count as u64);
        frame
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
            let frame = self.stamp_output_frame(frame);
            frames.push(frame);
            emitted = 1;
        }
        loop {
            match self.engine.decode_chunk(&[])? {
                Some(frame) => {
                    if emitted >= MAX_READY_FRAMES_PER_BURST {
                        return Err(DecoderError::Decode(
                            "decoder produced more than the bounded ready-frame limit for one IEC61937 burst"
                                .to_owned(),
                        ));
                    }
                    let frame = self.stamp_output_frame(frame);
                    frames.push(frame);
                    emitted += 1;
                }
                None => return Ok(()),
            }
        }
    }

    /// Flushes decoder-owned PCM before a transport codec transition, then
    /// resets codec detection/backend state for the newly observed IEC61937 type.
    ///
    /// Resetting first would silently discard a short (< block-size) JOC/worker
    /// tail that is still buffered behind the previous transport format. The
    /// outer presentation clock deliberately survives this codec-only reset.
    fn prepare_format_change(
        &mut self,
        frames: &mut Vec<DecodedFrame>,
    ) -> Result<(), DecoderError> {
        self.engine.flush_pending()?;
        self.collect_ready_frames(None, frames)?;
        self.engine.reset();
        Ok(())
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
            self.observe_valid_burst(observation.carrier_offset_bytes);
            batch.bursts += 1;
            batch.transport_codecs.push(observation.burst.codec);

            if observation.format_change.is_some() {
                // Retire the old decoder before resetting codec detection. This
                // preserves a short final PCM tail instead of dropping it at a
                // source/format switch.
                self.prepare_format_change(&mut batch.frames)?;
                self.total_format_changes = self.total_format_changes.saturating_add(1);
                batch.format_changes += 1;
            }

            // IEC61937 E-AC-3 data-bursts carry one complete six-block access
            // unit/repetition period. Preserve that authenticated transport
            // boundary so the JOC framer does not wait for the next AU merely to
            // prove an end boundary it already has. Other codecs retain the
            // generic byte-stream front door.
            let first = if observation.burst.codec == TransportCodec::Eac3 {
                self.engine
                    .decode_complete_eac3_access_unit(&observation.burst.payload)?
            } else {
                self.engine.decode_chunk(&observation.burst.payload)?
            };
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
            last_burst_spacing_bytes: self.last_burst_spacing_bytes,
            min_burst_spacing_bytes: self.min_burst_spacing_bytes,
            max_burst_spacing_bytes: self.max_burst_spacing_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::{AudioBlock, SampleType};

    fn format() -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: 12,
            sample_type: SampleType::F32,
            block_size: 40,
        }
    }

    fn silent_frame(frame_count: usize, synthetic_pts: f64) -> DecodedFrame {
        DecodedFrame {
            audio: AudioBlock {
                channels: (0..12).map(|_| vec![0.0; frame_count]).collect(),
                frame_count,
                presentation_time_seconds: synthetic_pts,
                discontinuity: false,
            },
            objects: Vec::new(),
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
                last_burst_spacing_bytes: None,
                min_burst_spacing_bytes: None,
                max_burst_spacing_bytes: None,
            }
        );
        assert!(!direct.engine().joc_status().codec_classified_joc);
        assert!(direct.finish().unwrap().frames.is_empty());
    }

    #[test]
    fn cadence_tracks_exact_carrier_spacing_without_wall_clock() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.observe_valid_burst(0);
        direct.observe_valid_burst(24_576);
        direct.observe_valid_burst(49_160);
        let health = direct.transport_telemetry();
        assert_eq!(health.last_burst_spacing_bytes, Some(24_584));
        assert_eq!(health.min_burst_spacing_bytes, Some(24_576));
        assert_eq!(health.max_burst_spacing_bytes, Some(24_584));
        assert_eq!(health.total_bursts, 3);
    }

    #[test]
    fn empty_format_change_retirement_is_lossless_and_ready_for_new_codec() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.configure(format()).unwrap();
        let mut frames = Vec::new();

        direct.prepare_format_change(&mut frames).unwrap();

        assert!(frames.is_empty());
        assert!(!direct.engine().joc_status().codec_classified_joc);
    }

    #[test]
    fn format_change_keeps_one_outer_presentation_clock() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.configure(format()).unwrap();
        let first = direct.stamp_output_frame(silent_frame(40, 123.0));
        assert_eq!(first.audio.presentation_time_seconds, 0.0);

        let mut retired = Vec::new();
        direct.prepare_format_change(&mut retired).unwrap();
        assert!(retired.is_empty());

        let after_change = direct.stamp_output_frame(silent_frame(16, 0.0));
        assert!(
            (after_change.audio.presentation_time_seconds - 40.0 / 48_000.0).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn discontinuity_starts_a_new_outer_presentation_epoch() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.configure(format()).unwrap();
        let _ = direct.stamp_output_frame(silent_frame(40, 0.0));
        direct.begin_new_epoch();
        let first = direct.stamp_output_frame(silent_frame(16, 99.0));
        assert_eq!(first.audio.presentation_time_seconds, 0.0);
    }

    #[test]
    fn discontinuity_clears_partial_transport_state_and_starts_new_epoch() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.configure(format()).unwrap();
        direct.observe_valid_burst(0);
        direct.observe_valid_burst(24_576);
        assert_eq!(direct.transport_telemetry().last_burst_spacing_bytes, Some(24_576));

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
        assert_eq!(health.last_burst_spacing_bytes, None);
        assert_eq!(health.min_burst_spacing_bytes, None);
        assert_eq!(health.max_burst_spacing_bytes, None);
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
