//! Dolby MAT adapter boundary.
//!
//! Framing may be detected, but Aurora does not ship a Dolby MAT object
//! metadata decoder. Input is rejected instead of producing synthetic PCM.

use aurora_core::{AudioBlock, AudioObject};
use aurora_decoder_api::DecodedFrame;
use thiserror::Error;

pub const MAT_SYNC_WORD: u16 = 0x07B5;
pub const MAT_SYNC_WORD_ALT: u16 = 0x07B6;

#[derive(Debug, Clone, PartialEq)]
pub struct DolbyMatFrame {
    pub sample_rate: u32,
    pub channel_count: usize,
    pub frame_count: usize,
    pub pcm_channels: Vec<Vec<f32>>,
    pub objects: Vec<AudioObject>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DolbyMatError {
    #[error("MAT payload too short: {0} bytes")]
    PayloadTooShort(usize),
    #[error("invalid Dolby MAT syncword: found {0:#06x}")]
    InvalidSyncword(u16),
    #[error("Dolby MAT decoding is unavailable; an external reviewed decoder is required")]
    DecoderUnavailable,
}

pub fn parse_dolby_mat_payload(payload: &[u8]) -> Result<DolbyMatFrame, DolbyMatError> {
    if payload.len() < 4 { return Err(DolbyMatError::PayloadTooShort(payload.len())); }
    let sync = u16::from_be_bytes([payload[0], payload[1]]);
    if sync != MAT_SYNC_WORD && sync != MAT_SYNC_WORD_ALT {
        return Err(DolbyMatError::InvalidSyncword(sync));
    }
    Err(DolbyMatError::DecoderUnavailable)
}

impl DolbyMatFrame {
    pub fn to_decoded_frame(&self, presentation_time_seconds: f64) -> DecodedFrame {
        DecodedFrame {
            audio: AudioBlock { channels: self.pcm_channels.clone(), frame_count: self.frame_count, presentation_time_seconds, discontinuity: false },
            objects: self.objects.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognized_mat_is_not_replaced_with_synthetic_audio() {
        assert_eq!(parse_dolby_mat_payload(&[0x07, 0xB5, 8, 0]), Err(DolbyMatError::DecoderUnavailable));
    }
}
