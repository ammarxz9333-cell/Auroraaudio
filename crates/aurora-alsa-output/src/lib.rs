//! Aurora-owned native Linux ALSA/ASoC playback sink.
//!
//! The reusable parts of this crate (F32 -> S32_LE conversion, channel padding
//! and validation) are platform-independent. Native playback is compiled only
//! on Linux and talks to ALSA directly; no `aplay` subprocess is involved.

use thiserror::Error;

/// Aurora's canonical speaker output width for 7.1.4.
pub const AURORA_LOGICAL_CHANNELS: usize = 12;

/// Native playback configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlsaOutputConfig {
    /// ALSA PCM device name, for example `hw:0,0`.
    pub device: String,
    /// Required playback sample rate.
    pub sample_rate: u32,
    /// Number of logical Aurora channels present in each input frame.
    pub logical_channels: usize,
    /// Physical PCM/TDM slots exposed by the hardware device.
    pub hardware_channels: usize,
    /// Preferred hardware period size in PCM frames.
    pub period_frames: usize,
    /// Preferred hardware buffer size in PCM frames.
    pub buffer_frames: usize,
}

impl Default for AlsaOutputConfig {
    fn default() -> Self {
        Self {
            device: "default".to_owned(),
            sample_rate: 48_000,
            logical_channels: AURORA_LOGICAL_CHANNELS,
            hardware_channels: AURORA_LOGICAL_CHANNELS,
            period_frames: 256,
            buffer_frames: 1_024,
        }
    }
}

/// Negotiated hardware parameters and live counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlsaOutputTelemetry {
    pub device: String,
    pub sample_rate: u32,
    pub hardware_channels: usize,
    pub period_frames: usize,
    pub buffer_frames: usize,
    pub frames_written: u64,
    pub xruns: u64,
    pub recoveries: u64,
    pub discontinuity_resets: u64,
}

/// Errors returned by Aurora's native ALSA sink.
#[derive(Debug, Error)]
pub enum AlsaOutputError {
    #[error("invalid ALSA output configuration: {0}")]
    InvalidConfig(String),
    #[error("speaker frame contains {actual} samples; expected a multiple of {logical_channels}")]
    InvalidFrameShape {
        actual: usize,
        logical_channels: usize,
    },
    #[error("speaker DSP produced a non-finite sample")]
    NonFiniteSample,
    #[error("ALSA negotiated an unsupported format: {0}")]
    Negotiation(String),
    #[error("native ALSA output is supported only on Linux")]
    UnsupportedPlatform,
    #[error("ALSA write made no forward progress")]
    NoForwardProgress,
    #[cfg(target_os = "linux")]
    #[error("ALSA error: {0}")]
    Alsa(#[from] alsa::Error),
}

/// Converts one interleaved Aurora F32 block to interleaved signed 32-bit PCM,
/// zero-padding any physical slots wider than the logical layout.
pub fn encode_f32_to_s32_padded(
    interleaved_f32: &[f32],
    logical_channels: usize,
    hardware_channels: usize,
) -> Result<Vec<i32>, AlsaOutputError> {
    let mut encoded = Vec::new();
    encode_f32_to_s32_padded_into(
        interleaved_f32,
        logical_channels,
        hardware_channels,
        &mut encoded,
    )?;
    Ok(encoded)
}

/// Same conversion as [`encode_f32_to_s32_padded`] but writes into caller-owned
/// storage so the native playback hot path can reuse capacity across Aurora's
/// small realtime DSP blocks.
fn encode_f32_to_s32_padded_into(
    interleaved_f32: &[f32],
    logical_channels: usize,
    hardware_channels: usize,
    encoded: &mut Vec<i32>,
) -> Result<(), AlsaOutputError> {
    validate_channel_shape(logical_channels, hardware_channels)?;
    if interleaved_f32.len() % logical_channels != 0 {
        return Err(AlsaOutputError::InvalidFrameShape {
            actual: interleaved_f32.len(),
            logical_channels,
        });
    }

    let frames = interleaved_f32.len() / logical_channels;
    let required = frames.checked_mul(hardware_channels).ok_or_else(|| {
        AlsaOutputError::InvalidConfig(
            "speaker frame/channel product overflowed addressable memory".to_owned(),
        )
    })?;
    encoded.clear();
    if encoded.capacity() < required {
        // After clear len is zero, so reserve the complete target. Use the
        // fallible reserve API so impossible geometry is an Aurora error rather
        // than a capacity-overflow panic in the realtime output path.
        encoded.try_reserve(required).map_err(|_| {
            AlsaOutputError::InvalidConfig(
                "unable to reserve the encoded ALSA output staging buffer".to_owned(),
            )
        })?;
    }
    for frame in interleaved_f32.chunks_exact(logical_channels) {
        for &sample in frame {
            encoded.push(f32_to_s32(sample)?);
        }
        encoded.extend(std::iter::repeat_n(
            0_i32,
            hardware_channels - logical_channels,
        ));
    }
    debug_assert_eq!(encoded.len(), required);
    Ok(())
}

/// Saturating full-scale conversion used by the hardware sink.
pub fn f32_to_s32(sample: f32) -> Result<i32, AlsaOutputError> {
    if !sample.is_finite() {
        return Err(AlsaOutputError::NonFiniteSample);
    }
    if sample <= -1.0 {
        return Ok(i32::MIN);
    }
    if sample >= 1.0 {
        return Ok(i32::MAX);
    }
    Ok((sample * i32::MAX as f32).round() as i32)
}

fn validate_config(config: &AlsaOutputConfig) -> Result<(), AlsaOutputError> {
    validate_channel_shape(config.logical_channels, config.hardware_channels)?;
    if config.device.trim().is_empty() {
        return Err(AlsaOutputError::InvalidConfig(
            "device name must not be empty".to_owned(),
        ));
    }
    if config.sample_rate == 0 {
        return Err(AlsaOutputError::InvalidConfig(
            "sample rate must be greater than zero".to_owned(),
        ));
    }
    if u32::try_from(config.hardware_channels).is_err() {
        return Err(AlsaOutputError::InvalidConfig(
            "hardware channel count exceeds ALSA's u32 range".to_owned(),
        ));
    }
    if config.period_frames == 0 {
        return Err(AlsaOutputError::InvalidConfig(
            "period size must be greater than zero".to_owned(),
        ));
    }
    if i64::try_from(config.period_frames).is_err() {
        return Err(AlsaOutputError::InvalidConfig(
            "period size exceeds ALSA's signed frame range".to_owned(),
        ));
    }
    if i64::try_from(config.buffer_frames).is_err() {
        return Err(AlsaOutputError::InvalidConfig(
            "buffer size exceeds ALSA's signed frame range".to_owned(),
        ));
    }
    if config.buffer_frames < config.period_frames.saturating_mul(2) {
        return Err(AlsaOutputError::InvalidConfig(
            "buffer size must be at least two periods".to_owned(),
        ));
    }
    Ok(())
}

fn validate_channel_shape(
    logical_channels: usize,
    hardware_channels: usize,
) -> Result<(), AlsaOutputError> {
    if logical_channels == 0 {
        return Err(AlsaOutputError::InvalidConfig(
            "logical channel count must be greater than zero".to_owned(),
        ));
    }
    if hardware_channels < logical_channels {
        return Err(AlsaOutputError::InvalidConfig(format!(
            "hardware exposes {hardware_channels} channels but Aurora needs {logical_channels}"
        )));
    }
    Ok(())
}

fn configured_channels_u32(channels: usize) -> Result<u32, AlsaOutputError> {
    u32::try_from(channels).map_err(|_| {
        AlsaOutputError::InvalidConfig("hardware channel count exceeds ALSA's u32 range".to_owned())
    })
}

fn configured_frames_i64(name: &str, frames: usize) -> Result<i64, AlsaOutputError> {
    i64::try_from(frames).map_err(|_| {
        AlsaOutputError::InvalidConfig(format!(
            "{name} frame count exceeds ALSA's signed frame range"
        ))
    })
}

fn negotiated_frames_usize(name: &str, frames: i64) -> Result<usize, AlsaOutputError> {
    usize::try_from(frames).map_err(|_| {
        AlsaOutputError::Negotiation(format!(
            "device negotiated an invalid negative or oversized {name} frame count: {frames}"
        ))
    })
}

/// Keep one hardware period free while requiring the rest of the playback
/// buffer to be primed before ALSA starts (or restarts) the stream. Compared
/// with starting after one period, this trades a small, deterministic startup
/// delay for materially more protection against decoder/render scheduling
/// spikes such as the first OpenJOC access unit after a bed-to-JOC transition.
fn playback_start_threshold(period_frames: usize, buffer_frames: usize) -> usize {
    buffer_frames
        .saturating_sub(period_frames)
        .max(period_frames)
        .min(buffer_frames)
}

/// Native blocking ALSA playback stream.
#[cfg(target_os = "linux")]
pub struct NativeAlsaPlayback {
    pcm: alsa::pcm::PCM,
    config: AlsaOutputConfig,
    telemetry: AlsaOutputTelemetry,
    /// Reused F32 -> S32_LE/TDM staging storage. Capacity is primed to one
    /// negotiated hardware period and grows only if a caller submits more.
    encoded_scratch: Vec<i32>,
}

#[cfg(target_os = "linux")]
impl NativeAlsaPlayback {
    /// Opens and configures a native ALSA playback device.
    ///
    /// Sample rate, channel count and S32_LE format must negotiate exactly.
    /// Period and buffer sizes may move to the nearest hardware-supported value
    /// and the actual values are exposed in telemetry.
    pub fn open(config: AlsaOutputConfig) -> Result<Self, AlsaOutputError> {
        use alsa::pcm::{Access, Format, HwParams};
        use alsa::{Direction, ValueOr};

        validate_config(&config)?;
        let configured_channels = configured_channels_u32(config.hardware_channels)?;
        let configured_period = configured_frames_i64("period", config.period_frames)?;
        let configured_buffer = configured_frames_i64("buffer", config.buffer_frames)?;
        let pcm = alsa::pcm::PCM::new(&config.device, Direction::Playback, false)?;

        let hw = HwParams::any(&pcm)?;
        hw.set_rate_resample(false)?;
        hw.set_access(Access::RWInterleaved)?;
        hw.set_format(Format::S32LE)?;
        hw.set_channels(configured_channels)?;
        hw.set_rate(config.sample_rate, ValueOr::Nearest)?;
        hw.set_period_size_near(configured_period, ValueOr::Nearest)?;
        hw.set_buffer_size_near(configured_buffer)?;
        pcm.hw_params(&hw)?;
        drop(hw);

        let current = pcm.hw_params_current()?;
        let negotiated_rate = current.get_rate()?;
        let negotiated_channels = usize::try_from(current.get_channels()?).map_err(|_| {
            AlsaOutputError::Negotiation(
                "device negotiated a channel count outside usize".to_owned(),
            )
        })?;
        let negotiated_format = current.get_format()?;
        let period_frames = negotiated_frames_usize("period", current.get_period_size()?)?;
        let buffer_frames = negotiated_frames_usize("buffer", current.get_buffer_size()?)?;
        drop(current);

        if negotiated_rate != config.sample_rate {
            return Err(AlsaOutputError::Negotiation(format!(
                "requested {} Hz but device negotiated {} Hz",
                config.sample_rate, negotiated_rate
            )));
        }
        if negotiated_channels != config.hardware_channels {
            return Err(AlsaOutputError::Negotiation(format!(
                "requested {} channels but device negotiated {}",
                config.hardware_channels, negotiated_channels
            )));
        }
        if negotiated_format != Format::S32LE {
            return Err(AlsaOutputError::Negotiation(format!(
                "requested S32_LE but device negotiated {negotiated_format:?}"
            )));
        }
        if period_frames == 0 || buffer_frames < period_frames.saturating_mul(2) {
            return Err(AlsaOutputError::Negotiation(format!(
                "device negotiated period={period_frames} buffer={buffer_frames}; Aurora requires a buffer of at least two periods"
            )));
        }

        let negotiated_period = configured_frames_i64("negotiated period", period_frames)?;
        let start_threshold = playback_start_threshold(period_frames, buffer_frames);
        let start_threshold = configured_frames_i64("playback start threshold", start_threshold)?;
        let sw = pcm.sw_params_current()?;
        sw.set_avail_min(negotiated_period)?;
        sw.set_start_threshold(start_threshold)?;
        pcm.sw_params(&sw)?;
        drop(sw);
        pcm.prepare()?;

        let telemetry = AlsaOutputTelemetry {
            device: config.device.clone(),
            sample_rate: negotiated_rate,
            hardware_channels: negotiated_channels,
            period_frames,
            buffer_frames,
            frames_written: 0,
            xruns: 0,
            recoveries: 0,
            discontinuity_resets: 0,
        };
        let scratch_capacity = period_frames.checked_mul(negotiated_channels).ok_or_else(|| {
            AlsaOutputError::Negotiation(
                "negotiated playback period/channel product overflowed usize".to_owned(),
            )
        })?;
        let mut encoded_scratch = Vec::new();
        encoded_scratch.try_reserve_exact(scratch_capacity).map_err(|_| {
            AlsaOutputError::InvalidConfig(
                "unable to reserve the native playback staging buffer".to_owned(),
            )
        })?;

        Ok(Self {
            pcm,
            config,
            telemetry,
            encoded_scratch,
        })
    }

    /// Writes an interleaved Aurora F32 block to the PCM device.
    ///
    /// `discontinuity=true` discards queued PCM before the new presentation
    /// epoch so old audio cannot leak across a source/format transition.
    pub fn write_interleaved_f32(
        &mut self,
        interleaved_f32: &[f32],
        discontinuity: bool,
    ) -> Result<(), AlsaOutputError> {
        if discontinuity {
            self.reset_for_discontinuity()?;
        }

        // Move the scratch out temporarily so the ALSA write can borrow `self`
        // mutably without aliasing a field borrowed for the encoded slice.
        let mut encoded = std::mem::take(&mut self.encoded_scratch);
        let conversion = encode_f32_to_s32_padded_into(
            interleaved_f32,
            self.config.logical_channels,
            self.config.hardware_channels,
            &mut encoded,
        );
        if let Err(error) = conversion {
            self.encoded_scratch = encoded;
            return Err(error);
        }
        let write_result = self.write_i32_frames(&encoded);
        self.encoded_scratch = encoded;
        write_result
    }

    /// Returns live negotiated format and fault counters.
    pub fn telemetry(&self) -> &AlsaOutputTelemetry {
        &self.telemetry
    }

    /// Drains queued playback before closing.
    pub fn drain(&mut self) -> Result<(), AlsaOutputError> {
        self.pcm.drain()?;
        Ok(())
    }

    /// Discards queued pre-break PCM and prepares the same ALSA handle for the
    /// next presentation epoch. Keeping one handle avoids a second-open `EBUSY`
    /// failure on exclusive `hw:X,Y` devices and preserves cumulative telemetry.
    pub fn reset_for_discontinuity(&mut self) -> Result<(), AlsaOutputError> {
        use alsa::pcm::State;

        match self.pcm.state() {
            State::Prepared | State::Running | State::Draining | State::Paused | State::XRun
            | State::Suspended => {
                self.pcm.drop()?;
            }
            State::Open | State::Setup => {}
            State::Disconnected => {
                return Err(AlsaOutputError::Negotiation(
                    "ALSA playback device is disconnected".to_owned(),
                ));
            }
            _ => {}
        }
        self.pcm.prepare()?;
        self.telemetry.discontinuity_resets =
            self.telemetry.discontinuity_resets.saturating_add(1);
        Ok(())
    }

    fn write_i32_frames(&mut self, encoded: &[i32]) -> Result<(), AlsaOutputError> {
        let channels = self.config.hardware_channels;
        if encoded.len() % channels != 0 {
            return Err(AlsaOutputError::InvalidFrameShape {
                actual: encoded.len(),
                logical_channels: channels,
            });
        }

        let total_frames = encoded.len() / channels;
        let mut frame_offset = 0_usize;
        while frame_offset < total_frames {
            let sample_offset = frame_offset.saturating_mul(channels);
            let result = {
                let io = self.pcm.io_i32()?;
                io.writei(&encoded[sample_offset..])
            };
            match result {
                Ok(0) => return Err(AlsaOutputError::NoForwardProgress),
                Ok(written_frames) => {
                    let remaining = total_frames - frame_offset;
                    if written_frames > remaining {
                        return Err(AlsaOutputError::Negotiation(format!(
                            "ALSA reported writing {written_frames} frames with only {remaining} remaining"
                        )));
                    }
                    frame_offset += written_frames;
                    self.telemetry.frames_written = self
                        .telemetry
                        .frames_written
                        .saturating_add(written_frames as u64);
                }
                Err(error) => {
                    if self.pcm.state() == alsa::pcm::State::XRun {
                        self.telemetry.xruns = self.telemetry.xruns.saturating_add(1);
                    }
                    self.pcm.try_recover(error, true)?;
                    self.telemetry.recoveries = self.telemetry.recoveries.saturating_add(1);
                }
            }
        }
        Ok(())
    }
}

/// Non-Linux stub so callers remain portable and fail explicitly at runtime.
#[cfg(not(target_os = "linux"))]
pub struct NativeAlsaPlayback;

#[cfg(not(target_os = "linux"))]
impl NativeAlsaPlayback {
    pub fn open(config: AlsaOutputConfig) -> Result<Self, AlsaOutputError> {
        validate_config(&config)?;
        Err(AlsaOutputError::UnsupportedPlatform)
    }

    pub fn write_interleaved_f32(
        &mut self,
        _interleaved_f32: &[f32],
        _discontinuity: bool,
    ) -> Result<(), AlsaOutputError> {
        Err(AlsaOutputError::UnsupportedPlatform)
    }

    pub fn reset_for_discontinuity(&mut self) -> Result<(), AlsaOutputError> {
        Err(AlsaOutputError::UnsupportedPlatform)
    }

    pub fn drain(&mut self) -> Result<(), AlsaOutputError> {
        Err(AlsaOutputError::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_saturates_and_rejects_non_finite_samples() {
        assert_eq!(f32_to_s32(-2.0).unwrap(), i32::MIN);
        assert_eq!(f32_to_s32(-1.0).unwrap(), i32::MIN);
        assert_eq!(f32_to_s32(0.0).unwrap(), 0);
        assert_eq!(f32_to_s32(1.0).unwrap(), i32::MAX);
        assert_eq!(f32_to_s32(2.0).unwrap(), i32::MAX);
        assert!(matches!(
            f32_to_s32(f32::NAN),
            Err(AlsaOutputError::NonFiniteSample)
        ));
    }

    #[test]
    fn canonical_channel_order_is_preserved() {
        let input = (0..AURORA_LOGICAL_CHANNELS)
            .map(|index| index as f32 / 32.0)
            .collect::<Vec<_>>();
        let encoded = encode_f32_to_s32_padded(
            &input,
            AURORA_LOGICAL_CHANNELS,
            AURORA_LOGICAL_CHANNELS,
        )
        .unwrap();
        assert_eq!(encoded.len(), AURORA_LOGICAL_CHANNELS);
        for (index, value) in encoded.iter().enumerate() {
            assert_eq!(*value, f32_to_s32(input[index]).unwrap());
        }
    }

    #[test]
    fn twelve_channels_zero_pad_cleanly_into_tdm16() {
        let input = (0..AURORA_LOGICAL_CHANNELS)
            .map(|index| (index + 1) as f32 / 32.0)
            .collect::<Vec<_>>();
        let encoded = encode_f32_to_s32_padded(&input, AURORA_LOGICAL_CHANNELS, 16).unwrap();
        assert_eq!(encoded.len(), 16);
        for index in 0..AURORA_LOGICAL_CHANNELS {
            assert_eq!(encoded[index], f32_to_s32(input[index]).unwrap());
        }
        assert_eq!(&encoded[AURORA_LOGICAL_CHANNELS..], &[0_i32; 4]);
    }

    #[test]
    fn caller_owned_conversion_storage_reuses_capacity() {
        let input = vec![0.25_f32; AURORA_LOGICAL_CHANNELS * 40];
        let mut encoded = Vec::with_capacity(16 * 40);
        let capacity = encoded.capacity();
        encode_f32_to_s32_padded_into(&input, AURORA_LOGICAL_CHANNELS, 16, &mut encoded)
            .unwrap();
        assert_eq!(encoded.len(), 16 * 40);
        assert_eq!(encoded.capacity(), capacity);
        encode_f32_to_s32_padded_into(&input, AURORA_LOGICAL_CHANNELS, 16, &mut encoded)
            .unwrap();
        assert_eq!(encoded.capacity(), capacity);
    }

    #[test]
    fn narrower_physical_output_fails_closed() {
        let error = encode_f32_to_s32_padded(&[0.0; 12], 12, 8).unwrap_err();
        assert!(matches!(error, AlsaOutputError::InvalidConfig(_)));
    }

    #[test]
    fn alsa_integer_geometry_is_checked_before_casting() {
        if usize::BITS > 32 {
            let too_many_channels = (u32::MAX as usize).saturating_add(1);
            assert!(configured_channels_u32(too_many_channels).is_err());
        }
        assert!(negotiated_frames_usize("period", -1).is_err());
        assert_eq!(negotiated_frames_usize("period", 256).unwrap(), 256);
    }

    #[test]
    fn buffer_must_hold_at_least_two_periods() {
        let config = AlsaOutputConfig {
            period_frames: 256,
            buffer_frames: 256,
            ..AlsaOutputConfig::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn playback_prefills_all_but_one_period_before_start() {
        assert_eq!(playback_start_threshold(256, 1_024), 768);
        assert_eq!(playback_start_threshold(128, 256), 128);
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires AURORA_ALSA_OUTPUT_DEVICE and a real Linux ALSA playback endpoint"]
    fn integration_open_write_silence_and_drain() {
        let device = std::env::var("AURORA_ALSA_OUTPUT_DEVICE")
            .expect("set AURORA_ALSA_OUTPUT_DEVICE to a real ALSA PCM device");
        let mut playback = NativeAlsaPlayback::open(AlsaOutputConfig {
            device,
            ..AlsaOutputConfig::default()
        })
        .unwrap();
        let silence = vec![0.0_f32; AURORA_LOGICAL_CHANNELS * 256];
        playback.write_interleaved_f32(&silence, true).unwrap();
        playback.drain().unwrap();
        assert!(playback.telemetry().frames_written >= 256);
        assert!(playback.telemetry().discontinuity_resets >= 1);
    }
}
