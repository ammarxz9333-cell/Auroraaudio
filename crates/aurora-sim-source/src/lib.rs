//! Deterministic encoded-audio source helpers for Aurora simulation and fault injection.
//!
//! This crate generates carrier bytes only. It does not classify E-AC-3 as JOC/Atmos
//! and deliberately reuses Aurora's transport data-type constant without depending on
//! decoder behavior.

#![forbid(unsafe_code)]

use aurora_iec61937::DATA_TYPE_EAC3;
use std::error::Error;
use std::fmt;

const PA_LE: [u8; 2] = [0x72, 0xF8];
const PB_LE: [u8; 2] = [0x1F, 0x4E];
const HEADER_BYTES: usize = 8;

/// E-AC-3 carrier period used by the direct-eARC product path.
pub const EAC3_BURST_PERIOD_BYTES: usize = 24_576;

/// Mirrors the bounded E-AC-3 ingress payload accepted by `aurora-iec61937`.
pub const EAC3_MAX_PAYLOAD_BYTES: usize = 24_560;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimSourceError {
    IncorrectPeriodSize { expected: usize, actual: usize },
    EmptyPayload,
    PayloadTooLarge { maximum: usize, actual: usize },
}

impl fmt::Display for SimSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncorrectPeriodSize { expected, actual } => write!(
                formatter,
                "IEC61937 E-AC-3 period buffer must be {expected} bytes, got {actual}"
            ),
            Self::EmptyPayload => write!(formatter, "IEC61937 E-AC-3 payload must not be empty"),
            Self::PayloadTooLarge { maximum, actual } => write!(
                formatter,
                "IEC61937 E-AC-3 payload is {actual} bytes; maximum is {maximum}"
            ),
        }
    }
}

impl Error for SimSourceError {}

/// Writes one complete, zero-padded IEC61937 E-AC-3 carrier period.
///
/// Aurora's ingress contract uses a byte-count `Pd` for data type `0x15`. Native
/// encoded bytes are swapped within each 16-bit carrier word, exactly reversing
/// the normalization performed by `aurora_iec61937::BurstParser`.
///
/// The function performs no heap allocation and overwrites the complete output
/// period on every call.
pub fn write_eac3_period(payload: &[u8], out: &mut [u8]) -> Result<(), SimSourceError> {
    if out.len() != EAC3_BURST_PERIOD_BYTES {
        return Err(SimSourceError::IncorrectPeriodSize {
            expected: EAC3_BURST_PERIOD_BYTES,
            actual: out.len(),
        });
    }
    if payload.is_empty() {
        return Err(SimSourceError::EmptyPayload);
    }
    if payload.len() > EAC3_MAX_PAYLOAD_BYTES {
        return Err(SimSourceError::PayloadTooLarge {
            maximum: EAC3_MAX_PAYLOAD_BYTES,
            actual: payload.len(),
        });
    }

    out.fill(0);
    out[0..2].copy_from_slice(&PA_LE);
    out[2..4].copy_from_slice(&PB_LE);
    out[4..6].copy_from_slice(&u16::from(DATA_TYPE_EAC3).to_le_bytes());
    out[6..8].copy_from_slice(&(payload.len() as u16).to_le_bytes());

    let mut source = 0usize;
    let mut carrier = HEADER_BYTES;
    while source + 1 < payload.len() {
        out[carrier] = payload[source + 1];
        out[carrier + 1] = payload[source];
        source += 2;
        carrier += 2;
    }

    if source < payload.len() {
        // Pad the final native byte to one complete carrier word before swapping.
        out[carrier] = 0;
        out[carrier + 1] = payload[source];
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_iec61937::{BurstParser, CodecFilter, TransportCodec};

    fn deterministic_payload(length: usize, seed: u8) -> Vec<u8> {
        (0..length)
            .map(|index| seed.wrapping_add((index as u8).wrapping_mul(37)))
            .collect()
    }

    #[test]
    fn one_period_round_trips_payload_exactly() {
        let payload = deterministic_payload(257, 0x0B);
        let mut period = [0u8; EAC3_BURST_PERIOD_BYTES];
        write_eac3_period(&payload, &mut period).unwrap();

        let mut parser = BurstParser::new(CodecFilter::Eac3);
        let observations = parser.push(&period);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].burst.data_type, DATA_TYPE_EAC3);
        assert_eq!(observations[0].burst.codec, TransportCodec::Eac3);
        assert_eq!(observations[0].burst.pd as usize, payload.len());
        assert_eq!(observations[0].burst.payload, payload);
        assert_eq!(observations[0].carrier_offset_bytes, 0);
        assert_eq!(parser.malformed_headers(), 0);
        parser.finish().unwrap();
    }

    #[test]
    fn arbitrary_read_boundaries_preserve_two_fixed_periods() {
        let first_payload = deterministic_payload(128, 0x11);
        let second_payload = deterministic_payload(511, 0x7A);
        let mut first = [0u8; EAC3_BURST_PERIOD_BYTES];
        let mut second = [0u8; EAC3_BURST_PERIOD_BYTES];
        write_eac3_period(&first_payload, &mut first).unwrap();
        write_eac3_period(&second_payload, &mut second).unwrap();

        let mut carrier = Vec::with_capacity(EAC3_BURST_PERIOD_BYTES * 2);
        carrier.extend_from_slice(&first);
        carrier.extend_from_slice(&second);

        let chunk_pattern = [1usize, 2, 3, 7, 31, 257, 4096, 19];
        let mut parser = BurstParser::new(CodecFilter::All);
        let mut observations = Vec::new();
        let mut cursor = 0usize;
        let mut chunk_index = 0usize;
        while cursor < carrier.len() {
            let requested = chunk_pattern[chunk_index % chunk_pattern.len()];
            let end = cursor.saturating_add(requested).min(carrier.len());
            observations.extend(parser.push(&carrier[cursor..end]));
            cursor = end;
            chunk_index += 1;
        }
        parser.finish().unwrap();

        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].burst.payload, first_payload);
        assert_eq!(observations[1].burst.payload, second_payload);
        assert_eq!(observations[0].carrier_offset_bytes, 0);
        assert_eq!(
            observations[1].carrier_offset_bytes,
            EAC3_BURST_PERIOD_BYTES as u64
        );
        assert_eq!(parser.malformed_headers(), 0);
    }

    #[test]
    fn odd_payload_is_word_swapped_with_zero_pad_only_on_carrier() {
        let payload = [0x0B, 0x77, 0xAA, 0xBB, 0xCC];
        let mut period = [0u8; EAC3_BURST_PERIOD_BYTES];
        write_eac3_period(&payload, &mut period).unwrap();

        assert_eq!(&period[HEADER_BYTES..HEADER_BYTES + 6], &[0x77, 0x0B, 0xBB, 0xAA, 0x00, 0xCC]);

        let mut parser = BurstParser::new(CodecFilter::All);
        let observations = parser.push(&period);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].burst.payload, payload);
    }

    #[test]
    fn invalid_generator_inputs_fail_closed() {
        let mut period = [0u8; EAC3_BURST_PERIOD_BYTES];
        assert_eq!(
            write_eac3_period(&[], &mut period),
            Err(SimSourceError::EmptyPayload)
        );

        let oversized = vec![0u8; EAC3_MAX_PAYLOAD_BYTES + 1];
        assert_eq!(
            write_eac3_period(&oversized, &mut period),
            Err(SimSourceError::PayloadTooLarge {
                maximum: EAC3_MAX_PAYLOAD_BYTES,
                actual: EAC3_MAX_PAYLOAD_BYTES + 1,
            })
        );

        let mut short_period = [0u8; 32];
        assert_eq!(
            write_eac3_period(&[1], &mut short_period),
            Err(SimSourceError::IncorrectPeriodSize {
                expected: EAC3_BURST_PERIOD_BYTES,
                actual: short_period.len(),
            })
        );
    }
}
