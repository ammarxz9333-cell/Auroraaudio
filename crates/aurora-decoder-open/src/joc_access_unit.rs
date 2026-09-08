//! Bounded streaming JOC access-unit assembler.
//!
//! Adapted from the Apache-2.0 OpenJOC streaming framer contract. Aurora owns
//! the buffering/lifecycle while `openjoc-eac3` supplies the normative access-
//! unit boundary parser.

use aurora_decoder_api::DecoderError;
use openjoc_eac3::{AccessUnitParse, GENERAL_MAX_ACCESS_UNIT_BYTES, parse_access_unit_bounds};

const MAX_PENDING_INPUT_BYTES: usize = GENERAL_MAX_ACCESS_UNIT_BYTES * 2;

#[derive(Debug, Default)]
pub struct JocAccessUnitAssembler {
    pending: Vec<u8>,
}

impl JocAccessUnitAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn staged_bytes(&self) -> usize {
        self.pending.len()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, DecoderError> {
        let total = self
            .pending
            .len()
            .checked_add(bytes.len())
            .ok_or(DecoderError::UnsupportedInput("JOC staging size overflow"))?;
        if total > MAX_PENDING_INPUT_BYTES {
            return Err(DecoderError::UnsupportedInput(
                "JOC staging exceeded two maximum access units",
            ));
        }
        self.pending
            .try_reserve(bytes.len())
            .map_err(|_| DecoderError::Unavailable("JOC staging allocation failed"))?;
        self.pending.extend_from_slice(bytes);

        let mut units = Vec::new();
        loop {
            match parse_access_unit_bounds(&self.pending, false)
                .map_err(|e| DecoderError::ExternalProcess(format!("JOC AU framing failed: {e}")))?
            {
                AccessUnitParse::NeedMore => break,
                AccessUnitParse::Complete(length) => {
                    if length == 0 || length > self.pending.len() {
                        return Err(DecoderError::UnsupportedInput(
                            "JOC access-unit parser returned invalid length",
                        ));
                    }
                    units.push(self.pending.drain(..length).collect());
                }
            }
        }
        Ok(units)
    }

    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, DecoderError> {
        let mut units = Vec::new();
        loop {
            if self.pending.is_empty() {
                break;
            }
            match parse_access_unit_bounds(&self.pending, true)
                .map_err(|e| DecoderError::ExternalProcess(format!("JOC AU final framing failed: {e}")))?
            {
                AccessUnitParse::NeedMore => {
                    return Err(DecoderError::UnsupportedInput(
                        "truncated E-AC-3/JOC access unit at end of stream",
                    ));
                }
                AccessUnitParse::Complete(length) => {
                    if length == 0 || length > self.pending.len() {
                        return Err(DecoderError::UnsupportedInput(
                            "JOC access-unit parser returned invalid final length",
                        ));
                    }
                    units.push(self.pending.drain(..length).collect());
                }
            }
        }
        Ok(units)
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stream_stages_nothing() {
        let mut assembler = JocAccessUnitAssembler::new();
        assert!(assembler.push(&[]).unwrap().is_empty());
        assert_eq!(assembler.staged_bytes(), 0);
    }
}
