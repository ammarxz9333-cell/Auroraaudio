//! Aurora-owned native Linux ALSA/ASoC capture for direct eARC carrier audio.
//!
//! This crate owns only the physical S32_LE capture boundary. It does not
//! parse IEC61937, decode codecs or infer JOC/Atmos capability. Captured slot
//! samples remain bit-preserving signed 32-bit words for the existing Aurora
//! carrier normalizer.

use thiserror::Error;

const MIN_CAPTURE_HEADROOM_MS: u64 = 40;

/// Native ALSA capture configuration for the recovered eARC serial-audio carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlsaInputConfig {
    /// ALSA PCM device name, for example `hw:0,0`.
    pub device: String,
    /// Recovered serial-audio carrier frame rate, typically 192 kHz for DD+ IEC61937.
    pub sample_rate: u32,
    /// Physical interleaved serial-audio slots per frame.
    pub channels: usize,
    /// Preferred hardware period size in carrier frames.
    pub period_frames: usize,
    /// Preferred hardware buffer size in carrier frames. The native backend
    /// may raise this preference to preserve Aurora's minimum realtime capture
    /// headroom; the negotiated value is exposed through telemetry.
    pub buffer_frames: usize,
}

impl Default for AlsaInputConfig {
    fn default() -> Self {
        Self {
            device: "default".to_owned(),
            sample_rate: 192_000,
            channels: 2,
            // At the 192-kHz eARC carrier rate this is 5.33 ms, matching a
            // 256-frame period at Aurora's 48-kHz speaker rate while reducing
            // capture syscalls versus the previous 256-carrier-frame period.
            period_frames: 1_024,
            // 42.67 ms of carrier headroom at 192 kHz. Buffer size does not add
            // read latency by itself; it protects the single-process prototype
            // while a burst occasionally spends time in JOC decode/render.
            buffer_frames: 8_192,
        }
    }
}

/// One captured interleaved S32_LE block borrowed from the capture object's
/// preallocated period buffer. The slice remains valid until the next mutable
/// operation on the same `NativeAlsaCapture`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureBlock<'a> {
    /// Interleaved signed 32-bit slot values in ALSA channel order.
    pub interleaved_s32: &'a [i32],
    /// Number of complete carrier frames in this block.
    pub frame_count: usize,
    /// True when a capture fault was recovered immediately before this block.
    /// Callers must reset partial carrier/decoder state before consuming it.
    pub discontinuity: bool,
}

/// Owned capture period suitable for transfer to a bounded worker queue.
///
/// The vector length is exactly the number of valid captured slot samples.
/// Its capacity remains large enough for the negotiated ALSA period so the
/// consumer can recycle the vector back to [`NativeAlsaCapture`] without a
/// steady-state allocation.
#[derive(Debug, PartialEq, Eq)]
pub struct OwnedCaptureBlock {
    pub interleaved_s32: Vec<i32>,
    pub frame_count: usize,
    pub discontinuity: bool,
}

/// Negotiated capture geometry and live transport counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlsaInputTelemetry {
    pub device: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub period_frames: usize,
    pub buffer_frames: usize,
    pub frames_captured: u64,
    pub xruns: u64,
    pub recoveries: u64,
    pub discontinuities: u64,
}

#[derive(Debug, Error)]
pub enum AlsaInputError {
    #[error("invalid ALSA input configuration: {0}")]
    InvalidConfig(String),
    #[error("ALSA negotiated an unsupported capture format: {0}")]
    Negotiation(String),
    #[error("native ALSA direct-eARC capture is supported only on Linux")]
    UnsupportedPlatform,
    #[error("ALSA capture made no forward progress")]
    NoForwardProgress,
    #[cfg(target_os = "linux")]
    #[error("ALSA error: {0}")]
    Alsa(#[from] alsa::Error),
}

fn validate_config(config: &AlsaInputConfig) -> Result<(), AlsaInputError> {
    if config.device.trim().is_empty() {
        return Err(AlsaInputError::InvalidConfig(
            "device name must not be empty".to_owned(),
        ));
    }
    if config.sample_rate == 0 {
        return Err(AlsaInputError::InvalidConfig(
            "sample rate must be greater than zero".to_owned(),
        ));
    }
    if config.channels == 0 {
        return Err(AlsaInputError::InvalidConfig(
            "channel/slot count must be greater than zero".to_owned(),
        ));
    }
    if u32::try_from(config.channels).is_err() {
        return Err(AlsaInputError::InvalidConfig(
            "channel/slot count exceeds ALSA's u32 range".to_owned(),
        ));
    }
    if config.period_frames == 0 {
        return Err(AlsaInputError::InvalidConfig(
            "period size must be greater than zero".to_owned(),
        ));
    }
    if i64::try_from(config.period_frames).is_err() {
        return Err(AlsaInputError::InvalidConfig(
            "period size exceeds ALSA's signed frame range".to_owned(),
        ));
    }
    if i64::try_from(config.buffer_frames).is_err() {
        return Err(AlsaInputError::InvalidConfig(
            "buffer size exceeds ALSA's signed frame range".to_owned(),
        ));
    }
    if config.buffer_frames < config.period_frames.saturating_mul(2) {
        return Err(AlsaInputError::InvalidConfig(
            "buffer size must be at least two periods".to_owned(),
        ));
    }
    Ok(())
}

/// Minimum hardware capture capacity used to isolate the realtime carrier
/// reader from occasional decoder/render work in the current single-process
/// prototype. This is buffer headroom, not an intentional read delay.
fn minimum_capture_buffer_frames(sample_rate: u32, period_frames: usize) -> usize {
    let duration_frames = u64::from(sample_rate)
        .saturating_mul(MIN_CAPTURE_HEADROOM_MS)
        .saturating_add(999)
        / 1_000;
    let duration_frames = usize::try_from(duration_frames).unwrap_or(usize::MAX);
    duration_frames.max(period_frames.saturating_mul(2))
}

/// Reject a negotiated geometry that silently collapses the realtime headroom
/// requested above. `set_buffer_size_near` is permitted to move to a nearby
/// value, so the post-negotiation contract must be checked independently.
fn validate_negotiated_buffer(
    sample_rate: u32,
    period_frames: usize,
    buffer_frames: usize,
) -> Result<(), AlsaInputError> {
    if period_frames == 0 {
        return Err(AlsaInputError::Negotiation(
            "device negotiated a zero-frame capture period".to_owned(),
        ));
    }
    let required = minimum_capture_buffer_frames(sample_rate, period_frames);
    if buffer_frames < required {
        return Err(AlsaInputError::Negotiation(format!(
            "device negotiated period={period_frames} buffer={buffer_frames}; Aurora requires at least {required} capture frames ({MIN_CAPTURE_HEADROOM_MS} ms headroom and at least two periods)"
        )));
    }
    Ok(())
}

fn configured_channels_u32(channels: usize) -> Result<u32, AlsaInputError> {
    u32::try_from(channels).map_err(|_| {
        AlsaInputError::InvalidConfig("channel/slot count exceeds ALSA's u32 range".to_owned())
    })
}

fn configured_frames_i64(name: &str, frames: usize) -> Result<i64, AlsaInputError> {
    i64::try_from(frames).map_err(|_| {
        AlsaInputError::InvalidConfig(format!(
            "{name} frame count exceeds ALSA's signed frame range"
        ))
    })
}

fn negotiated_frames_usize(name: &str, frames: i64) -> Result<usize, AlsaInputError> {
    usize::try_from(frames).map_err(|_| {
        AlsaInputError::Negotiation(format!(
            "device negotiated an invalid negative or oversized {name} frame count: {frames}"
        ))
    })
}

fn prepare_recycled_buffer(
    buffer: &mut Vec<i32>,
    required_samples: usize,
) -> Result<(), AlsaInputError> {
    if buffer.capacity() < required_samples {
        return Err(AlsaInputError::InvalidConfig(format!(
            "recycled capture buffer capacity {} is smaller than negotiated period requirement {required_samples}",
            buffer.capacity()
        )));
    }
    buffer.resize(required_samples, 0_i32);
    Ok(())
}

/// Converts interleaved native i32 slot words into the exact little-endian byte
/// representation expected by Aurora's existing S32_LE carrier normalizer.
pub fn interleaved_i32_to_le_bytes(samples: &[i32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len().saturating_mul(4));
    for &sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

#[cfg(target_os = "linux")]
pub struct NativeAlsaCapture {
    pcm: alsa::pcm::PCM,
    config: AlsaInputConfig,
    telemetry: AlsaInputTelemetry,
    pending_discontinuity: bool,
    samples: Vec<i32>,
}

#[cfg(target_os = "linux")]
impl NativeAlsaCapture {
    /// Opens the ALSA capture endpoint with exact S32_LE/rate/channel semantics.
    /// ALSA resampling is disabled. Period and buffer sizes may negotiate to a
    /// nearby hardware-supported value and are exposed through telemetry. The
    /// requested buffer is raised when needed to retain at least 40 ms of
    /// capture headroom in the current synchronous decoder prototype.
    pub fn open(config: AlsaInputConfig) -> Result<Self, AlsaInputError> {
        use alsa::pcm::{Access, Format, HwParams};
        use alsa::{Direction, ValueOr};

        validate_config(&config)?;
        let configured_channels = configured_channels_u32(config.channels)?;
        let configured_period = configured_frames_i64("period", config.period_frames)?;
        let requested_buffer_frames = config.buffer_frames.max(minimum_capture_buffer_frames(
            config.sample_rate,
            config.period_frames,
        ));
        let configured_buffer = configured_frames_i64("buffer", requested_buffer_frames)?;

        let pcm = alsa::pcm::PCM::new(&config.device, Direction::Capture, false)?;
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
            AlsaInputError::Negotiation(
                "device negotiated a channel count outside usize".to_owned(),
            )
        })?;
        let negotiated_format = current.get_format()?;
        let period_frames = negotiated_frames_usize("period", current.get_period_size()?)?;
        let buffer_frames = negotiated_frames_usize("buffer", current.get_buffer_size()?)?;
        drop(current);

        if negotiated_rate != config.sample_rate {
            return Err(AlsaInputError::Negotiation(format!(
                "requested {} Hz but device negotiated {} Hz",
                config.sample_rate, negotiated_rate
            )));
        }
        if negotiated_channels != config.channels {
            return Err(AlsaInputError::Negotiation(format!(
                "requested {} channels/slots but device negotiated {}",
                config.channels, negotiated_channels
            )));
        }
        if negotiated_format != Format::S32LE {
            return Err(AlsaInputError::Negotiation(format!(
                "requested S32_LE but device negotiated {negotiated_format:?}"
            )));
        }
        validate_negotiated_buffer(negotiated_rate, period_frames, buffer_frames)?;

        let sample_count = period_frames
            .checked_mul(negotiated_channels)
            .ok_or_else(|| {
                AlsaInputError::Negotiation(
                    "negotiated capture period/channel product overflowed usize".to_owned(),
                )
            })?;
        let mut samples = Vec::new();
        samples.try_reserve_exact(sample_count).map_err(|_| {
            AlsaInputError::InvalidConfig(
                "unable to reserve the native capture period buffer".to_owned(),
            )
        })?;
        samples.resize(sample_count, 0_i32);

        let sw = pcm.sw_params_current()?;
        sw.set_avail_min(configured_frames_i64("negotiated period", period_frames)?)?;
        pcm.sw_params(&sw)?;
        drop(sw);
        pcm.prepare()?;
        pcm.start()?;

        let telemetry = AlsaInputTelemetry {
            device: config.device.clone(),
            sample_rate: negotiated_rate,
            channels: negotiated_channels,
            period_frames,
            buffer_frames,
            frames_captured: 0,
            xruns: 0,
            recoveries: 0,
            discontinuities: 0,
        };

        Ok(Self {
            pcm,
            config,
            telemetry,
            pending_discontinuity: false,
            samples,
        })
    }

    /// Reads one block into the capture object's preallocated period storage.
    /// Recoverable ALSA faults are repaired in place, and the next successful
    /// block is explicitly marked as a discontinuity.
    pub fn read_block(&mut self) -> Result<CaptureBlock<'_>, AlsaInputError> {
        let channels = self.config.channels;

        loop {
            let result = {
                let io = self.pcm.io_i32()?;
                io.readi(&mut self.samples)
            };
            match result {
                Ok(0) => return Err(AlsaInputError::NoForwardProgress),
                Ok(frames) => {
                    let sample_count = frames.checked_mul(channels).ok_or_else(|| {
                        AlsaInputError::Negotiation(
                            "captured frame/channel product overflowed usize".to_owned(),
                        )
                    })?;
                    if sample_count > self.samples.len() {
                        return Err(AlsaInputError::Negotiation(
                            "ALSA returned more capture frames than the negotiated period buffer"
                                .to_owned(),
                        ));
                    }
                    self.telemetry.frames_captured = self
                        .telemetry
                        .frames_captured
                        .saturating_add(frames as u64);
                    let discontinuity = std::mem::take(&mut self.pending_discontinuity);
                    if discontinuity {
                        self.telemetry.discontinuities =
                            self.telemetry.discontinuities.saturating_add(1);
                    }
                    return Ok(CaptureBlock {
                        interleaved_s32: &self.samples[..sample_count],
                        frame_count: frames,
                        discontinuity,
                    });
                }
                Err(error) => {
                    if self.pcm.state() == alsa::pcm::State::XRun {
                        self.telemetry.xruns = self.telemetry.xruns.saturating_add(1);
                    }
                    self.pcm.try_recover(error, true)?;
                    self.telemetry.recoveries = self.telemetry.recoveries.saturating_add(1);
                    self.pending_discontinuity = true;
                    if self.pcm.state() == alsa::pcm::State::Prepared {
                        self.pcm.start()?;
                    }
                }
            }
        }
    }

    /// Reads one period and transfers ownership of its sample storage without
    /// copying the captured words. `replacement` must be a previously allocated
    /// or recycled buffer with enough capacity for the negotiated period.
    pub fn read_owned_block(
        &mut self,
        mut replacement: Vec<i32>,
    ) -> Result<OwnedCaptureBlock, AlsaInputError> {
        let required_samples = self.samples.len();
        prepare_recycled_buffer(&mut replacement, required_samples)?;
        let (sample_count, frame_count, discontinuity) = {
            let block = self.read_block()?;
            (
                block.interleaved_s32.len(),
                block.frame_count,
                block.discontinuity,
            )
        };
        std::mem::swap(&mut self.samples, &mut replacement);
        replacement.truncate(sample_count);
        Ok(OwnedCaptureBlock {
            interleaved_s32: replacement,
            frame_count,
            discontinuity,
        })
    }

    pub fn telemetry(&self) -> &AlsaInputTelemetry {
        &self.telemetry
    }
}

#[cfg(not(target_os = "linux"))]
pub struct NativeAlsaCapture {
    telemetry: AlsaInputTelemetry,
}

#[cfg(not(target_os = "linux"))]
impl NativeAlsaCapture {
    pub fn open(config: AlsaInputConfig) -> Result<Self, AlsaInputError> {
        validate_config(&config)?;
        Err(AlsaInputError::UnsupportedPlatform)
    }

    pub fn read_block(&mut self) -> Result<CaptureBlock<'_>, AlsaInputError> {
        Err(AlsaInputError::UnsupportedPlatform)
    }

    pub fn read_owned_block(
        &mut self,
        _replacement: Vec<i32>,
    ) -> Result<OwnedCaptureBlock, AlsaInputError> {
        Err(AlsaInputError::UnsupportedPlatform)
    }

    pub fn telemetry(&self) -> &AlsaInputTelemetry {
        &self.telemetry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_words_serialize_without_mutating_bits() {
        let words = [i32::MIN, -1, 0, 1, i32::MAX, 0x1234_5678];
        let bytes = interleaved_i32_to_le_bytes(&words);
        assert_eq!(bytes.len(), words.len() * 4);
        for (word, encoded) in words.iter().zip(bytes.chunks_exact(4)) {
            assert_eq!(*word, i32::from_le_bytes(encoded.try_into().unwrap()));
        }
    }

    #[test]
    fn default_geometry_has_realtime_headroom_at_192k() {
        let config = AlsaInputConfig::default();
        assert_eq!(config.sample_rate, 192_000);
        assert_eq!(config.period_frames, 1_024);
        assert_eq!(config.buffer_frames, 8_192);
        assert_eq!(config.buffer_frames / config.period_frames, 8);
        assert_eq!(minimum_capture_buffer_frames(192_000, 256), 7_680);
    }

    #[test]
    fn minimum_capture_headroom_never_breaks_two_period_rule() {
        assert_eq!(minimum_capture_buffer_frames(48_000, 2_000), 4_000);
    }

    #[test]
    fn negotiated_buffer_cannot_fall_below_realtime_headroom() {
        assert!(validate_negotiated_buffer(192_000, 256, 7_679).is_err());
        assert!(validate_negotiated_buffer(192_000, 256, 7_680).is_ok());
        assert!(validate_negotiated_buffer(48_000, 2_000, 3_999).is_err());
        assert!(validate_negotiated_buffer(48_000, 2_000, 4_000).is_ok());
    }

    #[test]
    fn recycled_capture_buffer_requires_preallocated_capacity() {
        let mut too_small = Vec::<i32>::with_capacity(7);
        assert!(prepare_recycled_buffer(&mut too_small, 8).is_err());

        let mut recycled = Vec::<i32>::with_capacity(8);
        prepare_recycled_buffer(&mut recycled, 8).unwrap();
        assert_eq!(recycled.len(), 8);
        assert!(recycled.iter().all(|sample| *sample == 0));
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
        let config = AlsaInputConfig {
            period_frames: 256,
            buffer_frames: 256,
            ..AlsaInputConfig::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn zero_slots_fail_closed() {
        let config = AlsaInputConfig {
            channels: 0,
            ..AlsaInputConfig::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires AURORA_ALSA_INPUT_DEVICE and a real Linux direct-eARC capture endpoint"]
    fn integration_open_and_capture_one_block() {
        let device = std::env::var("AURORA_ALSA_INPUT_DEVICE")
            .expect("set AURORA_ALSA_INPUT_DEVICE to a real ALSA capture endpoint");
        let mut capture = NativeAlsaCapture::open(AlsaInputConfig {
            device,
            ..AlsaInputConfig::default()
        })
        .unwrap();
        let channels = capture.telemetry().channels;
        let block = capture.read_block().unwrap();
        assert!(block.frame_count > 0);
        assert_eq!(block.interleaved_s32.len(), block.frame_count * channels);
    }
}
