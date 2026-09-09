//! Single-process Aurora encoded-input runtime.
//!
//! This layer owns explicit source selection, source normalization and handoff
//! into the Aurora decoder engine. It deliberately stops before speaker
//! rendering/output transport so object-bearing decoder frames remain intact
//! for the renderer instead of being silently flattened or discarded.

#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, DecoderError};
use aurora_decoder_engine::EngineConfig;
use aurora_direct_earc_decoder::DirectEarcDecoder;
use aurora_encoded_input::{
    EncodedInput, EncodedInputConfig, EncodedInputError, EncodedInputKind,
};

/// Decoder output produced by one source-ingest call.
#[derive(Debug, Default)]
pub struct RuntimeBatch {
    pub frames: Vec<DecodedFrame>,
    pub bursts: usize,
    pub format_changes: usize,
    pub discontinuity: bool,
    pub pts_48k: Option<u64>,
}

/// One runtime instance is bound to exactly one input family until recreated.
/// This fail-closed rule prevents an accidental automatic switch between the
/// direct eARC receiver and the legacy STM32/USB fallback.
pub struct AuroraEncodedRuntime {
    input: EncodedInput,
    decoder: DirectEarcDecoder,
}

impl AuroraEncodedRuntime {
    pub fn new(
        input_config: EncodedInputConfig,
        engine_config: EngineConfig,
        output_format: AudioFormat,
    ) -> Result<Self, RuntimeError> {
        let input = EncodedInput::new(input_config).map_err(RuntimeError::Input)?;
        let mut decoder = DirectEarcDecoder::new(engine_config);
        decoder
            .configure(output_format)
            .map_err(RuntimeError::Decoder)?;
        Ok(Self { input, decoder })
    }

    pub const fn input_kind(&self) -> EncodedInputKind {
        self.input.kind()
    }

    /// Feeds raw S32_LE serial-audio capture bytes from the direct eARC path.
    pub fn push_direct_s32(&mut self, bytes: &[u8]) -> Result<RuntimeBatch, RuntimeError> {
        let Some(carrier) = self
            .input
            .push_direct_s32(bytes)
            .map_err(RuntimeError::Input)?
        else {
            return Ok(RuntimeBatch::default());
        };
        self.decode_carrier(carrier.carrier, carrier.discontinuity, carrier.pts_48k)
    }

    /// Feeds one complete Aurora USB v1 packet from the legacy STM32 bridge.
    /// Control/clock/output packets intentionally produce an empty batch.
    pub fn push_legacy_usb_packet(
        &mut self,
        packet: &[u8],
    ) -> Result<RuntimeBatch, RuntimeError> {
        let Some(carrier) = self
            .input
            .push_legacy_usb_packet(packet)
            .map_err(RuntimeError::Input)?
        else {
            return Ok(RuntimeBatch::default());
        };
        self.decode_carrier(carrier.carrier, carrier.discontinuity, carrier.pts_48k)
    }

    fn decode_carrier(
        &mut self,
        carrier: Vec<u8>,
        discontinuity: bool,
        pts_48k: Option<u64>,
    ) -> Result<RuntimeBatch, RuntimeError> {
        let decoded = self
            .decoder
            .push_carrier(&carrier, discontinuity)
            .map_err(RuntimeError::Decoder)?;
        Ok(RuntimeBatch {
            frames: decoded.frames,
            bursts: decoded.bursts,
            format_changes: decoded.format_changes,
            discontinuity: decoded.discontinuity,
            pts_48k,
        })
    }

    /// Clears both source-local partial state and decoder/parser state after an
    /// out-of-band physical reset.
    pub fn reset(&mut self) {
        self.input.reset();
        self.decoder.reset();
    }

    /// Checks source-local end-of-stream invariants.
    pub fn finish(&self) -> Result<(), RuntimeError> {
        self.input.finish().map_err(RuntimeError::Input)
    }

    pub fn decoder(&self) -> &DirectEarcDecoder {
        &self.decoder
    }

    pub fn decoder_mut(&mut self) -> &mut DirectEarcDecoder {
        &mut self.decoder
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    Input(EncodedInputError),
    Decoder(DecoderError),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => write!(f, "encoded input failed: {error}"),
            Self::Decoder(error) => write!(f, "decoder failed: {error}"),
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Decoder(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::SampleType;
    use aurora_encoded_input::EncodedInputConfig;
    use aurora_iec61937::CarrierWordHalf;
    use aurora_usb_protocol::{Header, Kind, FLAG_DISCONTINUITY};

    fn format() -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: 12,
            sample_type: SampleType::F32,
            block_size: 40,
        }
    }

    fn usb_packet(kind: Kind, flags: u32, payload: &[u8]) -> Vec<u8> {
        let mut header = Header::new(kind, payload.len() as u32);
        header.flags = flags;
        let mut packet = header.encode().to_vec();
        packet.extend_from_slice(payload);
        packet
    }

    #[test]
    fn direct_runtime_buffers_partial_serial_audio_without_fabricating_audio() {
        let mut runtime = AuroraEncodedRuntime::new(
            EncodedInputConfig::DirectEarc {
                slots: 2,
                word_half: CarrierWordHalf::High,
            },
            EngineConfig::default(),
            format(),
        )
        .unwrap();

        let batch = runtime.push_direct_s32(&[1, 2, 3]).unwrap();
        assert_eq!(runtime.input_kind(), EncodedInputKind::DirectEarc);
        assert_eq!(batch.bursts, 0);
        assert!(batch.frames.is_empty());
        runtime.reset();
        runtime.finish().unwrap();
    }

    #[test]
    fn legacy_control_packets_do_not_enter_decoder() {
        let mut runtime = AuroraEncodedRuntime::new(
            EncodedInputConfig::LegacyUsb,
            EngineConfig::default(),
            format(),
        )
        .unwrap();
        let packet = usb_packet(Kind::ClockReport, 0, &[0_u8; 24]);
        let batch = runtime.push_legacy_usb_packet(&packet).unwrap();
        assert_eq!(runtime.input_kind(), EncodedInputKind::LegacyUsb);
        assert_eq!(batch.bursts, 0);
        assert!(batch.frames.is_empty());
    }

    #[test]
    fn legacy_discontinuity_reaches_decoder_boundary_without_fake_output() {
        let mut runtime = AuroraEncodedRuntime::new(
            EncodedInputConfig::LegacyUsb,
            EngineConfig::default(),
            format(),
        )
        .unwrap();
        // A partial IEC61937 preamble is enough to verify reset propagation;
        // no codec payload is fabricated for this test.
        let packet = usb_packet(
            Kind::EncodedIec61937,
            FLAG_DISCONTINUITY,
            &[0x72, 0xF8],
        );
        let batch = runtime.push_legacy_usb_packet(&packet).unwrap();
        assert!(batch.discontinuity);
        assert_eq!(batch.bursts, 0);
        assert!(batch.frames.is_empty());
    }

    #[test]
    fn wrong_source_calls_fail_closed() {
        let mut runtime = AuroraEncodedRuntime::new(
            EncodedInputConfig::LegacyUsb,
            EngineConfig::default(),
            format(),
        )
        .unwrap();
        assert!(matches!(
            runtime.push_direct_s32(&[0_u8; 8]),
            Err(RuntimeError::Input(EncodedInputError::WrongSource { .. }))
        ));
    }
}
