//! Bounded streaming JOC access-unit assembler.
//!
//! Adapted from the Apache-2.0 OpenJOC streaming framer contract. Aurora owns
//! the buffering/lifecycle while `openjoc-eac3` supplies the normative access-
//! unit boundary parser.

use aurora_decoder_api::{DecodedFrame, DecoderError};
use openjoc_eac3::{AccessUnitParse, GENERAL_MAX_ACCESS_UNIT_BYTES, parse_access_unit_bounds};

use crate::sniff::{CodecKind, Encapsulation};
use crate::{Transport, UniversalOpenDecoder};

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

impl UniversalOpenDecoder {
    /// Decode one E-AC-3 access unit whose end boundary was already established
    /// by a higher-level transport (for example one validated IEC61937 type
    /// 0x15 data-burst). The generic byte-stream front door intentionally keeps
    /// its look-ahead framing semantics; only a transport-proven boundary may
    /// use this path.
    ///
    /// The OpenJOC parser still validates the payload with EOS semantics and
    /// Aurora requires it to describe exactly one complete AU. This prevents a
    /// caller from using the optimization to smuggle a partial AU or trailing
    /// bytes past the normal streaming framer.
    pub fn decode_complete_eac3_access_unit(
        &mut self,
        unit: &[u8],
    ) -> Result<Option<DecodedFrame>, DecoderError> {
        if unit.is_empty() {
            return Err(DecoderError::UnsupportedInput(
                "transport-bounded E-AC-3 access unit must not be empty",
            ));
        }
        if !self.pending.is_empty() {
            return Err(DecoderError::Decode(
                "transport-bounded E-AC-3 input arrived before prior PCM was drained".to_owned(),
            ));
        }
        if self
            .joc_assembler
            .as_ref()
            .is_some_and(|assembler| assembler.staged_bytes() != 0)
        {
            return Err(DecoderError::Decode(
                "cannot mix transport-bounded E-AC-3 access units with staged byte-stream framing"
                    .to_owned(),
            ));
        }

        let length = match parse_access_unit_bounds(unit, true).map_err(|error| {
            DecoderError::ExternalProcess(format!(
                "transport E-AC-3 AU validation failed: {error}"
            ))
        })? {
            AccessUnitParse::Complete(length) => length,
            AccessUnitParse::NeedMore => {
                return Err(DecoderError::UnsupportedInput(
                    "transport boundary did not contain a complete E-AC-3 access unit",
                ));
            }
        };
        if length == 0 || length != unit.len() {
            return Err(DecoderError::UnsupportedInput(
                "IEC61937 E-AC-3 payload must contain exactly one complete access unit",
            ));
        }

        if !matches!(self.codec, Some(CodecKind::Eac3 | CodecKind::Eac3Joc))
            || !matches!(&self.transport, Transport::Elementary)
        {
            self.codec = Some(CodecKind::Eac3);
            self.encapsulation = Encapsulation::Elementary;
            self.transport = Transport::Elementary;
            self.initialize_backend(CodecKind::Eac3, Encapsulation::Elementary)?;
        }

        self.process_eac3_access_unit(unit)?;
        Ok(self.pop_pending_frame())
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

    #[test]
    fn transport_bounded_front_door_rejects_empty_payload() {
        let mut decoder = UniversalOpenDecoder::new(crate::OpenDecoderConfig::default());
        let error = decoder.decode_complete_eac3_access_unit(&[]).unwrap_err();
        assert!(matches!(error, DecoderError::UnsupportedInput(_)));
    }
}
