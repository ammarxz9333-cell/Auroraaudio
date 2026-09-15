//! Aurora-owned real-time audio backend traits.

mod network;

pub use network::*;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Real-time audio sample representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealTimeSampleFormat {
    /// Interleaved 32-bit floating-point samples.
    F32,
}

/// Direction of a local audio device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioDeviceDirection {
    /// Capture/input device.
    Input,
    /// Playback/output device.
    Output,
}

/// Aurora-owned device identity descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDeviceDescriptor {
    /// Backend family, such as `cpal-wasapi`.
    pub backend: String,
    /// Human-readable device name supplied by the host.
    pub device_name: String,
    /// Input or output direction.
    pub direction: AudioDeviceDirection,
    /// Host identifier when the backend exposes one.
    pub host_identifier: Option<String>,
    /// Default channel count reported by the host.
    pub channel_count: Option<usize>,
    /// Default sample rate reported by the host.
    pub default_sample_rate: Option<u32>,
}

impl AudioDeviceDescriptor {
    /// Returns a deterministic selector derived from available host metadata.
    ///
    /// CPAL does not expose the Windows endpoint GUID, so a name-based selector
    /// can change when the device is renamed and can collide for duplicate names.
    pub fn stable_selector(&self) -> String {
        let direction = match self.direction {
            AudioDeviceDirection::Input => "input",
            AudioDeviceDirection::Output => "output",
        };
        let (identity_kind, identity) = self.host_identifier.as_ref().map_or_else(
            || ("name", self.device_name.as_str()),
            |host_identifier| ("host", host_identifier.as_str()),
        );
        format!(
            "{}:{}:{}:{}",
            self.backend,
            direction,
            identity_kind,
            normalize_selector_component(identity)
        )
    }

    /// Returns a descriptive selector including current format hints.
    ///
    /// Exact matching first uses the stable identity portion. Channel count and
    /// sample rate help users disambiguate devices but are treated as hints
    /// because CPAL cannot expose a Windows endpoint GUID.
    pub fn selector(&self) -> String {
        format!(
            "{}:channels:{}:rate:{}",
            self.stable_selector(),
            self.channel_count
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
            self.default_sample_rate
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
        )
    }
}

/// Public device description returned by Aurora backends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    /// Temporary run-local numeric identifier retained for CLI compatibility.
    pub id: String,
    /// Human-readable device name.
    pub name: String,
    /// Input or output direction.
    pub direction: AudioDeviceDirection,
    /// Default sample rate when the backend reports one.
    pub default_sample_rate: Option<u32>,
    /// Maximum supported channels when known.
    pub max_channels: Option<usize>,
    /// Aurora-owned identity descriptor.
    pub descriptor: AudioDeviceDescriptor,
}

/// Requested local real-time stream configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealTimeAudioConfig {
    /// Common requested sample rate in hertz when no direction override is set.
    pub sample_rate: u32,
    /// Optional input-device sample-rate override.
    pub input_sample_rate: Option<u32>,
    /// Optional output-device sample-rate override.
    pub output_sample_rate: Option<u32>,
    /// Requested callback block size in frames.
    pub block_size: usize,
    /// Requested input channel count.
    pub input_channels: usize,
    /// Requested output channel count.
    pub output_channels: usize,
    /// Requested sample format.
    pub sample_format: RealTimeSampleFormat,
    /// Optional backend-local input device id.
    pub input_device_id: Option<String>,
    /// Optional backend-local output device id.
    pub output_device_id: Option<String>,
}

impl RealTimeAudioConfig {
    /// Returns the requested input-device rate.
    pub fn requested_input_rate(&self) -> u32 {
        self.input_sample_rate.unwrap_or(self.sample_rate)
    }

    /// Returns the requested output-device rate.
    pub fn requested_output_rate(&self) -> u32 {
        self.output_sample_rate.unwrap_or(self.sample_rate)
    }
}

/// Negotiated stream configuration reported by a backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiatedAudioConfig {
    /// Negotiated sample rate.
    pub sample_rate: u32,
    /// Callback block size requested from the backend.
    pub requested_block_size: usize,
    /// Negotiated channel count.
    pub channels: usize,
    /// Negotiated sample format.
    pub sample_format: RealTimeSampleFormat,
    /// Device period when the backend exposes it.
    pub device_period_frames: Option<usize>,
    /// Device-reported stream latency when the backend exposes it.
    pub device_reported_latency_frames: Option<usize>,
}

/// Numeric stream fault published by a backend callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum AudioStreamFault {
    /// Stream has not reported a fault.
    #[default]
    None = 0,
    /// Device became unavailable.
    DeviceLost = 1,
    /// Host callback reported another stream failure.
    Callback = 2,
    /// Device format changed or became incompatible.
    FormatChanged = 3,
}

/// Errors returned by real-time audio backends.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RealTimeAudioError {
    /// Device enumeration failed.
    #[error("device enumeration failed: {0}")]
    DeviceEnumeration(String),
    /// Requested device was not found.
    #[error("device not found: {0}")]
    DeviceNotFound(String),
    /// A fuzzy selector matched multiple devices and cannot be chosen safely.
    #[error("ambiguous audio device selector: {0}")]
    AmbiguousDeviceSelector(String),
    /// Requested format is unsupported.
    #[error("unsupported audio format: {0}")]
    UnsupportedFormat(String),
    /// Device was lost or invalidated.
    #[error("audio device lost: {0}")]
    DeviceLost(String),
    /// Stream lifecycle operation failed.
    #[error("audio stream error: {0}")]
    Stream(String),
    /// Callback reported an error.
    #[error("audio callback error: {0}")]
    Callback(String),
}

/// Resolves an Aurora selector with exact-first, collision-safe matching.
pub fn resolve_device_selector<'a>(
    devices: &'a [AudioDeviceInfo],
    selector: &str,
    direction: AudioDeviceDirection,
) -> Result<&'a AudioDeviceInfo, RealTimeAudioError> {
    let candidates = devices
        .iter()
        .filter(|device| device.direction == direction)
        .collect::<Vec<_>>();
    if selector.chars().all(|character| character.is_ascii_digit()) {
        if let Some(device) = candidates.iter().find(|device| device.id == selector) {
            return Ok(device);
        }
    }
    let exact_identity = candidates
        .iter()
        .filter(|device| {
            device.descriptor.selector() == selector
                || device.descriptor.stable_selector() == selector
        })
        .copied()
        .collect::<Vec<_>>();
    match exact_identity.as_slice() {
        [device] => return Ok(*device),
        [] => {}
        _ => {
            return Err(RealTimeAudioError::AmbiguousDeviceSelector(
                selector.to_owned(),
            ))
        }
    }
    let exact_names = candidates
        .iter()
        .filter(|device| device.name.eq_ignore_ascii_case(selector))
        .copied()
        .collect::<Vec<_>>();
    match exact_names.as_slice() {
        [device] => return Ok(*device),
        [] => {}
        _ => {
            return Err(RealTimeAudioError::AmbiguousDeviceSelector(
                selector.to_owned(),
            ))
        }
    }
    let normalized = normalize_selector_component(selector);
    let fuzzy = candidates
        .into_iter()
        .filter(|device| {
            normalize_selector_component(&device.name).contains(&normalized)
                || device.descriptor.selector().contains(&normalized)
        })
        .collect::<Vec<_>>();
    match fuzzy.as_slice() {
        [device] => Ok(*device),
        [] => Err(RealTimeAudioError::DeviceNotFound(selector.to_owned())),
        _ => Err(RealTimeAudioError::AmbiguousDeviceSelector(
            selector.to_owned(),
        )),
    }
}

/// Output callback that fills an interleaved f32 block.
pub type OutputCallback = Box<dyn FnMut(&mut [f32], usize) + Send + 'static>;

/// Input callback that receives an interleaved f32 block.
pub type InputCallback = Box<dyn FnMut(&[f32], usize) + Send + 'static>;

/// Open input stream controlled through Aurora-owned methods.
pub trait AudioInputStream {
    /// Starts the stream.
    fn start(&mut self) -> Result<(), RealTimeAudioError>;
    /// Stops the stream.
    fn stop(&mut self) -> Result<(), RealTimeAudioError>;
    /// Returns the negotiated stream configuration.
    fn negotiated_config(&self) -> &NegotiatedAudioConfig;
    /// Returns device-reported latency when the backend exposes it.
    fn device_reported_latency_frames(&self) -> Option<usize>;
    /// Returns the latest numeric backend fault.
    fn fault(&self) -> AudioStreamFault;
}

/// Open output stream controlled through Aurora-owned methods.
pub trait AudioOutputStream {
    /// Starts the stream.
    fn start(&mut self) -> Result<(), RealTimeAudioError>;
    /// Stops the stream.
    fn stop(&mut self) -> Result<(), RealTimeAudioError>;
    /// Returns the negotiated stream configuration.
    fn negotiated_config(&self) -> &NegotiatedAudioConfig;
    /// Returns device-reported latency when the backend exposes it.
    fn device_reported_latency_frames(&self) -> Option<usize>;
    /// Returns the latest numeric backend fault.
    fn fault(&self) -> AudioStreamFault;
}

/// Audio input backend boundary.
pub trait AudioInputBackend {
    /// Lists available input devices.
    fn enumerate_devices(&self) -> Result<Vec<AudioDeviceInfo>, RealTimeAudioError>;
    /// Opens an input stream.
    fn open_input(
        &self,
        config: &RealTimeAudioConfig,
        callback: InputCallback,
    ) -> Result<Box<dyn AudioInputStream>, RealTimeAudioError>;
    /// Reports backend latency before stream opening when available.
    fn device_reported_latency_frames(&self) -> Option<usize>;
}

/// Audio output backend boundary.
pub trait AudioOutputBackend {
    /// Lists available output devices.
    fn enumerate_devices(&self) -> Result<Vec<AudioDeviceInfo>, RealTimeAudioError>;
    /// Opens an output stream.
    fn open_output(
        &self,
        config: &RealTimeAudioConfig,
        callback: OutputCallback,
    ) -> Result<Box<dyn AudioOutputStream>, RealTimeAudioError>;
    /// Reports backend latency before stream opening when available.
    fn device_reported_latency_frames(&self) -> Option<usize>;
}

/// Returns a frame duration for timing calculations.
pub fn block_duration(sample_rate: u32, block_size: usize) -> Duration {
    if sample_rate == 0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(block_size as f64 / f64::from(sample_rate))
}

fn normalize_selector_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_device_descriptor_serializes_and_round_trips() {
        let descriptor = AudioDeviceDescriptor {
            backend: "cpal-wasapi".to_owned(),
            device_name: "USB Audio Output".to_owned(),
            direction: AudioDeviceDirection::Output,
            host_identifier: None,
            channel_count: Some(8),
            default_sample_rate: Some(48_000),
        };
        let json = serde_json::to_string(&descriptor).unwrap();
        let decoded: AudioDeviceDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, descriptor);
        assert_eq!(
            descriptor.stable_selector(),
            "cpal-wasapi:output:name:usb-audio-output"
        );
        assert_eq!(
            descriptor.selector(),
            "cpal-wasapi:output:name:usb-audio-output:channels:8:rate:48000"
        );
    }

    #[test]
    fn fuzzy_selector_rejects_ambiguous_devices() {
        let devices = [
            device("0", "USB Audio Input"),
            device("1", "USB Audio Backup"),
        ];
        let error = resolve_device_selector(&devices, "USB Audio", AudioDeviceDirection::Input)
            .unwrap_err();
        assert_eq!(
            error,
            RealTimeAudioError::AmbiguousDeviceSelector("USB Audio".to_owned())
        );
        assert_eq!(
            resolve_device_selector(&devices, "USB Audio Input", AudioDeviceDirection::Input)
                .unwrap()
                .id,
            "0"
        );
    }

    fn device(id: &str, name: &str) -> AudioDeviceInfo {
        AudioDeviceInfo {
            id: id.to_owned(),
            name: name.to_owned(),
            direction: AudioDeviceDirection::Input,
            default_sample_rate: Some(48_000),
            max_channels: Some(2),
            descriptor: AudioDeviceDescriptor {
                backend: "cpal-wasapi".to_owned(),
                device_name: name.to_owned(),
                direction: AudioDeviceDirection::Input,
                host_identifier: None,
                channel_count: Some(2),
                default_sample_rate: Some(48_000),
            },
        }
    }
}
