//! Direct eARC carrier-to-decoder bridge for Aurora.
//!
//! This crate joins two already separate Aurora-owned boundaries:
//! canonical IEC 61937 transport parsing and the decoder policy engine. It does
//! not infer Atmos/JOC from the eARC transport type and it does not modify the
//! encoded payload before handing it to the decoder engine.

#![forbid(unsafe_code)]

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError};
use aurora_decoder_engine::{AuroraDecoderEngine, EngineConfig};
use aurora_iec61937::{BurstParser, CodecFilter, TransportCodec};

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

/// Read-only IEC61937 parser health used during direct-eARC hardware bring-up.
///
/// `discarded_bytes` includes ordinary non-preamble carrier bytes such as idle
/// padding, so it must not be interpreted by itself as an eARC unlock. A rising
/// `malformed_headers` count is stronger evidence that a Pa/Pb candidate was
/// followed by an invalid length/header. Physical unlock/xrun events remain an
/// explicit responsibility of the ALSA/eARC capture layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirectEarcTransportTelemetry {
    /// Incomplete canonical carrier bytes currently retained by the parser.
    pub pending_carrier_bytes: usize,
    /// Non-preamble bytes discarded while searching for the next Pa/Pb sync.
    pub discarded_bytes: u64,
    /// Pa/Pb candidates rejected because their payload length was impossible.
    pub malformed_headers: u64,
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
}

impl DirectEarcDecoder {
    /// Creates a direct-eARC decoder using all IEC 61937 transport types.
    pub fn new(mut engine_config: EngineConfig) -> Self {
        engine_config.codec_hint = None;
        Self {
            parser: BurstParser::new(CodecFilter::All),
            engine: AuroraDecoderEngine::new(engine_config),
        }
    }

    /// Configures the downstream decoder output format.
    pub fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.engine.configure(output_format)
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
            self.parser.reset();
            self.engine.reset();
        }

        let observations = self.parser.push(carrier);
        let mut batch = DirectEarcDecodeBatch {
            discontinuity,
            ..DirectEarcDecodeBatch::default()
        };

        for observation in observations {
            batch.bursts += 1;
            batch.transport_codecs.push(observation.burst.codec);

            if observation.format_change.is_some() {
                // Never let buffered state from one compressed format leak into
                // another when the TV switches source or output mode.
                self.engine.reset();
                batch.format_changes += 1;
            }

            if let Some(frame) = self.engine.decode_chunk(&observation.burst.payload)? {
                batch.frames.push(frame);
            }
        }

        Ok(batch)
    }

    /// Explicitly resets transport and decoder state.
    pub fn reset(&mut self) {
        self.parser.reset();
        self.engine.reset();
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

    /// Snapshot of transport-parser health counters for bring-up diagnostics.
    pub fn transport_telemetry(&self) -> DirectEarcTransportTelemetry {
        DirectEarcTransportTelemetry {
            pending_carrier_bytes: self.parser.pending_bytes(),
            discarded_bytes: self.parser.discarded_bytes(),
            malformed_headers: self.parser.malformed_headers(),
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
    fn empty_carrier_never_fabricates_output() {
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
            }
        );
    }

    #[test]
    fn discontinuity_clears_partial_transport_state() {
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
    }

    #[test]
    fn transport_telemetry_reports_discarded_carrier_bytes() {
        let mut direct = DirectEarcDecoder::new(EngineConfig::default());
        direct.configure(format()).unwrap();

        direct.push_carrier(&[0, 0, 0, 0, 0, 0, 0, 0], false).unwrap();
        let health = direct.transport_telemetry();

        // The parser retains up to three bytes so a Pa/Pb preamble may straddle
        // the next read boundary; the rest are explicitly accounted as discard.
        assert_eq!(health.pending_carrier_bytes, 3);
        assert_eq!(health.discarded_bytes, 5);
        assert_eq!(health.malformed_headers, 0);
    }
}
