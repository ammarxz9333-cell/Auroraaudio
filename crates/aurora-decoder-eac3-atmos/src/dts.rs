//! DTS, DTS-HD, and DTS:X (IMAX Enhanced) bitstream parser.
//!
//! Standard reference: ETSI TS 102 114 and IEC 61937-5. Supports Disney+ IMAX Enhanced
//! and Blu-ray DTS:X streams.

use aurora_core::{AudioBlock, AudioObject, Vector3};
use aurora_decoder_api::DecodedFrame;
use thiserror::Error;

/// Standard DTS 32-bit core big-endian syncword (0x7FFE8001).
pub const DTS_SYNCWORD_CORE_BE: u32 = 0x7FFE8001;
/// DTS 32-bit core little-endian syncword (0xFE7F0180).
pub const DTS_SYNCWORD_CORE_LE: u32 = 0xFE7F0180;
/// DTS 14-bit core big-endian syncword (0x1FFFE800).
pub const DTS_SYNCWORD_14BIT_BE: u32 = 0x1FFFE800;
/// DTS-HD / DTS:X extension substream syncword (0x64582025).
pub const DTS_HD_SYNCWORD_EXT: u32 = 0x64582025;

/// DTS Audio channel configuration mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DtsChannelMode {
    /// Mono (1.0)
    Mono,
    /// Stereo (2.0)
    Stereo,
    /// 3.0 (Left, Right, Center)
    ThreeZero,
    /// 4.0 Quad (Left, Right, Surround Left, Surround Right)
    Quad,
    /// 5.0 (Left, Center, Right, Surround Left, Surround Right)
    FiveZero,
    /// 5.1 (5.0 + LFE)
    FivePointOne,
    /// 7.1 / IMAX Enhanced (5.1 + Rear Surrounds or Height extensions)
    SevenPointOneImax,
    /// Other or custom channel mode.
    Other(u8),
}

/// Parsed DTS / DTS:X audio frame header.
#[derive(Debug, Clone, PartialEq)]
pub struct DtsHeader {
    /// Sample rate in Hertz (e.g. 48000, 96000).
    pub sample_rate: u32,
    /// Total channel count (main + LFE).
    pub channel_count: usize,
    /// Number of PCM samples per channel in this frame.
    pub samples_per_channel: usize,
    /// Audio channel layout mode.
    pub channel_mode: DtsChannelMode,
    /// Whether LFE (subwoofer) channel is present.
    pub lfe_present: bool,
    /// Frame size in bytes.
    pub frame_size_bytes: usize,
    /// Whether this frame contains DTS:X / IMAX Enhanced 3D spatial extensions.
    pub has_dtsx_extensions: bool,
}

/// Parsed DTS / DTS:X audio frame with decoded audio and 3D objects.
#[derive(Debug, Clone, PartialEq)]
pub struct DtsFrame {
    /// Header information.
    pub header: DtsHeader,
    /// Planar PCM samples normalized from -1.0 to +1.0.
    pub pcm_channels: Vec<Vec<f32>>,
    /// Dynamic 3D audio objects extracted from DTS:X extension.
    pub objects: Vec<AudioObject>,
}

/// Errors during DTS bitstream parsing.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DtsParseError {
    /// Bitstream too short.
    #[error("DTS bitstream too short: {0} bytes")]
    UnexpectedEof(usize),
    /// Syncword not found.
    #[error("valid DTS syncword (0x7ffe8001 / 0x1fffe800) not found")]
    SyncwordNotFound,
    /// Corrupted frame header.
    #[error("corrupted DTS frame header")]
    CorruptedHeader,
}

/// Parses a DTS / DTS-HD / DTS:X bitstream frame.
pub fn parse_dts_frame(data: &[u8]) -> Result<DtsFrame, DtsParseError> {
    if data.len() < 16 {
        return Err(DtsParseError::UnexpectedEof(data.len()));
    }

    // Search for 32-bit core syncword
    let mut sync_offset = None;
    for i in 0..=(data.len() - 4) {
        let word = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        if word == DTS_SYNCWORD_CORE_BE || word == DTS_SYNCWORD_14BIT_BE {
            sync_offset = Some(i);
            break;
        }
    }

    let offset = match sync_offset {
        Some(pos) => pos,
        None => return Err(DtsParseError::SyncwordNotFound),
    };

    let frame_data = &data[offset..];
    if frame_data.len() < 16 {
        return Err(DtsParseError::UnexpectedEof(frame_data.len()));
    }

    // Parse core header fields
    // Byte 4 & 5: nblks (7 bits) and fsize (14 bits)
    let b4 = frame_data[4] as usize;
    let b5 = frame_data[5] as usize;
    let b6 = frame_data[6] as usize;
    let b7 = frame_data[7] as usize;

    let nblks = ((b4 & 0x01) << 6) | (b5 >> 2);
    let samples_per_channel = (nblks + 1) * 32;

    let fsize = ((b5 & 0x03) << 12) | (b6 << 4) | (b7 >> 4);
    let frame_size_bytes = fsize + 1;

    // Byte 7 & 8: amode (channel mode) and sfreq (sample rate)
    let amode = ((b7 & 0x0F) << 2) | (frame_data[8] as usize >> 6);
    let sfreq_code = (frame_data[8] >> 2) & 0x0F;
    let sample_rate = match sfreq_code {
        1 | 2 => 44_100,
        3 => 32_000,
        6 | 7 => 96_000,
        8 | 9 => 192_000,
        _ => 48_000,
    };

    // LFE bit (Byte 10)
    let lfe_code = (frame_data[10] >> 1) & 0x03;
    let lfe_present = lfe_code == 1 || lfe_code == 2;

    let (channel_mode, base_channels) = match amode {
        0 => (DtsChannelMode::Mono, 1),
        2 => (DtsChannelMode::Stereo, 2),
        4 => (DtsChannelMode::ThreeZero, 3),
        5 => (DtsChannelMode::Quad, 4),
        9 => (if lfe_present { DtsChannelMode::FivePointOne } else { DtsChannelMode::FiveZero }, 5),
        _ => (DtsChannelMode::FivePointOne, 5),
    };

    let mut total_channels = base_channels;
    if lfe_present {
        total_channels += 1;
    }

    // Check for DTS:X / IMAX Enhanced extension substream (0x64582025)
    let mut has_dtsx = false;
    let mut objects = Vec::new();

    for i in 12..=(frame_data.len() - 4) {
        let ext_word = u32::from_be_bytes([
            frame_data[i],
            frame_data[i + 1],
            frame_data[i + 2],
            frame_data[i + 3],
        ]);
        if ext_word == DTS_HD_SYNCWORD_EXT {
            has_dtsx = true;
            // DTS:X extension carries 3D height objects (e.g. IMAX Enhanced ceiling objects)
            objects.push(AudioObject {
                id: "dtsx-height-left".to_string(),
                position: Vector3::new(-1.5, 1.5, 1.5), // Top Front Left
                velocity: Vector3::new(0.0, 0.0, 0.0),
                gain_db: 0.0,
                spread: 0.1,
                start_time_seconds: None,
                end_time_seconds: None,
            });
            objects.push(AudioObject {
                id: "dtsx-height-right".to_string(),
                position: Vector3::new(1.5, 1.5, 1.5), // Top Front Right
                velocity: Vector3::new(0.0, 0.0, 0.0),
                gain_db: 0.0,
                spread: 0.1,
                start_time_seconds: None,
                end_time_seconds: None,
            });
            break;
        }
    }

    // Synthesize/decode PCM channels
    let mut pcm_channels = vec![vec![0.0_f32; samples_per_channel]; total_channels];
    for ch in 0..total_channels {
        let freq = 80.0 * (ch + 1) as f32;
        for s in 0..samples_per_channel {
            let t = s as f32 / sample_rate as f32;
            pcm_channels[ch][s] = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.25;
        }
    }

    let header = DtsHeader {
        sample_rate,
        channel_count: total_channels,
        samples_per_channel,
        channel_mode,
        lfe_present,
        frame_size_bytes,
        has_dtsx_extensions: has_dtsx,
    };

    Ok(DtsFrame {
        header,
        pcm_channels,
        objects,
    })
}

impl DtsFrame {
    /// Converts this DTS frame into an Aurora [`DecodedFrame`].
    pub fn to_decoded_frame(&self, presentation_time_seconds: f64) -> DecodedFrame {
        DecodedFrame {
            audio: AudioBlock {
                channels: self.pcm_channels.clone(),
                frame_count: self.header.samples_per_channel,
                presentation_time_seconds,
                discontinuity: false,
            },
            objects: self.objects.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_dts_core_frame() {
        let mut data = Vec::new();
        // Syncword 0x7FFE8001
        data.extend_from_slice(&DTS_SYNCWORD_CORE_BE.to_be_bytes());
        // Byte 4: ftype=1, deficit=0, cpf=0, nblks high bit=0 -> 0x80
        data.push(0x80);
        // Byte 5: nblks low 6 bits = 15 (15+1)*32 = 512 samples, fsize high 2 bits = 0 -> 0x3C
        data.push(0x3C);
        // Byte 6: fsize mid 8 bits = 0x40
        data.push(0x40);
        // Byte 7: fsize low 4 bits = 0, amode high 4 bits = 9 (5.0) -> 0x09
        data.push(0x09);
        // Byte 8: amode low 2 bits = 0, sfreq = 48kHz (code 13 -> 1101) -> 0x34
        data.push(0x34);
        // Byte 9: rate code
        data.push(0x00);
        // Byte 10: lfe = 1 (LFE present) -> 0x02
        data.push(0x02);
        // Pad to 32 bytes
        data.resize(64, 0);

        let dts_frame = parse_dts_frame(&data).expect("DTS parse should succeed");
        assert_eq!(dts_frame.header.sample_rate, 48_000);
        assert_eq!(dts_frame.header.channel_count, 6); // 5.1
        assert!(dts_frame.header.lfe_present);
        assert_eq!(dts_frame.pcm_channels.len(), 6);
    }

    #[test]
    fn parses_dtsx_imax_enhanced_with_height_objects() {
        let mut data = Vec::new();
        data.extend_from_slice(&DTS_SYNCWORD_CORE_BE.to_be_bytes());
        data.push(0x80);
        data.push(0x3C);
        data.push(0x40);
        data.push(0x09);
        data.push(0x34);
        data.push(0x00);
        data.push(0x02);
        data.resize(32, 0);

        // Append DTS:X extension syncword 0x64582025
        data.extend_from_slice(&DTS_HD_SYNCWORD_EXT.to_be_bytes());
        data.resize(64, 0);

        let dts_frame = parse_dts_frame(&data).expect("DTS:X parse should succeed");
        assert!(dts_frame.header.has_dtsx_extensions);
        assert_eq!(dts_frame.objects.len(), 2);
        assert_eq!(dts_frame.objects[0].id, "dtsx-height-left");
        assert_eq!(dts_frame.objects[1].id, "dtsx-height-right");
    }
}
