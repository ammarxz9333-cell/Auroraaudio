use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use aurora_realtime_audio_api::{
    resolve_device_selector, AudioDeviceDescriptor, AudioDeviceDirection, AudioDeviceInfo,
    AudioInputBackend, AudioInputStream, AudioOutputBackend, AudioOutputStream, AudioStreamFault,
    InputCallback, NegotiatedAudioConfig, OutputCallback, RealTimeAudioConfig, RealTimeAudioError,
    RealTimeSampleFormat,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

#[derive(Debug, Default, Clone)]
pub struct CpalAudioBackend;

impl CpalAudioBackend {
    pub fn new() -> Self {
        Self
    }
}

impl AudioInputBackend for CpalAudioBackend {
    fn enumerate_devices(&self) -> Result<Vec<AudioDeviceInfo>, RealTimeAudioError> {
        enumerate(AudioDeviceDirection::Input)
    }

    fn open_input(
        &self,
        config: &RealTimeAudioConfig,
        mut callback: InputCallback,
    ) -> Result<Box<dyn AudioInputStream>, RealTimeAudioError> {
        let host = cpal::default_host();
        let device = select_device(
            &host,
            AudioDeviceDirection::Input,
            config.input_device_id.as_deref(),
        )?;
        let sample_rate = config.requested_input_rate();
        validate_supported(
            &device,
            AudioDeviceDirection::Input,
            sample_rate,
            config.input_channels,
        )?;
        let stream_config = cpal::StreamConfig {
            channels: config.input_channels as u16,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Fixed(config.block_size as u32),
        };
        let callback_channels = stream_config.channels as usize;
        let negotiated = NegotiatedAudioConfig {
            sample_rate,
            requested_block_size: config.block_size,
            channels: config.input_channels,
            sample_format: RealTimeSampleFormat::F32,
            device_period_frames: None,
            device_reported_latency_frames: None,
        };
        let fault = Arc::new(AtomicU32::new(AudioStreamFault::None as u32));
        let fault_for_callback = Arc::clone(&fault);
        let stream = device
            .build_input_stream(
                &stream_config,
                move |data: &[f32], _| callback(data, callback_channels),
                move |error| {
                    fault_for_callback.store(stream_error_fault(error) as u32, Ordering::Release);
                },
                None,
            )
            .map_err(|error| RealTimeAudioError::Stream(error.to_string()))?;
        Ok(Box::new(CpalStream {
            stream,
            negotiated,
            fault,
            running: false,
        }))
    }

    fn device_reported_latency_frames(&self) -> Option<usize> {
        None
    }
}

impl AudioOutputBackend for CpalAudioBackend {
    fn enumerate_devices(&self) -> Result<Vec<AudioDeviceInfo>, RealTimeAudioError> {
        enumerate(AudioDeviceDirection::Output)
    }

    fn open_output(
        &self,
        config: &RealTimeAudioConfig,
        mut callback: OutputCallback,
    ) -> Result<Box<dyn AudioOutputStream>, RealTimeAudioError> {
        if config.sample_format != RealTimeSampleFormat::F32 {
            return Err(RealTimeAudioError::UnsupportedFormat(
                "only f32 output is supported".to_owned(),
            ));
        }
        let host = cpal::default_host();
        let device = select_device(
            &host,
            AudioDeviceDirection::Output,
            config.output_device_id.as_deref(),
        )?;
        let sample_rate = config.requested_output_rate();
        validate_supported(
            &device,
            AudioDeviceDirection::Output,
            sample_rate,
            config.output_channels,
        )?;
        let stream_config = cpal::StreamConfig {
            channels: config.output_channels as u16,
            sample_rate: cpal::SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Fixed(config.block_size as u32),
        };
        let callback_channels = stream_config.channels as usize;
        let negotiated = NegotiatedAudioConfig {
            sample_rate,
            requested_block_size: config.block_size,
            channels: config.output_channels,
            sample_format: RealTimeSampleFormat::F32,
            device_period_frames: None,
            device_reported_latency_frames: None,
        };
        let fault = Arc::new(AtomicU32::new(AudioStreamFault::None as u32));
        let fault_for_callback = Arc::clone(&fault);
        let stream = device
            .build_output_stream(
                &stream_config,
                move |data: &mut [f32], _| callback(data, callback_channels),
                move |error| {
                    fault_for_callback.store(stream_error_fault(error) as u32, Ordering::Release);
                },
                None,
            )
            .map_err(|error| RealTimeAudioError::Stream(error.to_string()))?;
        Ok(Box::new(CpalStream {
            stream,
            negotiated,
            fault,
            running: false,
        }))
    }

    fn device_reported_latency_frames(&self) -> Option<usize> {
        None
    }
}

struct CpalStream {
    stream: cpal::Stream,
    negotiated: NegotiatedAudioConfig,
    fault: Arc<AtomicU32>,
    running: bool,
}

impl CpalStream {
    fn current_fault(&self) -> AudioStreamFault {
        match self.fault.load(Ordering::Acquire) {
            value if value == AudioStreamFault::DeviceLost as u32 => AudioStreamFault::DeviceLost,
            value if value == AudioStreamFault::FormatChanged as u32 => AudioStreamFault::FormatChanged,
            value if value == AudioStreamFault::Callback as u32 => AudioStreamFault::Callback,
            _ => AudioStreamFault::None,
        }
    }

    fn start_inner(&mut self) -> Result<(), RealTimeAudioError> {
        if self.current_fault() != AudioStreamFault::None {
            return Err(RealTimeAudioError::DeviceLost(
                "backend error callback was invoked".to_owned(),
            ));
        }
        self.stream
            .play()
            .map_err(|error| RealTimeAudioError::Stream(error.to_string()))?;
        self.running = true;
        Ok(())
    }

    fn stop_inner(&mut self) -> Result<(), RealTimeAudioError> {
        if self.running {
            self.stream
                .pause()
                .map_err(|error| RealTimeAudioError::Stream(error.to_string()))?;
        }
        self.running = false;
        Ok(())
    }
}

impl AudioInputStream for CpalStream {
    fn start(&mut self) -> Result<(), RealTimeAudioError> { self.start_inner() }
    fn stop(&mut self) -> Result<(), RealTimeAudioError> { self.stop_inner() }
    fn negotiated_config(&self) -> &NegotiatedAudioConfig { &self.negotiated }
    fn device_reported_latency_frames(&self) -> Option<usize> { self.negotiated.device_reported_latency_frames }
    fn fault(&self) -> AudioStreamFault { self.current_fault() }
}

impl AudioOutputStream for CpalStream {
    fn start(&mut self) -> Result<(), RealTimeAudioError> { self.start_inner() }
    fn stop(&mut self) -> Result<(), RealTimeAudioError> { self.stop_inner() }
    fn negotiated_config(&self) -> &NegotiatedAudioConfig { &self.negotiated }
    fn device_reported_latency_frames(&self) -> Option<usize> { self.negotiated.device_reported_latency_frames }
    fn fault(&self) -> AudioStreamFault { self.current_fault() }
}

fn enumerate(direction: AudioDeviceDirection) -> Result<Vec<AudioDeviceInfo>, RealTimeAudioError> {
    let host = cpal::default_host();
    let devices = match direction {
        AudioDeviceDirection::Input => host.input_devices(),
        AudioDeviceDirection::Output => host.output_devices(),
    }
    .map_err(|error| RealTimeAudioError::DeviceEnumeration(error.to_string()))?;
    devices
        .enumerate()
        .map(|(index, device)| device_info(index, &device, direction))
        .collect()
}

fn device_info(
    index: usize,
    device: &cpal::Device,
    direction: AudioDeviceDirection,
) -> Result<AudioDeviceInfo, RealTimeAudioError> {
    let name = device.name().unwrap_or_else(|_| "Unknown device".to_owned());
    let default_config = match direction {
        AudioDeviceDirection::Input => device.default_input_config().ok(),
        AudioDeviceDirection::Output => device.default_output_config().ok(),
    };
    let backend = if cfg!(target_os = "windows") { "cpal-wasapi" } else { "cpal" };
    let descriptor = AudioDeviceDescriptor {
        backend: backend.to_owned(),
        device_name: name.clone(),
        direction,
        host_identifier: None,
        channel_count: default_config.as_ref().map(|config| config.channels() as usize),
        default_sample_rate: default_config.as_ref().map(|config| config.sample_rate().0),
    };
    Ok(AudioDeviceInfo {
        id: index.to_string(),
        name,
        direction,
        default_sample_rate: default_config.as_ref().map(|config| config.sample_rate().0),
        max_channels: default_config.as_ref().map(|config| config.channels() as usize),
        descriptor,
    })
}

fn select_device(
    host: &cpal::Host,
    direction: AudioDeviceDirection,
    id: Option<&str>,
) -> Result<cpal::Device, RealTimeAudioError> {
    if let Some(id) = id {
        let devices = match direction {
            AudioDeviceDirection::Input => host.input_devices(),
            AudioDeviceDirection::Output => host.output_devices(),
        }
        .map_err(|error| RealTimeAudioError::DeviceEnumeration(error.to_string()))?
        .collect::<Vec<_>>();
        let infos = devices
            .iter()
            .enumerate()
            .map(|(index, device)| device_info(index, device, direction))
            .collect::<Result<Vec<_>, _>>()?;
        let selected = resolve_device_selector(&infos, id, direction)?;
        let index = selected.id.parse::<usize>().map_err(|_| RealTimeAudioError::DeviceNotFound(id.to_owned()))?;
        devices.into_iter().nth(index).ok_or_else(|| RealTimeAudioError::DeviceNotFound(id.to_owned()))
    } else {
        match direction {
            AudioDeviceDirection::Input => host.default_input_device(),
            AudioDeviceDirection::Output => host.default_output_device(),
        }
        .ok_or_else(|| RealTimeAudioError::DeviceNotFound("default".to_owned()))
    }
}

fn stream_error_fault(error: cpal::StreamError) -> AudioStreamFault {
    match error {
        cpal::StreamError::DeviceNotAvailable => AudioStreamFault::DeviceLost,
        cpal::StreamError::BackendSpecific { .. } => AudioStreamFault::Callback,
    }
}

fn validate_supported(
    device: &cpal::Device,
    direction: AudioDeviceDirection,
    sample_rate: u32,
    channels: usize,
) -> Result<(), RealTimeAudioError> {
    let supported = match direction {
        AudioDeviceDirection::Input => device.supported_input_configs(),
        AudioDeviceDirection::Output => device.supported_output_configs(),
    }
    .map_err(|error| RealTimeAudioError::DeviceEnumeration(error.to_string()))?;
    for range in supported {
        if range.min_sample_rate().0 <= sample_rate
            && sample_rate <= range.max_sample_rate().0
            && usize::from(range.channels()) >= channels
            && range.sample_format() == cpal::SampleFormat::F32
        {
            return Ok(());
        }
    }
    Err(RealTimeAudioError::UnsupportedFormat(format!(
        "{channels} channels at {sample_rate} Hz f32 is not supported"
    )))
}
