//! Seamless Zero-Click Format Auto-Switching and Crossfader.
//!
//! Enables instantaneous, click-free transitions between streaming formats
//! (Stereo PCM, E-AC-3 5.1, Dolby Atmos JOC, Dolby MAT 2.0, DTS:X).

use std::f32::consts::PI;

use aurora_decoder_api::{DecodedFrame, Decoder};

use crate::decoder::Eac3AtmosDecoder;
use crate::dts::{parse_dts_frame, DTS_SYNCWORD_CORE_BE, DTS_SYNCWORD_14BIT_BE};
use crate::eac3::EAC3_SYNCWORD;
use crate::iec61937::{Iec61937DataType, Iec61937Parser, PREAMBLE_PA, PREAMBLE_PA_SWAPPED};
use crate::mat::{parse_dolby_mat_payload, MAT_SYNC_WORD, MAT_SYNC_WORD_ALT};

/// Audio bitstream formats detected automatically on the incoming stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedStreamFormat {
    /// Unknown or silence.
    Unknown,
    /// Raw uncompressed PCM (Stereo or Multichannel).
    Pcm,
    /// Dolby Digital Plus / E-AC-3 with Atmos JOC.
    DolbyAtmosEac3,
    /// Dolby MAT 2.0 / 2.1 (Apple TV 4K, Xbox, PS5 uncompressed LPCM + OAMD).
    DolbyMat,
    /// DTS / DTS-HD / DTS:X (IMAX Enhanced).
    DtsX,
}

/// Statistics and telemetry for auto-switching.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AutoSwitchStats {
    /// Active format currently being decoded.
    pub active_format: Option<DetectedStreamFormat>,
    /// Total number of detected format transitions.
    pub format_transitions: u64,
    /// Total number of zero-click crossfades performed.
    pub crossfades_performed: u64,
}

/// Seamless format auto-switcher with micro-fade smoothing.
pub struct FormatAutoSwitch {
    pub iec_parser: Iec61937Parser,
    eac3_decoder: Eac3AtmosDecoder,
    current_format: DetectedStreamFormat,
    stats: AutoSwitchStats,
    crossfade_buffer: Vec<Vec<f32>>,
    fade_frames: usize,
    pts: f64,
}

impl FormatAutoSwitch {
    /// Creates a new format auto-switcher with specified crossfade duration in frames (default 192 frames ~ 4ms at 48kHz).
    pub fn new(fade_frames: usize) -> Self {
        Self {
            iec_parser: Iec61937Parser::new(),
            eac3_decoder: Eac3AtmosDecoder::new(),
            current_format: DetectedStreamFormat::Unknown,
            stats: AutoSwitchStats::default(),
            crossfade_buffer: Vec::new(),
            fade_frames: fade_frames.max(32),
            pts: 0.0,
        }
    }

    /// Detects stream format from initial bytes without consuming the buffer.
    pub fn inspect_format(bytes: &[u8]) -> DetectedStreamFormat {
        if bytes.len() < 8 {
            return DetectedStreamFormat::Unknown;
        }

        // Check for IEC 61937 preamble
        let be_pa = u16::from_be_bytes([bytes[0], bytes[1]]);
        let le_pa = u16::from_le_bytes([bytes[0], bytes[1]]);
        if be_pa == PREAMBLE_PA || le_pa == PREAMBLE_PA_SWAPPED {
            let pc = if be_pa == PREAMBLE_PA {
                u16::from_be_bytes([bytes[4], bytes[5]])
            } else {
                u16::from_le_bytes([bytes[4], bytes[5]])
            };
            let data_type = (pc & 0x001F) as u8;
            match Iec61937DataType::from(data_type) {
                Iec61937DataType::EnhancedAc3 => return DetectedStreamFormat::DolbyAtmosEac3,
                Iec61937DataType::DolbyMat => return DetectedStreamFormat::DolbyMat,
                Iec61937DataType::Dts | Iec61937DataType::DtsHd => return DetectedStreamFormat::DtsX,
                _ => {}
            }
        }

        // Direct E-AC-3 syncword 0x0B77
        let sync16 = u16::from_be_bytes([bytes[0], bytes[1]]);
        if sync16 == EAC3_SYNCWORD {
            return DetectedStreamFormat::DolbyAtmosEac3;
        }

        // Direct MAT syncword 0x07B5 / 0x07B6
        if sync16 == MAT_SYNC_WORD || sync16 == MAT_SYNC_WORD_ALT {
            return DetectedStreamFormat::DolbyMat;
        }

        // Direct DTS core syncword 0x7FFE8001
        let sync32 = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if sync32 == DTS_SYNCWORD_CORE_BE || sync32 == DTS_SYNCWORD_14BIT_BE {
            return DetectedStreamFormat::DtsX;
        }

        DetectedStreamFormat::Pcm
    }

    /// Decodes a stream chunk, automatically switching decoders and applying micro-fade smoothing if format changes.
    pub fn decode_auto(&mut self, chunk: &[u8]) -> Option<DecodedFrame> {
        let detected = Self::inspect_format(chunk);

        let format_changed = self.current_format != DetectedStreamFormat::Unknown
            && detected != DetectedStreamFormat::Unknown
            && self.current_format != detected;

        if format_changed {
            self.stats.format_transitions += 1;
            self.stats.crossfades_performed += 1;
        }
        self.current_format = detected;
        self.stats.active_format = Some(detected);

        let mut decoded = match detected {
            DetectedStreamFormat::DolbyMat => {
                if let Ok(mat_frame) = parse_dolby_mat_payload(chunk) {
                    Some(mat_frame.to_decoded_frame(self.pts))
                } else {
                    None
                }
            }
            DetectedStreamFormat::DtsX => {
                if let Ok(dts_frame) = parse_dts_frame(chunk) {
                    Some(dts_frame.to_decoded_frame(self.pts))
                } else {
                    None
                }
            }
            _ => {
                // Default E-AC-3 / Atmos or standard ingest
                self.eac3_decoder.decode_chunk(chunk).ok().flatten()
            }
        };

        if let Some(ref mut frame) = decoded {
            let frame_len = frame.audio.frame_count;
            self.pts += frame_len as f64 / 48000.0;

            // Apply raised-cosine micro-fade crossfade across format transition
            if format_changed && !self.crossfade_buffer.is_empty() {
                let crossfade_len = self.fade_frames.min(frame_len);
                let num_channels = frame.audio.channels.len().min(self.crossfade_buffer.len());

                for ch in 0..num_channels {
                    let old_ch = &self.crossfade_buffer[ch];
                    let old_len = old_ch.len();
                    for i in 0..crossfade_len {
                        let alpha = 0.5 * (1.0 - (PI * (i as f32 / crossfade_len as f32)).cos());
                        let old_sample = if old_len > i { old_ch[old_len - 1 - i] } else { 0.0 };
                        let new_sample = frame.audio.channels[ch][i];
                        frame.audio.channels[ch][i] = (old_sample * (1.0 - alpha)) + (new_sample * alpha);
                    }
                }
            }

            // Save tail of current frame for future crossfade smoothing
            let tail_len = self.fade_frames.min(frame_len);
            self.crossfade_buffer = frame
                .audio
                .channels
                .iter()
                .map(|ch| ch[ch.len() - tail_len..].to_vec())
                .collect();
        }

        decoded
    }

    /// Returns current switching statistics and active format.
    pub fn stats(&self) -> AutoSwitchStats {
        self.stats.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspects_different_formats_correctly() {
        // E-AC-3
        let eac3_header = [0x0B, 0x77, 0x02, 0xFF, 0x3F, 0x87, 0x00, 0x00];
        assert_eq!(
            FormatAutoSwitch::inspect_format(&eac3_header),
            DetectedStreamFormat::DolbyAtmosEac3
        );

        // MAT
        let mat_header = [0x07, 0xB5, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            FormatAutoSwitch::inspect_format(&mat_header),
            DetectedStreamFormat::DolbyMat
        );

        // DTS
        let dts_header = [0x7F, 0xFE, 0x80, 0x01, 0x80, 0x3C, 0x40, 0x09];
        assert_eq!(
            FormatAutoSwitch::inspect_format(&dts_header),
            DetectedStreamFormat::DtsX
        );
    }
}
