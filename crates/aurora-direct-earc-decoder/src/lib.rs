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
}
