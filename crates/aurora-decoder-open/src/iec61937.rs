//! Streaming IEC 61937 burst depacketizer for eARC/S/PDIF capture.

use crate::sniff::{CodecKind, Encapsulation};

const PREAMBLE_LE: [u8; 4] = [0x72, 0xF8, 0x1F, 0x4E];

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

    /// Push arbitrary carrier bytes and return every complete burst that can
    /// be extracted. Fragmented preambles/payloads are retained for the next call.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Iec61937Burst> {
        self.buffer.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            let Some(start) = find_preamble(&self.buffer) else {
                // Keep at most the final 3 bytes because a preamble may straddle
                // the next input chunk.
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
            let data_type = (pc & 0x1F) as u8;
            let codec = codec_from_data_type(data_type);
            let payload_bytes = (usize::from(pd) + 7) / 8;
            if payload_bytes == 0 {
                // Invalid/empty burst: advance one word and resync.
                self.dropped_bytes = self.dropped_bytes.saturating_add(2);
                self.buffer.drain(..2);
                continue;
            }
            let padded_payload = (payload_bytes + 1) & !1;
            let need = 8usize.saturating_add(padded_payload);
            if self.buffer.len() < need {
                break;
            }

            let wire = self.buffer[8..8 + payload_bytes].to_vec();
            let payload = normalize_payload(codec, wire);
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

pub const fn codec_from_data_type(data_type: u8) -> CodecKind {
    match data_type {
        0x01 => CodecKind::Ac3,
        0x0B | 0x0C | 0x0D => CodecKind::Dts,
        0x11 => CodecKind::DtsHd,
        0x15 => CodecKind::Eac3,
        0x16 => CodecKind::TrueHd,
        _ => CodecKind::Unknown,
    }
}

fn find_preamble(data: &[u8]) -> Option<usize> {
    data.windows(PREAMBLE_LE.len())
        .position(|window| window == PREAMBLE_LE)
}

fn normalize_payload(codec: CodecKind, wire: Vec<u8>) -> Vec<u8> {
    let mut swapped = wire.clone();
    for pair in swapped.chunks_exact_mut(2) {
        pair.swap(0, 1);
    }

    let score = |candidate: &[u8]| match codec {
        CodecKind::Ac3 | CodecKind::Eac3 | CodecKind::Eac3Joc => {
            u8::from(candidate.starts_with(&[0x0B, 0x77])) * 3
        }
        CodecKind::TrueHd | CodecKind::Mlp => {
            u8::from(
                candidate.starts_with(&[0xF8, 0x72, 0x6F, 0xBA])
                    || candidate.starts_with(&[0xF8, 0x72, 0x6F, 0xBB]),
            ) * 3
        }
        CodecKind::Dts | CodecKind::DtsHd => {
            u8::from(
                candidate.starts_with(&[0x7F, 0xFE, 0x80, 0x01])
                    || candidate.starts_with(&[0xFE, 0x7F, 0x01, 0x80])
                    || candidate.starts_with(&[0x1F, 0xFF, 0xE8, 0x00])
                    || candidate.starts_with(&[0xFF, 0x1F, 0x00, 0xE8])
                    || candidate.starts_with(&[0x64, 0x58, 0x20, 0x25]),
            ) * 3
        }
        _ => 0,
    };
    if score(&swapped) > score(&wire) {
        swapped
    } else {
        wire
    }
}

/// The transport sniffer uses this helper to make clear that an IEC burst is
/// transport, not a codec by itself.
pub const fn encapsulation() -> Encapsulation {
    Encapsulation::Iec61937
}

#[cfg(test)]
mod tests {
    use super::*;

    fn burst(data_type: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = PREAMBLE_LE.to_vec();
        out.extend_from_slice(&u16::from(data_type).to_le_bytes());
        out.extend_from_slice(&((payload.len() * 8) as u16).to_le_bytes());
        out.extend_from_slice(payload);
        if payload.len() % 2 != 0 {
            out.push(0);
        }
        out
    }

    #[test]
    fn extracts_fragmented_eac3_burst() {
        let bytes = burst(0x15, &[0x0B, 0x77, 1, 2, 3, 4]);
        let mut d = Iec61937Depacketizer::new();
        assert!(d.push(&bytes[..5]).is_empty());
        let out = d.push(&bytes[5..]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].codec, CodecKind::Eac3);
        assert_eq!(out[0].payload, vec![0x0B, 0x77, 1, 2, 3, 4]);
    }

    #[test]
    fn repairs_word_swapped_ac3_payload() {
        let bytes = burst(0x01, &[0x77, 0x0B, 0x22, 0x11]);
        let mut d = Iec61937Depacketizer::new();
        let out = d.push(&bytes);
        assert_eq!(out[0].payload[0..2], [0x0B, 0x77]);
    }

    #[test]
    fn resynchronizes_after_garbage() {
        let mut bytes = vec![9, 8, 7, 6, 5];
        bytes.extend(burst(0x15, &[0x0B, 0x77, 0, 0]));
        let mut d = Iec61937Depacketizer::new();
        let out = d.push(&bytes);
        assert_eq!(out.len(), 1);
        assert!(d.dropped_bytes() >= 5);
    }
}
