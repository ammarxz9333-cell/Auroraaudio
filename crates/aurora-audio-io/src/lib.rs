//! Offline WAV reading and writing for Aurora fixtures and render output.

use std::io::{Seek, Write};
use std::path::Path;

use aurora_core::{AudioBlock, AudioFormat, ChannelRole, SampleType};
use hound::{SampleFormat, WavReader};
#[cfg(test)]
use hound::{WavSpec, WavWriter};
use thiserror::Error;

/// Decoded planar WAV data.
#[derive(Debug, Clone, PartialEq)]
pub struct WavData {
    /// Audio format derived from the WAV header.
    pub format: AudioFormat,
    /// Total frame count in the decoded WAV.
    pub frame_count: usize,
    /// Planar channel samples normalized to approximately `-1.0..=1.0`.
    pub channels: Vec<Vec<f32>>,
}

impl WavData {
    /// Returns the number of frames in the decoded file.
    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    /// Returns true when every sample is finite.
    pub fn all_samples_finite(&self) -> bool {
        self.channels
            .iter()
            .flat_map(|channel| channel.iter())
            .all(|sample| sample.is_finite())
    }
}

/// Metadata collected while writing a multichannel WAV.
#[derive(Debug, Clone, PartialEq)]
pub struct WavWriteReport {
    /// Peak absolute sample value per output channel before clipping.
    pub peak_per_channel: Vec<f32>,
    /// Whether any sample exceeded the `-1.0..=1.0` output range.
    pub clipped: bool,
    /// Number of frames written.
    pub frames_written: usize,
    /// Number of channels written.
    pub channel_count: usize,
    /// WAVE_FORMAT_EXTENSIBLE channel mask written to the file.
    pub channel_mask: u32,
}

/// Errors returned by WAV IO.
#[derive(Debug, Error)]
pub enum AudioIoError {
    /// The WAV library returned an error.
    #[error("wav error: {0}")]
    Wav(#[from] hound::Error),
    /// Standard file IO failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The WAV channel count was zero.
    #[error("wav file must contain at least one channel")]
    ZeroChannels,
    /// The provided planar channel data is empty.
    #[error("at least one output channel is required")]
    EmptyOutput,
    /// Output channels have inconsistent frame counts.
    #[error("output channel {channel} has {actual} frames, expected {expected}")]
    InconsistentFrameCount {
        /// Channel index with the invalid frame count.
        channel: usize,
        /// Actual frame count.
        actual: usize,
        /// Expected frame count.
        expected: usize,
    },
    /// The file has an unsupported sample representation.
    #[error("unsupported wav sample format: {format:?} with {bits_per_sample} bits")]
    UnsupportedFormat {
        /// WAV sample format.
        format: SampleFormat,
        /// Bits per sample.
        bits_per_sample: u16,
    },
    /// The provided channel roles cannot be represented as a standard WAV mask.
    #[error("channel roles cannot be represented as a WAV channel mask")]
    UnsupportedChannelMask,
}

/// Reads a PCM integer or 32-bit float WAV file into planar `f32` samples.
pub fn read_wav<P: AsRef<Path>>(path: P) -> Result<WavData, AudioIoError> {
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    let channel_count = usize::from(spec.channels);
    if channel_count == 0 {
        return Err(AudioIoError::ZeroChannels);
    }

    let interleaved = match spec.sample_format {
        SampleFormat::Float if spec.bits_per_sample == 32 => {
            reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?
        }
        SampleFormat::Int if spec.bits_per_sample <= 16 => {
            let scale = integer_scale(spec.bits_per_sample);
            reader
                .samples::<i16>()
                .map(|sample| sample.map(|value| value as f32 / scale))
                .collect::<Result<Vec<_>, _>>()?
        }
        SampleFormat::Int if spec.bits_per_sample <= 32 => {
            let scale = integer_scale(spec.bits_per_sample);
            reader
                .samples::<i32>()
                .map(|sample| sample.map(|value| value as f32 / scale))
                .collect::<Result<Vec<_>, _>>()?
        }
        _ => {
            return Err(AudioIoError::UnsupportedFormat {
                format: spec.sample_format,
                bits_per_sample: spec.bits_per_sample,
            });
        }
    };

    let frame_count = interleaved.len() / channel_count;
    let mut channels = vec![Vec::with_capacity(frame_count); channel_count];
    for frame in interleaved.chunks_exact(channel_count) {
        for (channel, sample) in channels.iter_mut().zip(frame) {
            channel.push(*sample);
        }
    }

    Ok(WavData {
        format: AudioFormat {
            sample_rate: spec.sample_rate,
            channel_count,
            sample_type: SampleType::F32,
            block_size: 0,
        },
        frame_count,
        channels,
    })
}

/// Writes planar `f32` samples as a multichannel 32-bit float WAV.
pub fn write_wav_f32<P: AsRef<Path>>(
    path: P,
    sample_rate: u32,
    channels: &[Vec<f32>],
) -> Result<WavWriteReport, AudioIoError> {
    let roles = (0..channels.len())
        .map(|index| ChannelRole::Custom(format!("channel-{index}")))
        .collect::<Vec<_>>();
    write_wav_f32_with_channel_roles(path, sample_rate, channels, &roles)
}

/// Writes planar `f32` samples as WAVE_FORMAT_EXTENSIBLE with a role-derived mask.
pub fn write_wav_f32_with_channel_roles<P: AsRef<Path>>(
    path: P,
    sample_rate: u32,
    channels: &[Vec<f32>],
    channel_roles: &[ChannelRole],
) -> Result<WavWriteReport, AudioIoError> {
    validate_output_channels(channels)?;
    if channels.len() != channel_roles.len() {
        return Err(AudioIoError::InconsistentFrameCount {
            channel: channel_roles.len(),
            actual: channel_roles.len(),
            expected: channels.len(),
        });
    }
    // Unlabelled exports use mask zero and retain caller order. Explicit
    // speaker roles must be complete and unique, and are serialized in bit order.
    let unlabelled = channel_roles
        .iter()
        .all(|role| matches!(role, ChannelRole::Custom(_)));
    let channel_mask = if unlabelled {
        0
    } else {
        wav_channel_mask(channel_roles)?
    };
    let mut channel_order: Vec<usize> = (0..channels.len()).collect();
    if !unlabelled {
        channel_order.sort_by_key(|&index| channel_roles[index].wav_channel_mask_bit());
    }

    let frame_count = channels[0].len();
    let mut peak_per_channel = vec![0.0_f32; channels.len()];
    let mut clipped = false;
    let mut writer = std::io::BufWriter::new(std::fs::File::create(path)?);
    write_extensible_float_header(
        &mut writer,
        sample_rate,
        channels.len() as u16,
        frame_count as u32,
        channel_mask,
    )?;
    for (frame_index, _) in channels[0].iter().enumerate() {
        for &channel_index in &channel_order {
            let sample = channels[channel_index][frame_index];
            let peak = sample.abs();
            peak_per_channel[channel_index] = peak_per_channel[channel_index].max(peak);
            if !(-1.0..=1.0).contains(&sample) {
                clipped = true;
            }
            writer.write_all(&sample.clamp(-1.0, 1.0).to_le_bytes())?;
        }
    }
    writer.flush()?;

    Ok(WavWriteReport {
        peak_per_channel,
        clipped,
        frames_written: frame_count,
        channel_count: channels.len(),
        channel_mask,
    })
}

/// Builds an [`AudioBlock`] view by copying a frame range from planar samples.
pub fn audio_block_from_range(
    channels: &[Vec<f32>],
    start_frame: usize,
    end_frame: usize,
    sample_rate: u32,
) -> AudioBlock {
    let clamped_end = end_frame.min(channels.first().map_or(0, Vec::len));
    let block_channels = channels
        .iter()
        .map(|channel| channel[start_frame..clamped_end].to_vec())
        .collect::<Vec<_>>();

    AudioBlock {
        channels: block_channels,
        frame_count: clamped_end.saturating_sub(start_frame),
        presentation_time_seconds: start_frame as f64 / f64::from(sample_rate),
        discontinuity: start_frame == 0,
    }
}

fn validate_output_channels(channels: &[Vec<f32>]) -> Result<(), AudioIoError> {
    if channels.is_empty() {
        return Err(AudioIoError::EmptyOutput);
    }

    let expected = channels[0].len();
    for (channel, samples) in channels.iter().enumerate() {
        if samples.len() != expected {
            return Err(AudioIoError::InconsistentFrameCount {
                channel,
                actual: samples.len(),
                expected,
            });
        }
    }

    Ok(())
}

fn integer_scale(bits_per_sample: u16) -> f32 {
    let clamped_bits = bits_per_sample.clamp(1, 32);
    2_f32.powi(i32::from(clamped_bits) - 1)
}

/// Returns the ORed WAV speaker mask for known channel roles.
pub fn wav_channel_mask(channel_roles: &[ChannelRole]) -> Result<u32, AudioIoError> {
    let mut mask = 0_u32;
    for role in channel_roles {
        let bit = role
            .wav_channel_mask_bit()
            .ok_or(AudioIoError::UnsupportedChannelMask)?;
        if mask & bit != 0 {
            return Err(AudioIoError::UnsupportedChannelMask);
        }
        mask |= bit;
    }
    Ok(mask)
}

fn write_extensible_float_header<W: Write + Seek>(
    writer: &mut W,
    sample_rate: u32,
    channels: u16,
    frames: u32,
    channel_mask: u32,
) -> Result<(), AudioIoError> {
    let bits_per_sample = 32_u16;
    let bytes_per_sample = u32::from(bits_per_sample / 8);
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data_bytes = frames * u32::from(channels) * bytes_per_sample;
    let fmt_size = 40_u32;
    let riff_size = 4 + (8 + fmt_size) + (8 + data_bytes);

    writer.write_all(b"RIFF")?;
    writer.write_all(&riff_size.to_le_bytes())?;
    writer.write_all(b"WAVE")?;
    writer.write_all(b"fmt ")?;
    writer.write_all(&fmt_size.to_le_bytes())?;
    writer.write_all(&0xFFFE_u16.to_le_bytes())?;
    writer.write_all(&channels.to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&bits_per_sample.to_le_bytes())?;
    writer.write_all(&22_u16.to_le_bytes())?;
    writer.write_all(&bits_per_sample.to_le_bytes())?;
    writer.write_all(&channel_mask.to_le_bytes())?;
    writer.write_all(&[
        0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B,
        0x71,
    ])?;
    writer.write_all(b"data")?;
    writer.write_all(&data_bytes.to_le_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn mono_wav_round_trip_parsing() {
        let path = temp_wav_path("mono_round_trip");
        write_wav_f32_with_channel_roles(
            &path,
            48_000,
            &[vec![0.0, 0.25, -0.25, 0.5]],
            &[ChannelRole::FrontCenter],
        )
        .unwrap();

        let wav = read_wav(&path).unwrap();

        assert_eq!(wav.format.sample_rate, 48_000);
        assert_eq!(wav.format.channel_count, 1);
        assert_eq!(wav.frame_count(), 4);
        assert_eq!(wav.channels[0], vec![0.0, 0.25, -0.25, 0.5]);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn mono_pcm_integer_wav_parses_to_f32() {
        let path = temp_wav_path("mono_pcm_integer");
        let spec = WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(&path, spec).unwrap();
        writer.write_sample(0_i16).unwrap();
        writer.write_sample(16_384_i16).unwrap();
        writer.write_sample(-16_384_i16).unwrap();
        writer.finalize().unwrap();

        let wav = read_wav(&path).unwrap();

        assert_eq!(wav.format.channel_count, 1);
        assert_eq!(wav.frame_count(), 3);
        assert!((wav.channels[0][1] - 0.5).abs() < 0.0001);
        assert!((wav.channels[0][2] + 0.5).abs() < 0.0001);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn correct_output_channel_count() {
        let path = temp_wav_path("channel_count");
        let report = write_wav_f32_with_channel_roles(
            &path,
            48_000,
            &[vec![0.0; 8], vec![0.0; 8]],
            &[ChannelRole::FrontLeft, ChannelRole::FrontRight],
        )
        .unwrap();
        let wav = read_wav(&path).unwrap();

        assert_eq!(report.channel_count, 2);
        assert_eq!(wav.format.channel_count, 2);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn clipping_warning_is_triggered_correctly() {
        let path = temp_wav_path("clipping");
        let report = write_wav_f32_with_channel_roles(
            &path,
            48_000,
            &[vec![0.0, 1.25, -1.5]],
            &[ChannelRole::FrontCenter],
        )
        .unwrap();

        assert!(report.clipped);
        assert_eq!(report.peak_per_channel, vec![1.5]);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn wav_extensible_header_contains_standard_surround_mask() {
        let path = temp_wav_path("mask");
        write_wav_f32_with_channel_roles(
            &path,
            48_000,
            &vec![vec![0.0; 4]; 6],
            &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
            ],
        )
        .unwrap();

        let header = fs::read(&path).unwrap();

        assert_eq!(&header[0..4], b"RIFF");
        assert_eq!(&header[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([header[20], header[21]]), 0xFFFE);
        assert_eq!(u16::from_le_bytes([header[22], header[23]]), 6);
        assert_eq!(
            u32::from_le_bytes([header[24], header[25], header[26], header[27]]),
            48_000
        );
        assert_eq!(u16::from_le_bytes([header[34], header[35]]), 32);
        assert_eq!(
            u32::from_le_bytes([header[40], header[41], header[42], header[43]]),
            0x60F
        );
        assert_eq!(
            &header[44..60],
            &[
                0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38,
                0x9B, 0x71
            ]
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn standard_layout_channel_masks_are_correct() {
        assert_eq!(
            wav_channel_mask(&[ChannelRole::FrontLeft, ChannelRole::FrontRight]).unwrap(),
            0x3
        );
        assert_eq!(
            wav_channel_mask(&[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
            ])
            .unwrap(),
            0x60F
        );
        assert_eq!(
            wav_channel_mask(&[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::SurroundBackLeft,
                ChannelRole::SurroundBackRight,
            ])
            .unwrap(),
            0x63F
        );
        assert_eq!(
            wav_channel_mask(&[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::TopFrontLeft,
                ChannelRole::TopFrontRight,
            ])
            .unwrap(),
            0x560F
        );
    }

    #[test]
    fn seven_one_four_export_permutates_samples_with_mask_order() {
        let path = temp_wav_path("714_order");
        let roles = aurora_core::StandardLayout::SevenOneFour.canonical_roles();
        let channels = (0..12)
            .map(|i| vec![(i + 1) as f32 / 16.0])
            .collect::<Vec<_>>();
        write_wav_f32_with_channel_roles(&path, 48000, &channels, roles).unwrap();
        let decoded = read_wav(&path).unwrap();
        let order = [0, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11];
        for (file_channel, source) in order.into_iter().enumerate() {
            assert_eq!(decoded.channels[file_channel], channels[source]);
        }
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn duplicate_channel_roles_are_rejected() {
        assert!(wav_channel_mask(&[ChannelRole::FrontLeft, ChannelRole::FrontLeft]).is_err());
    }

    #[test]
    fn unlabelled_wav_preserves_channel_order() {
        let path = temp_wav_path("unlabelled");
        let channels = vec![vec![0.25, 0.5], vec![0.0, -0.25]];
        let report = write_wav_f32(&path, 48000, &channels).unwrap();
        assert_eq!(report.channel_mask, 0);
        assert_eq!(read_wav(&path).unwrap().channels, channels);
        fs::remove_file(path).unwrap();
    }

    fn temp_wav_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("aurora_{name}_{nonce}.wav"))
    }
}
