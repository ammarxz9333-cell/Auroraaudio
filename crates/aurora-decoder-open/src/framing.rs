//! Streaming syncframe extraction for native AC-3 / E-AC-3 decode.

use crate::sniff::CodecKind;
use oxideav_ac3::{eac3, syncinfo};

#[derive(Debug)]
pub struct SyncFramer {
    codec: CodecKind,
    buffer: Vec<u8>,
    pending_eac3_group: Vec<u8>,
    dropped_bytes: u64,
}

impl SyncFramer {
    pub fn new(codec: CodecKind) -> Self {
        Self {
            codec,
            buffer: Vec::new(),
            pending_eac3_group: Vec::new(),
            dropped_bytes: 0,
        }
    }

    pub fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
    }

    /// Push arbitrary elementary-stream bytes and return complete decoder packets.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.buffer.extend_from_slice(bytes);
        match self.codec {
            CodecKind::Ac3 => self.extract_ac3(),
            CodecKind::Eac3 | CodecKind::Eac3Joc => self.extract_eac3(false),
            _ => Vec::new(),
        }
    }

    /// Emit any complete pending E-AC-3 independent+dependent group.
    pub fn flush(&mut self) -> Vec<Vec<u8>> {
        let mut out = match self.codec {
            CodecKind::Ac3 => self.extract_ac3(),
            CodecKind::Eac3 | CodecKind::Eac3Joc => self.extract_eac3(true),
            _ => Vec::new(),
        };
        if !self.pending_eac3_group.is_empty() {
            out.push(std::mem::take(&mut self.pending_eac3_group));
        }
        out
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.pending_eac3_group.clear();
        self.dropped_bytes = 0;
    }

    fn extract_ac3(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            if !self.resync() || self.buffer.len() < 5 {
                break;
            }
            let info = match syncinfo::parse(&self.buffer) {
                Ok(info) => info,
                Err(_) => {
                    self.buffer.drain(..2.min(self.buffer.len()));
                    self.dropped_bytes = self.dropped_bytes.saturating_add(2);
                    continue;
                }
            };
            let len = info.frame_length as usize;
            if self.buffer.len() < len {
                break;
            }
            out.push(self.buffer.drain(..len).collect());
        }
        out
    }

    fn extract_eac3(&mut self, flushing: bool) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            if !self.resync() || self.buffer.len() < 6 {
                break;
            }
            let bsi = match eac3::bsi::parse(&self.buffer[2..]) {
                Ok(bsi) => bsi,
                Err(_) => {
                    self.buffer.drain(..2.min(self.buffer.len()));
                    self.dropped_bytes = self.dropped_bytes.saturating_add(2);
                    continue;
                }
            };
            let len = bsi.frame_bytes as usize;
            if self.buffer.len() < len {
                break;
            }
            let frame: Vec<u8> = self.buffer.drain(..len).collect();
            match bsi.strmtyp {
                eac3::bsi::StreamType::Independent | eac3::bsi::StreamType::Ac3Convert => {
                    if !self.pending_eac3_group.is_empty() {
                        out.push(std::mem::take(&mut self.pending_eac3_group));
                    }
                    self.pending_eac3_group = frame;
                }
                eac3::bsi::StreamType::Dependent => {
                    if self.pending_eac3_group.is_empty() {
                        // A dependent frame without an independent predecessor cannot
                        // produce a valid program; discard it rather than poisoning the
                        // next group.
                        self.dropped_bytes = self.dropped_bytes.saturating_add(frame.len() as u64);
                    } else {
                        self.pending_eac3_group.extend_from_slice(&frame);
                    }
                }
                eac3::bsi::StreamType::Reserved => {
                    self.dropped_bytes = self.dropped_bytes.saturating_add(frame.len() as u64);
                }
            }
        }
        if flushing && !self.pending_eac3_group.is_empty() {
            out.push(std::mem::take(&mut self.pending_eac3_group));
        }
        out
    }

    fn resync(&mut self) -> bool {
        if self.buffer.len() < 2 {
            return false;
        }
        if self.buffer[0] == 0x0B && self.buffer[1] == 0x77 {
            return true;
        }
        if let Some(pos) = self.buffer.windows(2).position(|w| w == [0x0B, 0x77]) {
            if pos > 0 {
                self.buffer.drain(..pos);
                self.dropped_bytes = self.dropped_bytes.saturating_add(pos as u64);
            }
            true
        } else {
            let keep = usize::from(self.buffer.last() == Some(&0x0B));
            let drop = self.buffer.len().saturating_sub(keep);
            self.buffer.drain(..drop);
            self.dropped_bytes = self.dropped_bytes.saturating_add(drop as u64);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_split_syncword_for_next_push() {
        let mut f = SyncFramer::new(CodecKind::Ac3);
        assert!(f.push(&[1, 2, 3, 0x0B]).is_empty());
        assert_eq!(f.buffer, vec![0x0B]);
    }
}
