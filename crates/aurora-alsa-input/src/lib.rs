//! Aurora-owned native Linux ALSA/ASoC capture for direct eARC carrier audio.
//!
//! This crate owns only the physical S32_LE capture boundary. It does not
//! parse IEC61937, decode codecs or infer JOC/Atmos capability. Captured slot
//! samples remain bit-preserving signed 32-bit words for the existing Aurora
//! carrier normalizer.

use thiserror::Error;

/// Native ALSA capture configuration for the recovered eARC serial-audio carrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlsaInputConfig {
    /// ALSA PCM capture endpoint, for example `hw:0,0`.
    pub device: String,
    /// Recovered serial-audio carrier frame rate, typically 192 kHz for DD+ IEC61937.
    pub sample_rate: u32,
    /// Physical interleaved serial-audio slots per frame.
    pub channels: usize,
    /// Preferred hardware period size in carrier frames.
    pub period_frames: usize,
    /// Preferred hardware buffer size in carrier frames.
    pub buffer_frames: usize,
}

impl Default for AlsaInputConfig {
    fn default() -> Self {
        Self {
            device: "default".to_owned(),
            sample_rate: 192_000,
            channels: 2,
            period_frames: 256,
            buffer_frames: 1_024,
        }
    }
}

/// One captured interleaved S32_LE block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureBlock {
    /// Interleaved signed 32-bit slot values in ALSA channel order.
    pub interleaved_s32: Vec<i32>,
    /// Number of complete carrier frames in this block.
    pub frame_count: usize,
    /// True when a capture fault was recovered immediately before this block.
    /// Callers must reset partial carrier/decoder state before consuming it.
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
    if config.period_frames == 0 {
        return Err(AlsaInputError::InvalidConfig(
            "period size must be greater than zero".to_owned(),
        ));
    }
    if config.buffer_frames < config.period_frames.saturating_mul(2) {
        return Err(AlsaInputError::InvalidConfig(
            "buffer size must be at least two periods".to_owned(),
        ));
    }
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
}

#[cfg(target_os = "linux")]
impl NativeAlsaCapture {
    /// Opens the ALSA capture endpoint with exact S32_LE/rate/channel semantics.
    /// ALSA resampling is disabled. Period and buffer sizes may negotiate to a
    /// nearby hardware-supported value and are exposed through telemetry.
    pub fn open(config: AlsaInputConfig) -> Result<Self, AlsaInputError> {
        use alsa::pcm::{Access, Format, HwParams};
        use alsa::{Direction, ValueOr};

        validate_config(&config)?;
        let pcm = alsa::pcm::PCM::new(&config.device, Direction::Capture, false)?;
        let hw = HwParams::any(&pcm)?;
        hw.set_rate_resample(false)?;
        hw.set_access(Access::RWInterleaved)?;
        hw.set_format(Format::S32LE)?;
        hw.set_channels(config.channels as u32)?;
        hw.set_rate(config.sample_rate, ValueOr::Nearest)?;
        hw.set_period_size_near(config.period_frames as i64, ValueOr::Nearest)?;
        hw.set_buffer_size_near(config.buffer_frames as i64)?;
        pcm.hw_params(&hw)?;
        drop(hw);

        let current = pcm.hw_params_current()?;
        let negotiated_rate = current.get_rate()?;
        let negotiated_channels = current.get_channels()? as usize;
        let negotiated_format = current.get_format()?;
        let period_frames = current.get_period_size()? as usize;
        let buffer_frames = current.get_buffer_size()? as usize;
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

        let sw = pcm.sw_params_current()?;
        sw.set_avail_min(period_frames as i64)?;
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
        })
    }

    /// Reads one block. Recoverable ALSA faults are repaired in place, and the
    /// next successful block is explicitly marked as a discontinuity.
    pub fn read_block(&mut self) -> Result<CaptureBlock, AlsaInputError> {
        let channels = self.config.channels;
        let requested_frames = self.telemetry.period_frames.max(1);
        let mut samples = vec![0_i32; requested_frames.saturating_mul(channels)];

        loop {
            let result = {
                let io = self.pcm.io_i32()?;
                io.readi(&mut samples)
            };
            match result {
                Ok(0) => return Err(AlsaInputError::NoForwardProgress),
                Ok(frames) => {
                    samples.truncate(frames.saturating_mul(channels));
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
                        interleaved_s32: samples,
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

    pub fn read_block(&mut self) -> Result<CaptureBlock, AlsaInputError> {
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
        let block = capture.read_block().unwrap();
        assert!(block.frame_count > 0);
        assert_eq!(
            block.interleaved_s32.len(),
            block.frame_count * capture.telemetry().channels
        );
    }
}
