//! Aurora-owned IEC 61937 ingress primitives for direct eARC capture.
//!
//! This crate is transport-only. It does not decode Dolby, DTS, JOC or object
//! metadata. Its job is to preserve the encoded payload exactly while removing
//! the IEC 61937 wrapper and reporting stream-type transitions.

#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

mod carrier;
pub use carrier::{CarrierNormalizeError, CarrierWordHalf, S32LeCarrierNormalizer};

const PA_LE: [u8; 2] = [0x72, 0xF8];
const PB_LE: [u8; 2] = [0x1F, 0x4E];
const PREAMBLE_LE: [u8; 4] = [PA_LE[0], PA_LE[1], PB_LE[0], PB_LE[1]];
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_EAC3_PAYLOAD_BYTES: usize = 24_560;

pub const DATA_TYPE_AC3: u8 = 0x01;
pub const DATA_TYPE_EAC3: u8 = 0x15;
pub const DATA_TYPE_MAT: u8 = 0x16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportCodec {
    Ac3,
    Eac3,
    MatTrueHd,
    DtsCore,
    Other(u8),
}

impl TransportCodec {
    pub fn from_data_type(data_type: u8) -> Self {
        match data_type {
            DATA_TYPE_AC3 => Self::Ac3,
            DATA_TYPE_EAC3 => Self::Eac3,
            DATA_TYPE_MAT => Self::MatTrueHd,
            0x0B..=0x0D => Self::DtsCore,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecFilter {
    Ac3,
    Eac3,
    MatTrueHd,
    DtsCore,
    All,
}

impl CodecFilter {
    pub fn accepts(self, data_type: u8) -> bool {
        match self {
            Self::Ac3 => data_type == DATA_TYPE_AC3,
            Self::Eac3 => data_type == DATA_TYPE_EAC3,
            Self::MatTrueHd => data_type == DATA_TYPE_MAT,
            Self::DtsCore => matches!(data_type, 0x0B..=0x0D),
            Self::All => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Burst {
    pub pc: u16,
    pub pd: u16,
    pub data_type: u8,
    pub codec: TransportCodec,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatChange {
    pub previous: TransportCodec,
    pub current: TransportCodec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurstObservation {
    pub burst: Burst,
    pub format_change: Option<FormatChange>,
    pub carrier_offset_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BurstFinishError {
    TruncatedPreamble { matched_bytes: usize },
    TruncatedHeader { pending_bytes: usize },
    TruncatedPayload {
        pending_bytes: usize,
        expected_bytes: usize,
    },
}

impl fmt::Display for BurstFinishError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedPreamble { matched_bytes } => write!(
                formatter,
                "IEC61937 stream ended after {matched_bytes} byte(s) of the Pa/Pb preamble"
            ),
            Self::TruncatedHeader { pending_bytes } => write!(
                formatter,
                "IEC61937 stream ended with a {pending_bytes}-byte incomplete burst header"
            ),
            Self::TruncatedPayload {
                pending_bytes,
                expected_bytes,
            } => write!(
                formatter,
                "IEC61937 stream ended with {pending_bytes} bytes of a burst requiring {expected_bytes} bytes"
            ),
        }
    }
}

impl Error for BurstFinishError {}

#[derive(Debug)]
pub struct BurstParser {
    filter: CodecFilter,
    buffer: Vec<u8>,
    stream_offset_bytes: u64,
    last_codec: Option<TransportCodec>,
    discarded_bytes: u64,
    malformed_headers: u64,
}

impl BurstParser {
    pub fn new(filter: CodecFilter) -> Self {
        Self {
            filter,
            buffer: Vec::with_capacity(32 * 1024),
            stream_offset_bytes: 0,
            last_codec: None,
            discarded_bytes: 0,
            malformed_headers: 0,
        }
    }

    pub fn push(&mut self, input: &[u8]) -> Vec<BurstObservation> {
        self.buffer.extend_from_slice(input);
        let mut observations = Vec::new();

        loop {
            let Some(sync_offset) = find_sync(&self.buffer) else {
                self.discard_non_sync_tail();
                break;
            };

            if sync_offset > 0 {
                self.discarded_bytes = self.discarded_bytes.saturating_add(sync_offset as u64);
                self.stream_offset_bytes = self
                    .stream_offset_bytes
                    .saturating_add(sync_offset as u64);
                self.buffer.drain(..sync_offset);
            }
            if self.buffer.len() < 8 {
                break;
            }

            let pc = u16::from_le_bytes([self.buffer[4], self.buffer[5]]);
            let pd = u16::from_le_bytes([self.buffer[6], self.buffer[7]]);
            let data_type = (pc & 0x7F) as u8;
            let payload_bytes = payload_length_bytes(data_type, pd);

            if !valid_payload_length(data_type, payload_bytes) {
                self.malformed_headers = self.malformed_headers.saturating_add(1);
                self.discarded_bytes = self.discarded_bytes.saturating_add(2);
                self.stream_offset_bytes = self.stream_offset_bytes.saturating_add(2);
                self.buffer.drain(..2);
                continue;
            }

            let carrier_payload_bytes = payload_bytes.saturating_add(payload_bytes & 1);
            let total = 8usize.saturating_add(carrier_payload_bytes);
            if self.buffer.len() < total {
                break;
            }

            let carrier_offset_bytes = self.stream_offset_bytes;
            if self.filter.accepts(data_type) {
                let mut payload = self.buffer[8..total].to_vec();
                for word in payload.chunks_exact_mut(2) {
                    word.swap(0, 1);
                }
                payload.truncate(payload_bytes);

                let codec = TransportCodec::from_data_type(data_type);
                let format_change = self.last_codec.and_then(|previous| {
                    (previous != codec).then_some(FormatChange {
                        previous,
                        current: codec,
                    })
                });
                self.last_codec = Some(codec);
                observations.push(BurstObservation {
                    burst: Burst {
                        pc,
                        pd,
                        data_type,
                        codec,
                        payload,
                    },
                    format_change,
                    carrier_offset_bytes,
                });
            }

            self.stream_offset_bytes = self.stream_offset_bytes.saturating_add(total as u64);
            self.buffer.drain(..total);
        }

        observations
    }

    pub fn finish(&mut self) -> Result<(), BurstFinishError> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        if let Some(sync_offset) = find_sync(&self.buffer) {
            let burst = &self.buffer[sync_offset..];
            if burst.len() < 8 {
                return Err(BurstFinishError::TruncatedHeader {
                    pending_bytes: burst.len(),
                });
            }

            let pc = u16::from_le_bytes([burst[4], burst[5]]);
            let pd = u16::from_le_bytes([burst[6], burst[7]]);
            let data_type = (pc & 0x7F) as u8;
            let payload_bytes = payload_length_bytes(data_type, pd);
            let carrier_payload_bytes = payload_bytes.saturating_add(payload_bytes & 1);
            let expected = 8usize.saturating_add(carrier_payload_bytes);
            if burst.len() < expected {
                return Err(BurstFinishError::TruncatedPayload {
                    pending_bytes: burst.len(),
                    expected_bytes: expected,
                });
            }
        }

        if let Some(matched_bytes) = trailing_sync_prefix_len(&self.buffer) {
            return Err(BurstFinishError::TruncatedPreamble { matched_bytes });
        }

        self.discarded_bytes = self
            .discarded_bytes
            .saturating_add(self.buffer.len() as u64);
        self.stream_offset_bytes = self
            .stream_offset_bytes
            .saturating_add(self.buffer.len() as u64);
        self.buffer.clear();
        Ok(())
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.stream_offset_bytes = 0;
        self.last_codec = None;
    }

    pub fn pending_bytes(&self) -> usize {
        self.buffer.len()
    }

    pub fn discarded_bytes(&self) -> u64 {
        self.discarded_bytes
    }

    pub fn malformed_headers(&self) -> u64 {
        self.malformed_headers
    }

    fn discard_non_sync_tail(&mut self) {
        const MAX_SYNC_STRADDLE: usize = 3;
        if self.buffer.len() > MAX_SYNC_STRADDLE {
            let discard = self.buffer.len() - MAX_SYNC_STRADDLE;
            self.discarded_bytes = self.discarded_bytes.saturating_add(discard as u64);
            self.stream_offset_bytes = self.stream_offset_bytes.saturating_add(discard as u64);
            self.buffer.drain(..discard);
        }
    }
}

/// Converts IEC61937 Pd to native payload bytes. E-AC-3/MAT use byte counts.
/// Legacy bit-count types are accepted only when the payload is byte-aligned;
/// a non-byte-aligned value returns zero so the parser treats the header as
/// malformed instead of silently truncating or rounding it.
pub fn payload_length_bytes(data_type: u8, pd: u16) -> usize {
    if matches!(data_type, DATA_TYPE_EAC3 | DATA_TYPE_MAT) {
        usize::from(pd)
    } else if pd & 7 != 0 {
        0
    } else {
        usize::from(pd) / 8
    }
}

fn valid_payload_length(data_type: u8, payload_bytes: usize) -> bool {
    payload_bytes != 0
        && payload_bytes <= MAX_PAYLOAD_BYTES
        && (data_type != DATA_TYPE_EAC3 || payload_bytes <= MAX_EAC3_PAYLOAD_BYTES)
}

fn find_sync(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == PREAMBLE_LE)
}

fn trailing_sync_prefix_len(buffer: &[u8]) -> Option<usize> {
    let max = buffer.len().min(PREAMBLE_LE.len().saturating_sub(1));
    (1..=max)
        .rev()
        .find(|&len| buffer[buffer.len() - len..] == PREAMBLE_LE[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eac3_type_15_round_trips_without_calling_it_joc() {
        let native = vec![0x0B, 0x77, 0x12, 0x34, 0xAB, 0xCD, 0xEF, 0x01];
        let carrier = make_burst(DATA_TYPE_EAC3, &native);
        let mut parser = BurstParser::new(CodecFilter::Eac3);
        let out = parser.push(&carrier);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].burst.data_type, DATA_TYPE_EAC3);
        assert_eq!(out[0].burst.codec, TransportCodec::Eac3);
        assert_eq!(out[0].burst.payload, native);
        assert_eq!(out[0].carrier_offset_bytes, 0);
        assert!(out[0].format_change.is_none());
        parser.finish().unwrap();
    }

    #[test]
    fn parser_survives_one_byte_read_boundaries() {
        let native = vec![0x0B, 0x77, 0x00, 0x02, 0x44, 0x55, 0x66, 0x77];
        let carrier = make_burst(DATA_TYPE_EAC3, &native);
        let mut parser = BurstParser::new(CodecFilter::All);
        let mut out = Vec::new();
        for byte in carrier {
            out.extend(parser.push(&[byte]));
        }
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].burst.payload, native);
        assert_eq!(out[0].carrier_offset_bytes, 0);
        parser.finish().unwrap();
    }

    #[test]
    fn odd_eac3_payload_restores_final_byte_exactly() {
        let native = vec![0x0B, 0x77, 0x10, 0x20, 0xAA];
        let carrier = make_burst(DATA_TYPE_EAC3, &native);
        let mut parser = BurstParser::new(CodecFilter::All);
        let out = parser.push(&carrier);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].burst.payload, native);
    }

    #[test]
    fn carrier_offsets_preserve_exact_padding_between_bursts() {
        const EAC3_PERIOD_BYTES: usize = 24_576;
        let first = make_burst(DATA_TYPE_EAC3, &[0x0B, 0x77, 0x10, 0x20]);
        let second = make_burst(DATA_TYPE_EAC3, &[0x0B, 0x77, 0x30, 0x40]);
        assert!(first.len() < EAC3_PERIOD_BYTES);
        let mut carrier = first;
        carrier.resize(EAC3_PERIOD_BYTES, 0);
        carrier.extend_from_slice(&second);
        let mut parser = BurstParser::new(CodecFilter::All);
        let out = parser.push(&carrier);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].carrier_offset_bytes, 0);
        assert_eq!(out[1].carrier_offset_bytes, EAC3_PERIOD_BYTES as u64);
    }

    #[test]
    fn eac3_to_ac3_reports_one_format_change() {
        let eac3 = make_burst(DATA_TYPE_EAC3, &[0x0B, 0x77, 0x10, 0x20]);
        let ac3 = make_burst(DATA_TYPE_AC3, &[0x0B, 0x77, 0x30, 0x40]);
        let mut parser = BurstParser::new(CodecFilter::All);
        let first = parser.push(&eac3);
        let second = parser.push(&ac3);
        assert!(first[0].format_change.is_none());
        assert_eq!(
            second[0].format_change,
            Some(FormatChange {
                previous: TransportCodec::Eac3,
                current: TransportCodec::Ac3,
            })
        );
    }

    #[test]
    fn reset_makes_next_burst_a_fresh_stream_not_a_format_change() {
        let eac3 = make_burst(DATA_TYPE_EAC3, &[0x0B, 0x77, 0x10, 0x20]);
        let ac3 = make_burst(DATA_TYPE_AC3, &[0x0B, 0x77, 0x30, 0x40]);
        let mut parser = BurstParser::new(CodecFilter::All);
        assert_eq!(parser.push(&eac3).len(), 1);
        parser.reset();
        let after_reset = parser.push(&ac3);
        assert!(after_reset[0].format_change.is_none());
        assert_eq!(after_reset[0].carrier_offset_bytes, 0);
    }

    #[test]
    fn zero_length_header_resynchronizes_to_next_valid_burst() {
        let mut input = Vec::new();
        input.extend_from_slice(&PA_LE);
        input.extend_from_slice(&PB_LE);
        input.extend_from_slice(&u16::from(DATA_TYPE_EAC3).to_le_bytes());
        input.extend_from_slice(&0_u16.to_le_bytes());
        input.extend_from_slice(&make_burst(
            DATA_TYPE_EAC3,
            &[0x0B, 0x77, 0x12, 0x34],
        ));
        let mut parser = BurstParser::new(CodecFilter::All);
        let out = parser.push(&input);
        assert_eq!(parser.malformed_headers(), 1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].burst.codec, TransportCodec::Eac3);
        assert_eq!(out[0].carrier_offset_bytes, 8);
    }

    #[test]
    fn oversized_eac3_header_resynchronizes_without_waiting_for_impossible_payload() {
        let mut input = Vec::new();
        input.extend_from_slice(&PA_LE);
        input.extend_from_slice(&PB_LE);
        input.extend_from_slice(&u16::from(DATA_TYPE_EAC3).to_le_bytes());
        input.extend_from_slice(&((MAX_EAC3_PAYLOAD_BYTES + 1) as u16).to_le_bytes());
        input.extend_from_slice(&make_burst(
            DATA_TYPE_EAC3,
            &[0x0B, 0x77, 0x12, 0x34],
        ));

        let mut parser = BurstParser::new(CodecFilter::All);
        let out = parser.push(&input);

        assert_eq!(parser.malformed_headers(), 1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].burst.codec, TransportCodec::Eac3);
        assert_eq!(out[0].carrier_offset_bytes, 8);
    }

    #[test]
    fn non_byte_aligned_legacy_pd_is_rejected_and_resynchronizes() {
        let mut input = Vec::new();
        input.extend_from_slice(&PA_LE);
        input.extend_from_slice(&PB_LE);
        input.extend_from_slice(&u16::from(DATA_TYPE_AC3).to_le_bytes());
        input.extend_from_slice(&9_u16.to_le_bytes());
        input.extend_from_slice(&make_burst(
            DATA_TYPE_EAC3,
            &[0x0B, 0x77, 0x12, 0x34],
        ));
        let mut parser = BurstParser::new(CodecFilter::All);
        let out = parser.push(&input);
        assert_eq!(parser.malformed_headers(), 1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].burst.codec, TransportCodec::Eac3);
    }

    #[test]
    fn pd_units_match_iec61937_codec_family_contract() {
        assert_eq!(payload_length_bytes(DATA_TYPE_AC3, 20_480), 2_560);
        assert_eq!(payload_length_bytes(DATA_TYPE_EAC3, 2_560), 2_560);
        assert_eq!(payload_length_bytes(DATA_TYPE_MAT, 61_424), 61_424);
        assert_eq!(payload_length_bytes(DATA_TYPE_AC3, 9), 0);
        assert!(valid_payload_length(DATA_TYPE_EAC3, MAX_EAC3_PAYLOAD_BYTES));
        assert!(!valid_payload_length(
            DATA_TYPE_EAC3,
            MAX_EAC3_PAYLOAD_BYTES + 1
        ));
    }

    #[test]
    fn eof_discards_idle_padding_but_rejects_partial_preamble() {
        let mut padding = BurstParser::new(CodecFilter::All);
        assert!(padding.push(&[0, 0, 0, 0, 0]).is_empty());
        assert_eq!(padding.pending_bytes(), 3);
        padding.finish().unwrap();
        assert_eq!(padding.pending_bytes(), 0);
        assert_eq!(padding.discarded_bytes(), 5);

        let mut partial = BurstParser::new(CodecFilter::All);
        assert!(partial.push(&[0, 0, PREAMBLE_LE[0], PREAMBLE_LE[1]]).is_empty());
        assert_eq!(
            partial.finish(),
            Err(BurstFinishError::TruncatedPreamble { matched_bytes: 2 })
        );
    }

    #[test]
    fn eof_rejects_incomplete_header_and_payload() {
        let mut header = BurstParser::new(CodecFilter::All);
        assert!(header.push(&PREAMBLE_LE).is_empty());
        assert_eq!(
            header.finish(),
            Err(BurstFinishError::TruncatedHeader { pending_bytes: 4 })
        );

        let full = make_burst(DATA_TYPE_EAC3, &[0x0B, 0x77, 0x12, 0x34, 0x56, 0x78]);
        let mut payload = BurstParser::new(CodecFilter::All);
        assert!(payload.push(&full[..full.len() - 2]).is_empty());
        assert!(matches!(
            payload.finish(),
            Err(BurstFinishError::TruncatedPayload { .. })
        ));
    }

    fn make_burst(data_type: u8, native_payload: &[u8]) -> Vec<u8> {
        let pd = if matches!(data_type, DATA_TYPE_EAC3 | DATA_TYPE_MAT) {
            native_payload.len() as u16
        } else {
            (native_payload.len() * 8) as u16
        };

        let mut burst = Vec::new();
        burst.extend_from_slice(&PA_LE);
        burst.extend_from_slice(&PB_LE);
        burst.extend_from_slice(&u16::from(data_type).to_le_bytes());
        burst.extend_from_slice(&pd.to_le_bytes());

        for pair in native_payload.chunks(2) {
            match pair {
                [a, b] => burst.extend_from_slice(&[*b, *a]),
                [a] => burst.extend_from_slice(&[0x00, *a]),
                _ => unreachable!(),
            }
        }
        burst
    }
}
