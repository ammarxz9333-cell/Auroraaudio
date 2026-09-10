//! Streaming syncframe extraction for native AC-3 / E-AC-3 decode.

use crate::sniff::CodecKind;
use oxideav_ac3::{eac3, syncinfo};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FramingError {
    #[error("truncated AC-3 syncword at EOF: {available} byte buffered")]
    TruncatedAc3Syncword { available: usize },
    #[error("truncated AC-3 syncframe header at EOF: {available} bytes buffered, need at least 5")]
    TruncatedAc3Header { available: usize },
    #[error(
        "truncated AC-3 syncframe at EOF: expected {expected} bytes, only {available} buffered"
    )]
    TruncatedAc3Frame { expected: usize, available: usize },
}

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

    /// Finalize a finite elementary stream.
    ///
    /// Streaming extraction deliberately keeps an incomplete AC-3 syncframe in
    /// the buffer because a later `push` may complete it. At a known EOF there
    /// can be no later bytes, so a syncword/header/payload prefix must be
    /// reported instead of silently disappearing. Non-sync garbage remains a
    /// resynchronization concern and is counted as dropped data rather than a
    /// truncated frame.
    pub fn finish_checked(&mut self) -> Result<Vec<Vec<u8>>, FramingError> {
        let out = self.flush();
        if self.codec == CodecKind::Ac3 {
            self.validate_ac3_eof()?;
        }
        Ok(out)
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.pending_eac3_group.clear();
        self.dropped_bytes = 0;
    }

    fn validate_ac3_eof(&mut self) -> Result<(), FramingError> {
        if self.buffer.is_empty() {
            return Ok(());
        }

        // A single non-sync byte can survive the streaming resynchronizer because
        // it intentionally waits for a possible second syncword byte. At EOF it
        // is just garbage and can be accounted for immediately.
        if self.buffer[0] != 0x0B {
            let dropped = self.buffer.len();
            self.buffer.clear();
            self.dropped_bytes = self.dropped_bytes.saturating_add(dropped as u64);
            return Ok(());
        }

        if self.buffer.len() == 1 {
            return Err(FramingError::TruncatedAc3Syncword { available: 1 });
        }

        if self.buffer[1] != 0x77 {
            let dropped = self.buffer.len();
            self.buffer.clear();
            self.dropped_bytes = self.dropped_bytes.saturating_add(dropped as u64);
            return Ok(());
        }

        if self.buffer.len() < 5 {
            return Err(FramingError::TruncatedAc3Header {
                available: self.buffer.len(),
            });
        }

        match syncinfo::parse(&self.buffer) {
            Ok(info) => {
                let expected = info.frame_length as usize;
                if self.buffer.len() < expected {
                    Err(FramingError::TruncatedAc3Frame {
                        expected,
                        available: self.buffer.len(),
                    })
                } else {
                    // `flush` already emits complete frames; reaching this branch
                    // only means the parser's invariants changed underneath us.
                    Ok(())
                }
            }
            Err(_) => {
                // Malformed sync-looking bytes are corruption/garbage rather than
                // a provably truncated frame. Preserve streaming resync semantics.
                let dropped = self.buffer.len();
                self.buffer.clear();
                self.dropped_bytes = self.dropped_bytes.saturating_add(dropped as u64);
                Ok(())
            }
        }
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

    fn minimal_ac3_frame() -> Vec<u8> {
        // 48 kHz (`fscod = 0`), `frmsizecod = 0` => 64 words / 128 bytes.
        // The framer only needs syncinfo; CRC/audio-block contents are irrelevant.
        let mut frame = vec![0_u8; 128];
        frame[0] = 0x0B;
        frame[1] = 0x77;
        frame[4] = 0;
        frame
    }

    #[test]
    fn preserves_split_syncword_for_next_push() {
        let mut f = SyncFramer::new(CodecKind::Ac3);
        assert!(f.push(&[1, 2, 3, 0x0B]).is_empty());
        assert_eq!(f.buffer, vec![0x0B]);
    }

    #[test]
    fn finite_ac3_eof_accepts_complete_final_frame() {
        let frame = minimal_ac3_frame();
        let mut f = SyncFramer::new(CodecKind::Ac3);
        assert_eq!(f.push(&frame), vec![frame]);
        assert!(f.finish_checked().unwrap().is_empty());
    }

    #[test]
    fn finite_ac3_eof_rejects_truncated_header() {
        let mut f = SyncFramer::new(CodecKind::Ac3);
        assert!(f.push(&[0x0B, 0x77, 0, 0]).is_empty());
        assert_eq!(
            f.finish_checked(),
            Err(FramingError::TruncatedAc3Header { available: 4 })
        );
    }

    #[test]
    fn finite_ac3_eof_rejects_truncated_payload() {
        let frame = minimal_ac3_frame();
        let partial = &frame[..20];
        let mut f = SyncFramer::new(CodecKind::Ac3);
        assert!(f.push(partial).is_empty());
        assert_eq!(
            f.finish_checked(),
            Err(FramingError::TruncatedAc3Frame {
                expected: 128,
                available: 20,
            })
        );
    }

    #[test]
    fn finite_ac3_eof_drops_non_sync_garbage_tail() {
        let frame = minimal_ac3_frame();
        let mut input = frame.clone();
        input.push(0xAA);
        let mut f = SyncFramer::new(CodecKind::Ac3);
        assert_eq!(f.push(&input), vec![frame]);
        assert!(f.finish_checked().unwrap().is_empty());
        assert_eq!(f.dropped_bytes(), 1);
    }
}
