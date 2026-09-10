//! Deterministic encoded-audio source helpers for Aurora simulation and fault injection.
//!
//! This crate generates carrier bytes only. It does not classify E-AC-3 as JOC/Atmos
//! and deliberately reuses Aurora's transport data-type constant without depending on
//! decoder behavior.

#![forbid(unsafe_code)]

pub mod latency;

use aurora_iec61937::DATA_TYPE_EAC3;
use openjoc_eac3::{AccessUnitParse, parse_access_unit_bounds};
pub use openjoc_eac3::Eac3Error;
use std::error::Error;
use std::fmt;

const PA_LE: [u8; 2] = [0x72, 0xF8];
const PB_LE: [u8; 2] = [0x1F, 0x4E];
const HEADER_BYTES: usize = 8;

/// E-AC-3 carrier period used by the direct-eARC product path.
pub const EAC3_BURST_PERIOD_BYTES: usize = 24_576;

/// Mirrors the bounded E-AC-3 ingress payload accepted by `aurora-iec61937`.
pub const EAC3_MAX_PAYLOAD_BYTES: usize = 24_560;

/// Canonical two-slot IEC61937 carrier rate used by the historical Aurora capture.
pub const EAC3_CARRIER_RATE_HZ: usize = 192_000;
/// Canonical carrier slot count.
pub const EAC3_CARRIER_CHANNELS: usize = 2;
/// Canonical carrier bytes per S16 sample.
pub const EAC3_CARRIER_BYTES_PER_SAMPLE: usize = 2;
/// Number of canonical carrier bytes representing one millisecond of wall time.
pub const EAC3_CARRIER_BYTES_PER_MS: usize =
    EAC3_CARRIER_RATE_HZ * EAC3_CARRIER_CHANNELS * EAC3_CARRIER_BYTES_PER_SAMPLE / 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimSourceError {
    IncorrectPeriodSize {
        expected: usize,
        actual: usize,
    },
    EmptyPayload,
    PayloadTooLarge {
        maximum: usize,
        actual: usize,
    },
    InvalidFaultRange {
        offset: usize,
        count: usize,
        carrier_bytes: usize,
    },
    InvalidTruncateLength {
        requested: usize,
        carrier_bytes: usize,
    },
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
            Self::InvalidFaultRange {
                offset,
                count,
                carrier_bytes,
            } => write!(
                formatter,
                "carrier fault range {offset}..+{count} exceeds {carrier_bytes} input bytes"
            ),
            Self::InvalidTruncateLength {
                requested,
                carrier_bytes,
            } => write!(
                formatter,
                "carrier truncate length {requested} must be smaller than {carrier_bytes} bytes"
            ),
        }
    }
}

impl Error for SimSourceError {}

/// A deterministic mutation applied to generated carrier bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierFault {
    /// Deletes exactly `count` bytes beginning at the absolute carrier byte `offset`.
    DeleteBytes { offset: usize, count: usize },
    /// Damages Pa while leaving the remainder of the period byte-for-byte intact.
    CorruptPa,
    /// Keeps only the first `length` bytes of the selected period.
    Truncate { length: usize },
}

/// Incremental E-AC-3 access-unit framer backed by Aurora's pinned OpenJOC revision.
///
/// A complete six-block E-AC-3 unit may require the following independent-frame
/// header, or finite EOS, to prove its boundary. Consequently `push()` can retain a
/// complete-looking final unit until more input arrives; `finish()` supplies that
/// finite-EOS proof and rejects a truncated final unit.
#[derive(Debug, Default)]
pub struct Eac3AccessUnitFramer {
    pending: Vec<u8>,
}

impl Eac3AccessUnitFramer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds elementary-stream bytes and emits every access unit whose boundary is proven.
    ///
    /// # Errors
    /// Returns the exact checked OpenJOC framing error for malformed or impossible
    /// E-AC-3 access-unit structure.
    pub fn push(&mut self, input: &[u8]) -> Result<Vec<Vec<u8>>, Eac3Error> {
        self.pending.extend_from_slice(input);
        self.drain_complete(false)
    }

    /// Finalizes a finite elementary stream and emits its final complete access unit.
    ///
    /// # Errors
    /// Returns the exact checked OpenJOC framing error if EOS leaves a truncated or
    /// structurally invalid access unit.
    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, Eac3Error> {
        let output = self.drain_complete(true)?;
        if self.pending.is_empty() {
            Ok(output)
        } else {
            Err(Eac3Error::InvalidAccessUnitRange)
        }
    }

    #[must_use]
    pub fn pending_bytes(&self) -> usize {
        self.pending.len()
    }

    fn drain_complete(&mut self, eos: bool) -> Result<Vec<Vec<u8>>, Eac3Error> {
        let mut output = Vec::new();
        loop {
            if self.pending.is_empty() {
                break;
            }
            match parse_access_unit_bounds(&self.pending, eos)? {
                AccessUnitParse::NeedMore => break,
                AccessUnitParse::Complete(length) => {
                    if length == 0 || length > self.pending.len() {
                        return Err(Eac3Error::InvalidAccessUnitRange);
                    }
                    output.push(self.pending.drain(..length).collect());
                }
            }
        }
        Ok(output)
    }
}

/// Applies one deterministic carrier mutation and returns the mutated byte stream.
///
/// This injector deliberately operates below IEC61937 parsing. It can therefore model
/// byte loss that a transport parser cannot authenticate by itself and lets downstream
/// E-AC-3 validation prove whether corruption is detected.
///
/// # Errors
/// Returns a checked range error when the requested mutation is outside the supplied
/// carrier stream.
pub fn inject_carrier_fault(
    carrier: &[u8],
    fault: CarrierFault,
) -> Result<Vec<u8>, SimSourceError> {
    match fault {
        CarrierFault::DeleteBytes { offset, count } => {
            let Some(end) = offset.checked_add(count) else {
                return Err(SimSourceError::InvalidFaultRange {
                    offset,
                    count,
                    carrier_bytes: carrier.len(),
                });
            };
            if end > carrier.len() {
                return Err(SimSourceError::InvalidFaultRange {
                    offset,
                    count,
                    carrier_bytes: carrier.len(),
                });
            }
            let mut mutated = Vec::with_capacity(carrier.len().saturating_sub(count));
            mutated.extend_from_slice(&carrier[..offset]);
            mutated.extend_from_slice(&carrier[end..]);
            Ok(mutated)
        }
        CarrierFault::CorruptPa => {
            if carrier.len() < PA_LE.len() {
                return Err(SimSourceError::InvalidFaultRange {
                    offset: 0,
                    count: PA_LE.len(),
                    carrier_bytes: carrier.len(),
                });
            }
            let mut mutated = carrier.to_vec();
            mutated[0] ^= 0x01;
            Ok(mutated)
        }
        CarrierFault::Truncate { length } => {
            if length >= carrier.len() {
                return Err(SimSourceError::InvalidTruncateLength {
                    requested: length,
                    carrier_bytes: carrier.len(),
                });
            }
            Ok(carrier[..length].to_vec())
        }
    }
}

/// Writes one period of non-IEC61937 idle/silence carrier while preserving wall-time.
///
/// This is used for dropped-burst/gap simulation. It deliberately contains no Pa/Pb
/// preamble and performs no heap allocation.
///
/// # Errors
/// Returns [`SimSourceError::IncorrectPeriodSize`] for a noncanonical period buffer.
pub fn write_idle_period(out: &mut [u8]) -> Result<(), SimSourceError> {
    if out.len() != EAC3_BURST_PERIOD_BYTES {
        return Err(SimSourceError::IncorrectPeriodSize {
            expected: EAC3_BURST_PERIOD_BYTES,
            actual: out.len(),
        });
    }
    out.fill(0);
    Ok(())
}

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
    use aurora_iec61937::{BurstFinishError, BurstParser, CodecFilter, TransportCodec};

    fn deterministic_payload(length: usize, seed: u8) -> Vec<u8> {
        (0..length)
            .map(|index| seed.wrapping_add((index as u8).wrapping_mul(37)))
            .collect()
    }

    fn minimal_eac3_frame(marker: u8) -> Vec<u8> {
        // Independent substream 0, 128-byte frame (`frmsiz = 63`), 48 kHz,
        // six audio blocks, stereo, no LFE, bsid 16. Long-form AU framing only
        // needs the acquisition header; the marker distinguishes test units.
        let mut frame = vec![0_u8; 128];
        frame[0] = 0x0B;
        frame[1] = 0x77;
        frame[2] = 0x00;
        frame[3] = 0x3F;
        frame[4] = 0x34;
        frame[5] = 0x80;
        frame[127] = marker;
        frame
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
    fn corrupt_pa_resyncs_at_following_period_without_fabricating_payload() {
        let first_payload = minimal_eac3_frame(0x11);
        let second_payload = minimal_eac3_frame(0x22);
        let mut first = [0u8; EAC3_BURST_PERIOD_BYTES];
        let mut second = [0u8; EAC3_BURST_PERIOD_BYTES];
        write_eac3_period(&first_payload, &mut first).unwrap();
        write_eac3_period(&second_payload, &mut second).unwrap();
        let damaged = inject_carrier_fault(&first, CarrierFault::CorruptPa).unwrap();

        let mut carrier = damaged;
        carrier.extend_from_slice(&second);
        let mut parser = BurstParser::new(CodecFilter::Eac3);
        let observations = parser.push(&carrier);
        parser.finish().unwrap();

        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].burst.payload, second_payload);
        assert_eq!(
            observations[0].carrier_offset_bytes,
            EAC3_BURST_PERIOD_BYTES as u64
        );
        assert!(parser.discarded_bytes() >= EAC3_BURST_PERIOD_BYTES as u64);
    }

    #[test]
    fn dropped_burst_gap_preserves_time_and_expands_pa_spacing() {
        let first_payload = minimal_eac3_frame(0x31);
        let second_payload = minimal_eac3_frame(0x32);
        let mut first = [0u8; EAC3_BURST_PERIOD_BYTES];
        let mut gap = [0u8; EAC3_BURST_PERIOD_BYTES];
        let mut second = [0u8; EAC3_BURST_PERIOD_BYTES];
        write_eac3_period(&first_payload, &mut first).unwrap();
        write_idle_period(&mut gap).unwrap();
        write_eac3_period(&second_payload, &mut second).unwrap();

        let mut carrier = Vec::with_capacity(EAC3_BURST_PERIOD_BYTES * 3);
        carrier.extend_from_slice(&first);
        carrier.extend_from_slice(&gap);
        carrier.extend_from_slice(&second);

        let mut parser = BurstParser::new(CodecFilter::Eac3);
        let observations = parser.push(&carrier);
        parser.finish().unwrap();

        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].burst.payload, first_payload);
        assert_eq!(observations[1].burst.payload, second_payload);
        assert_eq!(
            observations[1].carrier_offset_bytes - observations[0].carrier_offset_bytes,
            (EAC3_BURST_PERIOD_BYTES * 2) as u64
        );
    }

    #[test]
    fn truncated_eof_is_reported_by_existing_parser() {
        let payload = minimal_eac3_frame(0x41);
        let mut period = [0u8; EAC3_BURST_PERIOD_BYTES];
        write_eac3_period(&payload, &mut period).unwrap();
        let truncated = inject_carrier_fault(
            &period,
            CarrierFault::Truncate {
                length: HEADER_BYTES + 32,
            },
        )
        .unwrap();

        let mut parser = BurstParser::new(CodecFilter::Eac3);
        assert!(parser.push(&truncated).is_empty());
        assert!(matches!(
            parser.finish(),
            Err(BurstFinishError::TruncatedPayload { .. })
        ));
    }

    #[test]
    fn odd_payload_is_word_swapped_with_zero_pad_only_on_carrier() {
        let payload = [0x0B, 0x77, 0xAA, 0xBB, 0xCC];
        let mut period = [0u8; EAC3_BURST_PERIOD_BYTES];
        write_eac3_period(&payload, &mut period).unwrap();

        assert_eq!(
            &period[HEADER_BYTES..HEADER_BYTES + 6],
            &[0x77, 0x0B, 0xBB, 0xAA, 0x00, 0xCC]
        );

        let mut parser = BurstParser::new(CodecFilter::All);
        let observations = parser.push(&period);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].burst.payload, payload);
    }

    #[test]
    fn pinned_openjoc_framer_proves_boundaries_across_incremental_input() {
        let first = minimal_eac3_frame(0x11);
        let second = minimal_eac3_frame(0x22);
        let mut framer = Eac3AccessUnitFramer::new();

        assert!(framer.push(&first[..37]).unwrap().is_empty());
        assert!(framer.push(&first[37..]).unwrap().is_empty());
        assert_eq!(framer.pending_bytes(), first.len());

        let emitted = framer.push(&second[..8]).unwrap();
        assert_eq!(emitted, vec![first.clone()]);
        assert_eq!(framer.pending_bytes(), 8);

        assert!(framer.push(&second[8..]).unwrap().is_empty());
        assert_eq!(framer.finish().unwrap(), vec![second]);
        assert_eq!(framer.pending_bytes(), 0);
    }

    #[test]
    fn deleted_carrier_header_word_is_transport_parseable_but_au_invalid() {
        let payload = minimal_eac3_frame(0x5A);
        let mut period = [0u8; EAC3_BURST_PERIOD_BYTES];
        write_eac3_period(&payload, &mut period).unwrap();

        // Delete the second encoded 16-bit word, after Pa/Pb/Pc/Pd. IEC61937 has
        // no payload integrity field, so the parser still returns `Pd` bytes by
        // consuming two zero-padding bytes. OpenJOC must reject the damaged AU.
        let damaged = inject_carrier_fault(
            &period,
            CarrierFault::DeleteBytes {
                offset: HEADER_BYTES + 2,
                count: 2,
            },
        )
        .unwrap();

        let mut parser = BurstParser::new(CodecFilter::Eac3);
        let observations = parser.push(&damaged);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].burst.pd as usize, payload.len());
        assert_ne!(observations[0].burst.payload, payload);
        assert_eq!(parser.malformed_headers(), 0);
        parser.finish().unwrap();

        let mut framer = Eac3AccessUnitFramer::new();
        assert!(matches!(
            framer.push(&observations[0].burst.payload),
            Err(Eac3Error::MissingIndependentSubstreamZero { frame: 0 })
        ));
    }

    #[test]
    fn invalid_generator_and_fault_inputs_fail_closed() {
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
        assert_eq!(
            write_idle_period(&mut short_period),
            Err(SimSourceError::IncorrectPeriodSize {
                expected: EAC3_BURST_PERIOD_BYTES,
                actual: short_period.len(),
            })
        );

        assert_eq!(
            inject_carrier_fault(
                &[1, 2, 3],
                CarrierFault::DeleteBytes {
                    offset: 2,
                    count: 2,
                },
            ),
            Err(SimSourceError::InvalidFaultRange {
                offset: 2,
                count: 2,
                carrier_bytes: 3,
            })
        );
        assert_eq!(
            inject_carrier_fault(&[1, 2, 3], CarrierFault::Truncate { length: 3 }),
            Err(SimSourceError::InvalidTruncateLength {
                requested: 3,
                carrier_bytes: 3,
            })
        );
    }

    #[test]
    fn carrier_geometry_has_exact_32_ms_period() {
        assert_eq!(EAC3_CARRIER_BYTES_PER_MS, 768);
        assert_eq!(EAC3_BURST_PERIOD_BYTES / EAC3_CARRIER_BYTES_PER_MS, 32);
    }
}
