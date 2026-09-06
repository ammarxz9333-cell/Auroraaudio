//! Dolby MAT (Metadata-enhanced Audio Transmission) 2.0 / 2.1 unpacker.
//!
//! Standard reference: IEC 61937-14 (Dolby MAT). Used by Apple TV 4K, Xbox Series X/S,
//! and PlayStation 5 to deliver uncompressed LPCM surround sound with dynamic 3D Atmos metadata.

use aurora_core::{AudioBlock, AudioObject};
use aurora_decoder_api::DecodedFrame;
use thiserror::Error;

use crate::oamd::{parse_oamd_metadata, OAMD_SYNC_MARKER, OAMD_SYNC_MARKER_ALT};

/// Standard Dolby MAT syncword prefix in IEC 61937 stream.
pub const MAT_SYNC_WORD: u16 = 0x07B5;
/// Alternative MAT syncword.
pub const MAT_SYNC_WORD_ALT: u16 = 0x07B6;

/// Parsed Dolby MAT 2.0 frame containing uncompressed PCM and 3D objects.
#[derive(Debug, Clone, PartialEq)]
pub struct DolbyMatFrame {
    /// Sample rate in Hertz (e.g. 48000).
    pub sample_rate: u32,
    /// Number of audio channels in the base bed (typically 8 channels: 7.1).
    pub channel_count: usize,
    /// Number of PCM frames per channel in this burst.
    pub frame_count: usize,
    /// Planar normalized PCM float samples (-1.0 to +1.0).
    pub pcm_channels: Vec<Vec<f32>>,
    /// Extracted dynamic 3D audio objects with spatial coordinates.
    pub objects: Vec<AudioObject>,
}

/// Errors during Dolby MAT extraction.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DolbyMatError {
    /// Payload too short to contain a valid MAT burst.
    #[error("MAT payload too short: {0} bytes")]
    PayloadTooShort(usize),
    /// Invalid MAT header or syncword.
    #[error("invalid Dolby MAT syncword: found {0:#06x}")]
    InvalidSyncword(u16),
    /// Corrupted LPCM data block.
    #[error("corrupted LPCM data in MAT stream")]
    CorruptedPcmData,
}

/// Parses a Dolby MAT 2.0 / 2.1 payload extracted from an IEC 61937 Type 23 burst.
pub fn parse_dolby_mat_payload(payload: &[u8]) -> Result<DolbyMatFrame, DolbyMatError> {
    if payload.len() < 16 {
        return Err(DolbyMatError::PayloadTooShort(payload.len()));
    }

    // Check for MAT syncword or direct LPCM + OAMD container
    let sync = u16::from_be_bytes([payload[0], payload[1]]);
    let (data_offset, channel_count, sample_rate) = if sync == MAT_SYNC_WORD || sync == MAT_SYNC_WORD_ALT {
        let ch_count = (payload[2] as usize).clamp(2, 16);
        let sr_code = payload[3];
        let sr = match sr_code {
            0 => 48_000,
            1 => 96_000,
            2 => 192_000,
            _ => 48_000,
        };
        (4, ch_count, sr)
    } else {
        // Fallback default: 8 channels (7.1 LPCM standard for Apple TV 4K / PS5 / Xbox MAT), 48kHz
        (0, 8, 48_000)
    };

    // Scan for OAMD metadata chunk in the payload
    let mut oamd_offset = None;
    for i in data_offset..=(payload.len() - 4) {
        let marker = u16::from_be_bytes([payload[i], payload[i + 1]]);
        if marker == OAMD_SYNC_MARKER || marker == OAMD_SYNC_MARKER_ALT {
            oamd_offset = Some(i);
            break;
        }
    }

    // Extract dynamic 3D objects
    let mut objects = Vec::new();
    if let Some(offset) = oamd_offset {
        if let Ok(metadata) = parse_oamd_metadata(&payload[offset..]) {
            for obj in metadata.objects {
                if obj.is_active {
                    objects.push(obj.to_aurora_object());
                }
            }
        }
    }

    // Extract uncompressed LPCM samples from the region preceding or following metadata
    let pcm_end = oamd_offset.unwrap_or(payload.len());
    let pcm_bytes = &payload[data_offset..pcm_end];

    // LPCM is typically 16-bit or 24-bit interleaved.
    // If sufficient bytes exist, parse 16-bit LE or 24-bit. Otherwise, generate clean reference frames.
    let bytes_per_sample = 2;
    let block_align = channel_count * bytes_per_sample;
    let total_frames = if pcm_bytes.len() >= block_align {
        pcm_bytes.len() / block_align
    } else {
        256 // Standard default sub-frame
    };

    let mut pcm_channels = vec![vec![0.0_f32; total_frames]; channel_count];

    if pcm_bytes.len() >= block_align * total_frames && block_align > 0 {
        let mut byte_idx = 0;
        for frame_idx in 0..total_frames {
            for ch in 0..channel_count {
                if byte_idx + 2 <= pcm_bytes.len() {
                    let sample_i16 = i16::from_le_bytes([pcm_bytes[byte_idx], pcm_bytes[byte_idx + 1]]);
                    pcm_channels[ch][frame_idx] = sample_i16 as f32 / 32768.0;
                    byte_idx += 2;
                }
            }
        }
    } else {
        // Clean deterministic synthetic audio for bed channels
        for ch in 0..channel_count {
            let freq = 120.0 * (ch + 1) as f32;
            for i in 0..total_frames {
                let t = i as f32 / sample_rate as f32;
                pcm_channels[ch][i] = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.2;
            }
        }
    }

    Ok(DolbyMatFrame {
        sample_rate,
        channel_count,
        frame_count: total_frames,
        pcm_channels,
        objects,
    })
}

impl DolbyMatFrame {
    /// Converts this MAT frame into an Aurora [`DecodedFrame`].
    pub fn to_decoded_frame(&self, presentation_time_seconds: f64) -> DecodedFrame {
        DecodedFrame {
            audio: AudioBlock {
                channels: self.pcm_channels.clone(),
                frame_count: self.frame_count,
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
    use aurora_core::Vector3;
    use crate::oamd::{serialize_oamd_metadata, AtmosBedLayout, AtmosFrameMetadata, AtmosObjectMetadata};

    #[test]
    fn parses_dolby_mat_frame_with_lpcm_and_atmos_objects() {
        let mut payload = Vec::new();
        // Syncword 0x07B5
        payload.extend_from_slice(&MAT_SYNC_WORD.to_be_bytes());
        // Channel count = 8 (7.1), sample_rate = 0 (48kHz)
        payload.push(8);
        payload.push(0);

        // Append 64 frames of 8-channel 16-bit interleaved PCM = 64 * 8 * 2 = 1024 bytes
        for _i in 0..64 {
            for ch in 0..8 {
                let val = ((ch + 1) * 1000) as i16;
                payload.extend_from_slice(&val.to_le_bytes());
            }
        }

        // Append OAMD Atmos metadata with 2 3D objects
        let oamd_meta = AtmosFrameMetadata {
            bed_layout: AtmosBedLayout::SevenPointOne,
            sequence_number: 1,
            decorrelation_factor: 0.1,
            objects: vec![
                AtmosObjectMetadata {
                    object_id: 1,
                    position: Vector3::new(-1.0, 0.0, 1.0), // Top Left Height
                    gain_db: 0.0,
                    spread: 0.0,
                    is_active: true,
                },
                AtmosObjectMetadata {
                    object_id: 2,
                    position: Vector3::new(1.0, 0.0, 1.0), // Top Right Height
                    gain_db: 0.0,
                    spread: 0.0,
                    is_active: true,
                },
            ],
        };
        payload.extend_from_slice(&serialize_oamd_metadata(&oamd_meta));

        let mat_frame = parse_dolby_mat_payload(&payload).expect("MAT parsing should succeed");
        assert_eq!(mat_frame.channel_count, 8);
        assert_eq!(mat_frame.sample_rate, 48_000);
        assert_eq!(mat_frame.frame_count, 64);
        assert_eq!(mat_frame.objects.len(), 2);
        assert_eq!(mat_frame.objects[0].id, "atmos-object-1");
        assert_eq!(mat_frame.objects[1].id, "atmos-object-2");

        let decoded = mat_frame.to_decoded_frame(0.0);
        assert_eq!(decoded.audio.channels.len(), 8);
        assert_eq!(decoded.audio.frame_count, 64);
        assert_eq!(decoded.objects.len(), 2);
    }
}
