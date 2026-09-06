//! DTS family adapter boundary.
//!
//! Sync words may be recognized for routing, but Aurora does not bundle a
//! DTS/DTS-HD/DTS:X decoder. Input is rejected without fabricated PCM.

use aurora_core::{AudioBlock, AudioObject};
use aurora_decoder_api::DecodedFrame;
use thiserror::Error;

pub const DTS_SYNCWORD_CORE_BE: u32 = 0x7FFE8001;
pub const DTS_SYNCWORD_CORE_LE: u32 = 0xFE7F0180;
pub const DTS_SYNCWORD_14BIT_BE: u32 = 0x1FFFE800;
pub const DTS_HD_SYNCWORD_EXT: u32 = 0x64582025;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DtsChannelMode { Mono, Stereo, ThreeZero, Quad, FiveZero, FivePointOne, SevenPointOneImax, Other(u8) }

#[derive(Debug, Clone, PartialEq)]
pub struct DtsHeader {
    pub sample_rate: u32,
    pub channel_count: usize,
    pub samples_per_channel: usize,
    pub channel_mode: DtsChannelMode,
    pub lfe_present: bool,
    pub frame_size_bytes: usize,
    pub has_dtsx_extensions: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DtsFrame {
    pub header: DtsHeader,
    pub pcm_channels: Vec<Vec<f32>>,
    pub objects: Vec<AudioObject>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DtsParseError {
    #[error("DTS bitstream too short: {0} bytes")]
    UnexpectedEof(usize),
    #[error("valid DTS syncword not found")]
    SyncwordNotFound,
    #[error("DTS decoding is unavailable; an external reviewed decoder is required")]
    DecoderUnavailable,
}

pub fn parse_dts_frame(data: &[u8]) -> Result<DtsFrame, DtsParseError> {
    if data.len() < 4 { return Err(DtsParseError::UnexpectedEof(data.len())); }
    let sync = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    if sync != DTS_SYNCWORD_CORE_BE && sync != DTS_SYNCWORD_CORE_LE && sync != DTS_SYNCWORD_14BIT_BE {
        return Err(DtsParseError::SyncwordNotFound);
    }
    Err(DtsParseError::DecoderUnavailable)
}

impl DtsFrame {
    pub fn to_decoded_frame(&self, presentation_time_seconds: f64) -> DecodedFrame {
        DecodedFrame {
            audio: AudioBlock { channels: self.pcm_channels.clone(), frame_count: self.header.samples_per_channel, presentation_time_seconds, discontinuity: false },
            objects: self.objects.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognized_dts_is_not_replaced_with_synthetic_audio() {
        assert_eq!(parse_dts_frame(&DTS_SYNCWORD_CORE_BE.to_be_bytes()), Err(DtsParseError::DecoderUnavailable));
    }
}
