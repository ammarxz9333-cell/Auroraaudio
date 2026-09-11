//! Streaming IEC 61937 burst depacketizer for callers that feed the universal
//! open decoder a canonical S16_LE carrier directly.
//!
//! The product direct-eARC path uses the dedicated `aurora-iec61937` parser
//! before this decoder boundary. This compatibility frontend intentionally
//! follows the same Pc/Pd and word-order rules so the two paths cannot disagree.

use crate::sniff::{CodecKind, Encapsulation};

const PREAMBLE_LE: [u8; 4] = [0x72, 0xF8, 0x1F, 0x4E];
const DATA_TYPE_EAC3: u8 = 0x15;
const DATA_TYPE_MAT: u8 = 0x16;
const MAX_PAYLOAD_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iec61937Burst {
    pub codec: CodecKind,
    pub data_type: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct Iec61937Depacketizer {
    buffer: Vec<u8>,
    dropped_bytes: u64,
}

impl Iec61937Depacketizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<Iec61937Burst> {
        self.buffer.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            let Some(start) = find_preamble(&self.buffer) else {
                let keep = self.buffer.len().min(PREAMBLE_LE.len() - 1);
                let drop = self.buffer.len().saturating_sub(keep);
                if drop > 0 {
                    self.dropped_bytes = self.dropped_bytes.saturating_add(drop as u64);
                    self.buffer.drain(..drop);
                }
                break;
            };
            if start > 0 {
                self.dropped_bytes = self.dropped_bytes.saturating_add(start as u64);
                self.buffer.drain(..start);
            }
            if self.buffer.len() < 8 {
                break;
            }

            let pc = u16::from_le_bytes([self.buffer[4], self.buffer[5]]);
            let pd = u16::from_le_bytes([self.buffer[6], self.buffer[7]]);
            let data_type = (pc & 0x7F) as u8;
            let codec = codec_from_data_type(data_type);
            let payload_bytes = payload_length_bytes(data_type, pd);
            if payload_bytes == 0 || payload_bytes > MAX_PAYLOAD_BYTES {
                self.dropped_bytes = self.dropped_bytes.saturating_add(2);
                self.buffer.drain(..2);
                continue;
            }

            let carrier_payload_bytes = payload_bytes.saturating_add(payload_bytes & 1);
            let need = 8usize.saturating_add(carrier_payload_bytes);
            if self.buffer.len() < need {
                break;
            }

            let mut payload = self.buffer[8..need].to_vec();
            for word in payload.chunks_exact_mut(2) {
                word.swap(0, 1);
            }
            payload.truncate(payload_bytes);
            out.push(Iec61937Burst {
                codec,
                data_type,
                payload,
            });
            self.buffer.drain(..need);
        }
        out
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.dropped_bytes = 0;
    }
}

/// Match the product parser exactly: E-AC-3/MAT use byte counts; legacy
/// bit-count types must be byte-aligned or the header is rejected.
const fn payload_length_bytes(data_type: u8, pd: u16) -> usize {
    if data_type == DATA_TYPE_EAC3 || data_type == DATA_TYPE_MAT {
        pd as usize
    } else if pd & 7 != 0 {
        0
    } else {
        pd as usize / 8
    }
}

pub const fn codec_from_data_type(data_type: u8) -> CodecKind {
    match data_type {
        0x01 => CodecKind::Ac3,
        0x0B..=0x0D => CodecKind::Dts,
        0x11 => CodecKind::DtsHd,
        0x15 => CodecKind::Eac3,
        0x16 => CodecKind::DolbyMat,
        _ => CodecKind::Unknown,
    }
}

fn find_preamble(data: &[u8]) -> Option<usize> {
    data.windows(PREAMBLE_LE.len())
        .position(|window| window == PREAMBLE_LE)
}

pub const fn encapsulation() -> Encapsulation {
    Encapsulation::Iec61937
}

#[cfg(test)]
mod tests {
    use super::*;

    fn burst(data_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = PREAMBLE_LE.to_vec();
        out.extend_from_slice(&u16::from(data_type).to_le_bytes());
        let pd = if data_type == DATA_TYPE_EAC3 || data_type == DATA_TYPE_MAT {
            payload.len() as u16
        } else {
            (payload.len() * 8) as u16
        };
        out.extend_from_slice(&pd.to_le_bytes());

        let mut wire = payload.to_vec();
        if wire.len() % 2 != 0 {
            wire.push(0);
        }
        for word in wire.chunks_exact_mut(2) {
            word.swap(0, 1);
        }
        out.extend_from_slice(&wire);
        out
    }

    #[test]
    fn extracts_fragmented_eac3_burst_using_byte_length_pd() {
        let payload = [0x0B, 0x77, 1, 2, 3, 4];
        let bytes = burst(DATA_TYPE_EAC3, &payload);
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), payload.len() as u16);

        let mut d = Iec61937Depacketizer::new();
        assert!(d.push(&bytes[..5]).is_empty());
        let out = d.push(&bytes[5..]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].codec, CodecKind::Eac3);
        assert_eq!(out[0].payload, payload);
    }

    #[test]
    fn ac3_uses_bit_length_pd_and_restores_word_order() {
        let payload = [0x0B, 0x77, 0x11, 0x22];
        let bytes = burst(0x01, &payload);
        assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]), 32);
        let mut d = Iec61937Depacketizer::new();
        let out = d.push(&bytes);
        assert_eq!(out[0].payload, payload);
    }

    #[test]
    fn non_byte_aligned_legacy_pd_is_rejected_and_resynchronizes() {
        let mut bytes = PREAMBLE_LE.to_vec();
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&9_u16.to_le_bytes());
        bytes.extend_from_slice(&burst(DATA_TYPE_EAC3, &[0x0B, 0x77, 0x11, 0x22]));
        let mut d = Iec61937Depacketizer::new();
        let out = d.push(&bytes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].codec, CodecKind::Eac3);
        assert!(d.dropped_bytes() >= 8);
    }

    #[test]
    fn odd_native_payload_restores_last_byte_without_carrier_pad() {
        let payload = [0x0B, 0x77, 0xA5];
        let bytes = burst(DATA_TYPE_EAC3, &payload);
        let mut d = Iec61937Depacketizer::new();
        let out = d.push(&bytes);
        assert_eq!(out[0].payload, payload);
    }

    #[test]
    fn preserves_full_seven_bit_pc_type_and_mat_semantics() {
        assert_eq!(codec_from_data_type(DATA_TYPE_MAT), CodecKind::DolbyMat);
        let bytes = burst(0x21, &[0xAA, 0xBB]);
        let mut d = Iec61937Depacketizer::new();
        let out = d.push(&bytes);
        assert_eq!(out[0].data_type, 0x21);
        assert_eq!(out[0].codec, CodecKind::Unknown);
    }

    #[test]
    fn resynchronizes_after_garbage() {
        let mut bytes = vec![9, 8, 7, 6, 5];
        bytes.extend(burst(DATA_TYPE_EAC3, &[0x0B, 0x77, 0, 0]));
        let mut d = Iec61937Depacketizer::new();
        let out = d.push(&bytes);
        assert_eq!(out.len(), 1);
        assert!(d.dropped_bytes() >= 5);
    }
}