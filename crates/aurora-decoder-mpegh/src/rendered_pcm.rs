use thiserror::Error;

use crate::MpeghSpeakerLayout;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghRenderedPcm {
    /// Interleaved little-endian speaker-rendered PCM from the same libmpegh
    /// execute call that produced Aurora's external pre-render scene.
    pub bytes: Vec<u8>,
    pub bit_depth: u8,
    pub channel_count: usize,
    pub frame_count: usize,
    pub sample_rate: u32,
    /// Exact speaker layout reported by libmpegh for this render. Native
    /// captures populate this field from the same execute call. `None` is kept
    /// only for synthetic/baseline fixtures that have no native geometry.
    pub speaker_layout: Option<MpeghSpeakerLayout>,
}

impl MpeghRenderedPcm {
    pub fn validate(&self) -> Result<(), MpeghRenderedPcmError> {
        if self.sample_rate == 0 {
            return Err(MpeghRenderedPcmError::InvalidSampleRate);
        }
        if self.channel_count == 0 {
            return Err(MpeghRenderedPcmError::InvalidChannelCount);
        }
        if let Some(layout) = &self.speaker_layout {
            if !layout.speakers.is_empty() && layout.speakers.len() != self.channel_count {
                return Err(MpeghRenderedPcmError::SpeakerLayoutChannelMismatch {
                    speakers: layout.speakers.len(),
                    channels: self.channel_count,
                });
            }
        }
        let bytes_per_sample = bytes_per_sample(self.bit_depth)?;
        let expected = self
            .frame_count
            .checked_mul(self.channel_count)
            .and_then(|value| value.checked_mul(bytes_per_sample))
            .ok_or(MpeghRenderedPcmError::NumericOverflow)?;
        if expected != self.bytes.len() {
            return Err(MpeghRenderedPcmError::ByteLengthMismatch {
                bytes: self.bytes.len(),
                expected,
            });
        }
        Ok(())
    }

    /// Decode the reference/fallback speaker output to planar F32. This is a
    /// conformance/playback companion, not Aurora's pre-render spatial scene.
    pub fn decode_planar_f32(&self) -> Result<Vec<Vec<f32>>, MpeghRenderedPcmError> {
        self.validate()?;
        let bytes_per_sample = bytes_per_sample(self.bit_depth)?;
        let mut channels = (0..self.channel_count)
            .map(|_| Vec::with_capacity(self.frame_count))
            .collect::<Vec<_>>();
        for frame in self
            .bytes
            .chunks_exact(self.channel_count * bytes_per_sample)
        {
            for (channel_index, destination) in channels.iter_mut().enumerate() {
                let offset = channel_index * bytes_per_sample;
                let sample = match self.bit_depth {
                    16 => {
                        let value = i16::from_le_bytes([frame[offset], frame[offset + 1]]);
                        f32::from(value) / 32_768.0
                    }
                    24 => {
                        let raw = i32::from(frame[offset])
                            | (i32::from(frame[offset + 1]) << 8)
                            | (i32::from(frame[offset + 2]) << 16);
                        let value = if raw & 0x0080_0000 != 0 {
                            raw | !0x00ff_ffff
                        } else {
                            raw
                        };
                        value as f32 / 8_388_608.0
                    }
                    32 => {
                        let value = i32::from_le_bytes([
                            frame[offset],
                            frame[offset + 1],
                            frame[offset + 2],
                            frame[offset + 3],
                        ]);
                        value as f32 / 2_147_483_648.0
                    }
                    other => return Err(MpeghRenderedPcmError::UnsupportedBitDepth(other)),
                };
                destination.push(sample);
            }
        }
        Ok(channels)
    }
}

fn bytes_per_sample(bit_depth: u8) -> Result<usize, MpeghRenderedPcmError> {
    match bit_depth {
        16 => Ok(2),
        24 => Ok(3),
        32 => Ok(4),
        other => Err(MpeghRenderedPcmError::UnsupportedBitDepth(other)),
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghRenderedPcmError {
    #[error("rendered MPEG-H PCM bit depth {0} is unsupported")]
    UnsupportedBitDepth(u8),
    #[error("rendered MPEG-H PCM sample rate is zero")]
    InvalidSampleRate,
    #[error("rendered MPEG-H PCM channel count is zero")]
    InvalidChannelCount,
    #[error("rendered MPEG-H speaker layout has {speakers} speakers for {channels} PCM channels")]
    SpeakerLayoutChannelMismatch { speakers: usize, channels: usize },
    #[error("rendered MPEG-H PCM stores {bytes} bytes but geometry requires {expected}")]
    ByteLengthMismatch { bytes: usize, expected: usize },
    #[error("rendered MPEG-H PCM geometry arithmetic overflow")]
    NumericOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_interleaved_s16_reference_pcm() {
        let pcm = MpeghRenderedPcm {
            bytes: vec![0x00, 0x40, 0x00, 0xc0],
            bit_depth: 16,
            channel_count: 2,
            frame_count: 1,
            sample_rate: 48_000,
            speaker_layout: None,
        };
        let planar = pcm.decode_planar_f32().unwrap();
        assert!((planar[0][0] - 0.5).abs() < 1.0e-6);
        assert!((planar[1][0] + 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn rejects_misaligned_geometry() {
        let pcm = MpeghRenderedPcm {
            bytes: vec![0; 3],
            bit_depth: 16,
            channel_count: 2,
            frame_count: 1,
            sample_rate: 48_000,
            speaker_layout: None,
        };
        assert!(matches!(
            pcm.validate(),
            Err(MpeghRenderedPcmError::ByteLengthMismatch { .. })
        ));
    }
}
