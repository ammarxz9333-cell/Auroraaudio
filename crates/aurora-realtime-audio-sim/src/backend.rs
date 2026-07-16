use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};

use aurora_realtime_audio_api::{
    resolve_device_selector, AudioDeviceDescriptor, AudioDeviceDirection, AudioDeviceInfo,
    AudioInputBackend, AudioInputStream, AudioOutputBackend, AudioOutputStream, AudioStreamFault,
    InputCallback, NegotiatedAudioConfig, OutputCallback, RealTimeAudioConfig, RealTimeAudioError,
    RealTimeSampleFormat,
};

use crate::{SimulationProfile, VirtualDevice, VirtualSampleFormat};

/// Aurora backend-trait implementation backed by deterministic virtual devices.
#[derive(Debug, Clone)]
pub struct SimAudioBackend {
    profile: SimulationProfile,
}

impl SimAudioBackend {
    /// Creates a backend exposing the profile's input and output endpoints.
    pub fn new(profile: SimulationProfile) -> Self {
        Self { profile }
    }

    fn infos(&self) -> Vec<AudioDeviceInfo> {
        [&self.profile.input, &self.profile.output]
            .into_iter()
            .chain(self.profile.additional_devices.iter())
            .map(device_info)
            .collect()
    }

    fn select(
        &self,
        direction: AudioDeviceDirection,
        selector: Option<&str>,
    ) -> Result<&VirtualDevice, RealTimeAudioError> {
        let default_device = match direction {
            AudioDeviceDirection::Input => &self.profile.input,
            AudioDeviceDirection::Output => &self.profile.output,
        };
        if let Some(selector) = selector {
            let infos = self.infos();
            let selected = resolve_device_selector(&infos, selector, direction)?;
            return [&self.profile.input, &self.profile.output]
                .into_iter()
                .chain(self.profile.additional_devices.iter())
                .find(|device| device.id == selected.id)
                .ok_or_else(|| RealTimeAudioError::DeviceNotFound(selector.to_owned()));
        }
        Ok(default_device)
    }
}

impl AudioInputBackend for SimAudioBackend {
    fn enumerate_devices(&self) -> Result<Vec<AudioDeviceInfo>, RealTimeAudioError> {
        Ok(self
            .infos()
            .into_iter()
            .filter(|device| device.direction == AudioDeviceDirection::Input)
            .collect())
    }

    fn open_input(
        &self,
        config: &RealTimeAudioConfig,
        callback: InputCallback,
    ) -> Result<Box<dyn AudioInputStream>, RealTimeAudioError> {
        let device = self.select(
            AudioDeviceDirection::Input,
            config.input_device_id.as_deref(),
        )?;
        validate(
            device,
            config.requested_input_rate(),
            config.input_channels,
            config.sample_format,
        )?;
        Ok(Box::new(SimInputStream {
            common: SimStream::new(
                device,
                config.requested_input_rate(),
                config.input_channels,
                config.block_size,
            ),
            _callback: callback,
        }))
    }

    fn device_reported_latency_frames(&self) -> Option<usize> {
        Some(self.profile.input.latency_frames)
    }
}

impl AudioOutputBackend for SimAudioBackend {
    fn enumerate_devices(&self) -> Result<Vec<AudioDeviceInfo>, RealTimeAudioError> {
        Ok(self
            .infos()
            .into_iter()
            .filter(|device| device.direction == AudioDeviceDirection::Output)
            .collect())
    }

    fn open_output(
        &self,
        config: &RealTimeAudioConfig,
        callback: OutputCallback,
    ) -> Result<Box<dyn AudioOutputStream>, RealTimeAudioError> {
        let device = self.select(
            AudioDeviceDirection::Output,
            config.output_device_id.as_deref(),
        )?;
        validate(
            device,
            config.requested_output_rate(),
            config.output_channels,
            config.sample_format,
        )?;
        Ok(Box::new(SimOutputStream {
            common: SimStream::new(
                device,
                config.requested_output_rate(),
                config.output_channels,
                config.block_size,
            ),
            _callback: callback,
        }))
    }

    fn device_reported_latency_frames(&self) -> Option<usize> {
        Some(self.profile.output.latency_frames)
    }
}

fn device_info(device: &VirtualDevice) -> AudioDeviceInfo {
    let rate = device.supported_sample_rates.first().copied();
    let channels = device.supported_channel_counts.iter().copied().max();
    AudioDeviceInfo {
        id: device.id.clone(),
        name: device.name.clone(),
        direction: device.direction,
        default_sample_rate: rate,
        max_channels: channels,
        descriptor: AudioDeviceDescriptor {
            backend: "aurora-sim".to_owned(),
            device_name: device.name.clone(),
            direction: device.direction,
            host_identifier: Some(device.id.clone()),
            channel_count: channels,
            default_sample_rate: rate,
        },
    }
}

fn validate(
    device: &VirtualDevice,
    rate: u32,
    channels: usize,
    format: RealTimeSampleFormat,
) -> Result<(), RealTimeAudioError> {
    if format != RealTimeSampleFormat::F32
        || !device
            .supported_sample_formats
            .contains(&VirtualSampleFormat::F32)
        || !device.supported_sample_rates.contains(&rate)
        || !device.supported_channel_counts.contains(&channels)
    {
        return Err(RealTimeAudioError::UnsupportedFormat(format!(
            "virtual device {} does not support f32 {} Hz with {} channels",
            device.id, rate, channels
        )));
    }
    Ok(())
}

struct SimStream {
    negotiated: NegotiatedAudioConfig,
    running: AtomicBool,
    fault: Arc<AtomicU32>,
}

impl SimStream {
    fn new(device: &VirtualDevice, sample_rate: u32, channels: usize, block_size: usize) -> Self {
        Self {
            negotiated: NegotiatedAudioConfig {
                sample_rate,
                requested_block_size: block_size,
                channels,
                sample_format: RealTimeSampleFormat::F32,
                device_period_frames: Some(device.callback_size.maximum_frames()),
                device_reported_latency_frames: Some(device.latency_frames),
            },
            running: AtomicBool::new(false),
            fault: Arc::new(AtomicU32::new(AudioStreamFault::None as u32)),
        }
    }

    fn start(&self) -> Result<(), RealTimeAudioError> {
        self.running.store(true, Ordering::Release);
        Ok(())
    }
    fn stop(&self) -> Result<(), RealTimeAudioError> {
        self.running.store(false, Ordering::Release);
        Ok(())
    }
    fn fault(&self) -> AudioStreamFault {
        match self.fault.load(Ordering::Acquire) {
            1 => AudioStreamFault::DeviceLost,
            2 => AudioStreamFault::Callback,
            3 => AudioStreamFault::FormatChanged,
            _ => AudioStreamFault::None,
        }
    }
}

struct SimInputStream {
    common: SimStream,
    _callback: InputCallback,
}
struct SimOutputStream {
    common: SimStream,
    _callback: OutputCallback,
}

impl AudioInputStream for SimInputStream {
    fn start(&mut self) -> Result<(), RealTimeAudioError> {
        self.common.start()
    }
    fn stop(&mut self) -> Result<(), RealTimeAudioError> {
        self.common.stop()
    }
    fn negotiated_config(&self) -> &NegotiatedAudioConfig {
        &self.common.negotiated
    }
    fn device_reported_latency_frames(&self) -> Option<usize> {
        self.common.negotiated.device_reported_latency_frames
    }
    fn fault(&self) -> AudioStreamFault {
        self.common.fault()
    }
}

impl AudioOutputStream for SimOutputStream {
    fn start(&mut self) -> Result<(), RealTimeAudioError> {
        self.common.start()
    }
    fn stop(&mut self) -> Result<(), RealTimeAudioError> {
        self.common.stop()
    }
    fn negotiated_config(&self) -> &NegotiatedAudioConfig {
        &self.common.negotiated
    }
    fn device_reported_latency_frames(&self) -> Option<usize> {
        self.common.negotiated.device_reported_latency_frames
    }
    fn fault(&self) -> AudioStreamFault {
        self.common.fault()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin_profile;

    #[test]
    fn backend_negotiates_supported_format_and_rejects_other_formats() {
        let backend = SimAudioBackend::new(builtin_profile("usb-7-1").unwrap());
        let mut config = RealTimeAudioConfig {
            sample_rate: 48_000,
            input_sample_rate: None,
            output_sample_rate: None,
            block_size: 256,
            input_channels: 8,
            output_channels: 8,
            sample_format: RealTimeSampleFormat::F32,
            input_device_id: None,
            output_device_id: None,
        };
        let stream = backend.open_input(&config, Box::new(|_, _| {})).unwrap();
        assert_eq!(stream.negotiated_config().channels, 8);
        config.input_sample_rate = Some(44_100);
        assert!(matches!(
            backend.open_input(&config, Box::new(|_, _| {})),
            Err(RealTimeAudioError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn broken_profile_rejects_ambiguous_output_name() {
        let backend = SimAudioBackend::new(builtin_profile("broken-driver").unwrap());
        let config = RealTimeAudioConfig {
            sample_rate: 48_000,
            input_sample_rate: None,
            output_sample_rate: None,
            block_size: 256,
            input_channels: 2,
            output_channels: 2,
            sample_format: RealTimeSampleFormat::F32,
            input_device_id: None,
            output_device_id: Some("Duplicate USB Audio".to_owned()),
        };
        assert!(matches!(
            backend.open_output(&config, Box::new(|_, _| {})),
            Err(RealTimeAudioError::AmbiguousDeviceSelector(_))
        ));
    }
}
