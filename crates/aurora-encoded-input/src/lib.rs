//! Unified encoded-carrier input boundary for Aurora.
//!
//! Both supported sources terminate at the same contract: canonical S16_LE
//! IEC 61937 carrier bytes plus explicit discontinuity/PTS metadata. Direct
//! eARC capture first normalizes S32_LE serial-audio slots; the legacy
//! STM32/USB path unwraps Aurora USB v1 packets without changing their encoded
//! payload. Codec/JOC classification is intentionally outside this crate.

#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use aurora_iec61937::{CarrierNormalizeError, CarrierWordHalf, S32LeCarrierNormalizer};
use aurora_usb_protocol::{
    Header, Kind, ProtocolError, FLAG_DISCONTINUITY, FLAG_PTS_VALID, FLAG_XRUN_RECOVERY,
    HEADER_LEN,
};

/// Physical/logical encoded input selected for one runtime instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodedInputKind {
    /// eARC receiver serial audio captured directly by the Linux SoC.
    DirectEarc,
    /// Existing STM32 transport carrying Aurora USB v1 ENCODED_IEC61937 frames.
    LegacyUsb,
}

/// Source-specific configuration. Selection is explicit; there is no automatic
/// fallback that could silently switch physical inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodedInputConfig {
    DirectEarc {
        slots: usize,
        word_half: CarrierWordHalf,
    },
    LegacyUsb,
}

impl EncodedInputConfig {
    pub const fn kind(self) -> EncodedInputKind {
        match self {
            Self::DirectEarc { .. } => EncodedInputKind::DirectEarc,
            Self::LegacyUsb => EncodedInputKind::LegacyUsb,
        }
    }
}

/// Canonical carrier emitted by either input family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierBatch {
    /// Canonical S16_LE IEC 61937 bytes, untouched after source normalization.
    pub carrier: Vec<u8>,
    /// True after an explicit transport break/xrun recovery indication.
    pub discontinuity: bool,
    /// Source PTS in Aurora's 48 kHz clock domain when supplied by legacy USB.
    pub pts_48k: Option<u64>,
}

#[derive(Debug)]
enum InputState {
    DirectEarc(S32LeCarrierNormalizer),
    LegacyUsb,
}

/// Stateful source selector/normalizer used by the unified runtime.
#[derive(Debug)]
pub struct EncodedInput {
    kind: EncodedInputKind,
    state: InputState,
}

impl EncodedInput {
    pub fn new(config: EncodedInputConfig) -> Result<Self, EncodedInputError> {
        let kind = config.kind();
        let state = match config {
            EncodedInputConfig::DirectEarc { slots, word_half } => InputState::DirectEarc(
                S32LeCarrierNormalizer::new(slots, word_half).map_err(EncodedInputError::Carrier)?,
            ),
            EncodedInputConfig::LegacyUsb => InputState::LegacyUsb,
        };
        Ok(Self { kind, state })
    }

    pub const fn kind(&self) -> EncodedInputKind {
        self.kind
    }

    /// Pushes arbitrary S32_LE bytes from a direct Linux I2S/SAI capture.
    /// Incomplete serial-audio frames are retained until the next call.
    pub fn push_direct_s32(&mut self, bytes: &[u8]) -> Result<Option<CarrierBatch>, EncodedInputError> {
        let InputState::DirectEarc(normalizer) = &mut self.state else {
            return Err(EncodedInputError::WrongSource {
                configured: self.kind,
                attempted: EncodedInputKind::DirectEarc,
            });
        };
        let carrier = normalizer.push(bytes);
        if carrier.is_empty() {
            return Ok(None);
        }
        Ok(Some(CarrierBatch {
            carrier,
            discontinuity: false,
            pts_48k: None,
        }))
    }

    /// Pushes complete native ALSA S32_LE slot samples directly.
    ///
    /// This is the preferred direct-eARC hardware path because it avoids an
    /// intermediate i32 -> byte-vector allocation before carrier normalization.
    /// The sample count must contain complete serial-audio frames.
    pub fn push_direct_s32_words(
        &mut self,
        samples: &[i32],
    ) -> Result<Option<CarrierBatch>, EncodedInputError> {
        let InputState::DirectEarc(normalizer) = &mut self.state else {
            return Err(EncodedInputError::WrongSource {
                configured: self.kind,
                attempted: EncodedInputKind::DirectEarc,
            });
        };
        let carrier = normalizer
            .push_s32_words(samples)
            .map_err(EncodedInputError::Carrier)?;
        if carrier.is_empty() {
            return Ok(None);
        }
        Ok(Some(CarrierBatch {
            carrier,
            discontinuity: false,
            pts_48k: None,
        }))
    }

    /// Pushes one complete Aurora USB v1 SOCK_SEQPACKET packet from the legacy
    /// STM32 bridge. Non-encoded control/clock/PCM packets are ignored here;
    /// ENCODED_IEC61937 payload bytes are returned byte-for-byte.
    pub fn push_legacy_usb_packet(
        &mut self,
        packet: &[u8],
    ) -> Result<Option<CarrierBatch>, EncodedInputError> {
        if !matches!(self.state, InputState::LegacyUsb) {
            return Err(EncodedInputError::WrongSource {
                configured: self.kind,
                attempted: EncodedInputKind::LegacyUsb,
            });
        }

        let header = Header::decode(packet).map_err(EncodedInputError::UsbProtocol)?;
        let declared = header.payload_len as usize;
        let expected = HEADER_LEN
            .checked_add(declared)
            .ok_or(EncodedInputError::UsbPacketLength {
                declared,
                actual: packet.len(),
            })?;
        if packet.len() != expected {
            return Err(EncodedInputError::UsbPacketLength {
                declared,
                actual: packet.len().saturating_sub(HEADER_LEN.min(packet.len())),
            });
        }

        if header.kind != Kind::EncodedIec61937 {
            return Ok(None);
        }

        let flags = header.flags;
        Ok(Some(CarrierBatch {
            carrier: packet[HEADER_LEN..].to_vec(),
            discontinuity: flags & (FLAG_DISCONTINUITY | FLAG_XRUN_RECOVERY) != 0,
            pts_48k: (flags & FLAG_PTS_VALID != 0).then_some(header.pts_48k),
        }))
    }

    /// Drops only source-local partial state after a known physical break.
    pub fn reset(&mut self) {
        if let InputState::DirectEarc(normalizer) = &mut self.state {
            normalizer.reset();
        }
    }

    /// Verifies direct-eARC capture ended on a complete serial-audio frame.
    pub fn finish(&self) -> Result<(), EncodedInputError> {
        if let InputState::DirectEarc(normalizer) = &self.state {
            normalizer.finish().map_err(EncodedInputError::Carrier)?;
        }
        Ok(())
    }
}

/// Input-boundary failures. Decoder errors are intentionally not represented
/// here because this crate owns transport normalization only.
#[derive(Debug)]
pub enum EncodedInputError {
    Carrier(CarrierNormalizeError),
    UsbProtocol(ProtocolError),
    UsbPacketLength {
        declared: usize,
        actual: usize,
    },
    WrongSource {
        configured: EncodedInputKind,
        attempted: EncodedInputKind,
    },
}

impl fmt::Display for EncodedInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Carrier(error) => write!(f, "direct eARC carrier error: {error}"),
            Self::UsbProtocol(error) => write!(f, "Aurora USB protocol error: {error}"),
            Self::UsbPacketLength { declared, actual } => write!(
                f,
                "Aurora USB payload length mismatch: declared {declared} bytes, actual {actual} bytes"
            ),
            Self::WrongSource {
                configured,
                attempted,
            } => write!(
                f,
                "encoded input is configured for {configured:?}, not attempted source {attempted:?}"
            ),
        }
    }
}

impl Error for EncodedInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Carrier(error) => Some(error),
            Self::UsbProtocol(error) => Some(error),
            Self::UsbPacketLength { .. } | Self::WrongSource { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn high_slot(word: u16) -> [u8; 4] {
        (u32::from(word) << 16).to_le_bytes()
    }

    fn usb_packet(kind: Kind, flags: u32, pts: u64, payload: &[u8]) -> Vec<u8> {
        let mut header = Header::new(kind, payload.len() as u32);
        header.flags = flags;
        header.pts_48k = pts;
        let mut packet = header.encode().to_vec();
        packet.extend_from_slice(payload);
        packet
    }

    #[test]
    fn direct_earc_normalizes_to_canonical_carrier() {
        let mut input = EncodedInput::new(EncodedInputConfig::DirectEarc {
            slots: 2,
            word_half: CarrierWordHalf::High,
        })
        .unwrap();
        let mut raw = Vec::new();
        raw.extend_from_slice(&high_slot(0xF872));
        raw.extend_from_slice(&high_slot(0x4E1F));

        let batch = input.push_direct_s32(&raw).unwrap().unwrap();
        assert_eq!(batch.carrier, [0x72, 0xF8, 0x1F, 0x4E]);
        assert!(!batch.discontinuity);
        assert_eq!(batch.pts_48k, None);
        input.finish().unwrap();
    }

    #[test]
    fn direct_native_words_match_byte_oriented_normalization() {
        let samples = [
            (u32::from(0xF872_u16) << 16) as i32,
            (u32::from(0x4E1F_u16) << 16) as i32,
        ];
        let mut native = EncodedInput::new(EncodedInputConfig::DirectEarc {
            slots: 2,
            word_half: CarrierWordHalf::High,
        })
        .unwrap();
        let batch = native
            .push_direct_s32_words(&samples)
            .unwrap()
            .unwrap();
        assert_eq!(batch.carrier, [0x72, 0xF8, 0x1F, 0x4E]);
        native.finish().unwrap();
    }

    #[test]
    fn native_word_path_rejects_wrong_source() {
        let mut input = EncodedInput::new(EncodedInputConfig::LegacyUsb).unwrap();
        assert!(matches!(
            input.push_direct_s32_words(&[0, 0]),
            Err(EncodedInputError::WrongSource { .. })
        ));
    }

    #[test]
    fn legacy_usb_payload_is_bit_exact_and_preserves_timing_flags() {
        let payload = [0x72, 0xF8, 0x1F, 0x4E, 0x15, 0x00, 0x80, 0x00];
        let packet = usb_packet(
            Kind::EncodedIec61937,
            FLAG_PTS_VALID | FLAG_DISCONTINUITY,
            123_456,
            &payload,
        );
        let mut input = EncodedInput::new(EncodedInputConfig::LegacyUsb).unwrap();

        let batch = input.push_legacy_usb_packet(&packet).unwrap().unwrap();
        assert_eq!(batch.carrier, payload);
        assert!(batch.discontinuity);
        assert_eq!(batch.pts_48k, Some(123_456));
    }

    #[test]
    fn legacy_control_packet_is_not_promoted_to_audio() {
        let packet = usb_packet(Kind::ClockReport, 0, 0, &[0_u8; 24]);
        let mut input = EncodedInput::new(EncodedInputConfig::LegacyUsb).unwrap();
        assert!(input.push_legacy_usb_packet(&packet).unwrap().is_none());
    }

    #[test]
    fn malformed_usb_payload_length_is_rejected() {
        let mut packet = usb_packet(Kind::EncodedIec61937, 0, 0, &[1, 2, 3, 4]);
        packet.pop();
        let mut input = EncodedInput::new(EncodedInputConfig::LegacyUsb).unwrap();
        assert!(matches!(
            input.push_legacy_usb_packet(&packet),
            Err(EncodedInputError::UsbPacketLength { .. })
        ));
    }

    #[test]
    fn source_selection_is_fail_closed() {
        let mut input = EncodedInput::new(EncodedInputConfig::LegacyUsb).unwrap();
        assert!(matches!(
            input.push_direct_s32(&[0; 8]),
            Err(EncodedInputError::WrongSource { .. })
        ));
    }

    #[test]
    fn direct_reset_discards_partial_serial_audio_frame() {
        let mut input = EncodedInput::new(EncodedInputConfig::DirectEarc {
            slots: 2,
            word_half: CarrierWordHalf::High,
        })
        .unwrap();
        assert!(input.push_direct_s32(&[1, 2, 3]).unwrap().is_none());
        input.reset();
        input.finish().unwrap();
    }
}
