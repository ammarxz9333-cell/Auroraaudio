use aurora_realtime_audio_api::AudioDeviceDirection;
use serde::{Deserialize, Serialize};

/// Sample formats a virtual endpoint can advertise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VirtualSampleFormat {
    /// IEEE 32-bit floating point.
    F32,
    /// Signed 16-bit PCM.
    I16,
    /// Signed packed 24-bit PCM.
    I24,
    /// Signed 32-bit PCM.
    I32,
}

/// Deterministic callback frame-size model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CallbackSizePolicy {
    /// Every callback contains the same frame count.
    Fixed {
        /// Frames emitted by each callback.
        frames: usize,
    },
    /// Callback sizes rotate through two values.
    Alternating {
        /// Frames emitted by even-numbered callbacks.
        first: usize,
        /// Frames emitted by odd-numbered callbacks.
        second: usize,
    },
    /// Callback sizes are sampled uniformly from an inclusive bounded range.
    RandomBounded {
        /// Smallest callback size, in frames.
        minimum: usize,
        /// Largest callback size, in frames.
        maximum: usize,
    },
}

impl CallbackSizePolicy {
    /// Returns the largest callback this policy can emit.
    pub fn maximum_frames(&self) -> usize {
        match *self {
            Self::Fixed { frames } => frames,
            Self::Alternating { first, second } => first.max(second),
            Self::RandomBounded { maximum, .. } => maximum,
        }
    }

    pub(crate) fn frames(&self, index: u64, random: u64) -> usize {
        match *self {
            Self::Fixed { frames } => frames,
            Self::Alternating { first, second } => {
                if index % 2 == 0 {
                    first
                } else {
                    second
                }
            }
            Self::RandomBounded { minimum, maximum } => {
                let span = maximum.saturating_sub(minimum).saturating_add(1);
                minimum.saturating_add(random as usize % span.max(1))
            }
        }
        .max(1)
    }
}

/// One deterministic virtual audio endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualDevice {
    /// Stable simulator-owned endpoint identifier.
    pub id: String,
    /// Human-readable endpoint name.
    pub name: String,
    /// Capture or playback direction.
    pub direction: AudioDeviceDirection,
    /// Accepted nominal rates.
    pub supported_sample_rates: Vec<u32>,
    /// Accepted channel counts.
    pub supported_channel_counts: Vec<usize>,
    /// Formats advertised during negotiation.
    pub supported_sample_formats: Vec<VirtualSampleFormat>,
    /// Nominal endpoint latency.
    pub latency_frames: usize,
    /// Callback-size behavior.
    pub callback_size: CallbackSizePolicy,
    /// Device clock offset in parts per million.
    pub clock_ppm: i32,
    /// Uniform callback timing jitter bound in frames.
    pub callback_jitter_frames: usize,
}

/// Scripted simulator fault action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FaultAction {
    /// Capture endpoint disappears.
    InputLoss,
    /// Playback endpoint disappears.
    OutputLoss,
    /// A backend callback reports a numeric stream fault.
    CallbackError,
    /// Negotiated format becomes unsupported.
    FormatChange,
    /// Both callback domains stop temporarily.
    StreamFreeze,
    /// One or more callbacks are intentionally omitted.
    MissingCallbacks,
    /// The callback scheduler stalls for the event duration.
    SchedulingStall,
    /// Only the virtual input callback domain stalls.
    InputCallbackStall,
    /// Only the virtual output callback domain stalls.
    OutputCallbackStall,
    /// Back-to-back callbacks are emitted.
    CallbackBurst,
    /// Input clock changes by `value` ppm.
    ClockJump,
    /// Callback size changes to `value` frames.
    CallbackSizeChange,
}

/// One point on a deterministic fault timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaultEvent {
    /// Event time in simulated milliseconds.
    pub at_milliseconds: u64,
    /// Injected action.
    pub action: FaultAction,
    /// Optional action duration before recovery.
    pub duration_milliseconds: u64,
    /// Action-specific numeric value.
    pub value: i32,
}

/// Complete virtual hardware profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationProfile {
    /// Stable CLI profile name.
    pub name: String,
    /// Virtual input endpoint.
    pub input: VirtualDevice,
    /// Virtual output endpoint.
    pub output: VirtualDevice,
    /// Default deterministic faults.
    pub faults: Vec<FaultEvent>,
    /// Extra endpoints used for collision and recovery scenarios.
    pub additional_devices: Vec<VirtualDevice>,
}

/// Returns a built-in virtual hardware profile.
pub fn builtin_profile(name: &str) -> Option<SimulationProfile> {
    let fixed = |frames| CallbackSizePolicy::Fixed { frames };
    let device = |id: &str,
                  label: &str,
                  direction,
                  rates: &[u32],
                  channels: &[usize],
                  latency,
                  callback_size,
                  clock_ppm,
                  jitter| VirtualDevice {
        id: id.to_owned(),
        name: label.to_owned(),
        direction,
        supported_sample_rates: rates.to_vec(),
        supported_channel_counts: channels.to_vec(),
        supported_sample_formats: vec![VirtualSampleFormat::F32],
        latency_frames: latency,
        callback_size,
        clock_ppm,
        callback_jitter_frames: jitter,
    };
    let (input, output, faults) = match name {
        "stereo-consumer" => (
            device(
                "sim-stereo-in",
                "Aurora Stereo",
                AudioDeviceDirection::Input,
                &[48_000],
                &[2],
                192,
                fixed(256),
                50,
                0,
            ),
            device(
                "sim-stereo-out",
                "Aurora Stereo",
                AudioDeviceDirection::Output,
                &[48_000],
                &[2],
                192,
                fixed(256),
                -50,
                0,
            ),
            vec![],
        ),
        "usb-5-1" => (
            device(
                "sim-usb51-in",
                "Aurora USB 5.1",
                AudioDeviceDirection::Input,
                &[44_100, 48_000, 96_000],
                &[2],
                128,
                CallbackSizePolicy::RandomBounded {
                    minimum: 64,
                    maximum: 512,
                },
                25,
                3,
            ),
            device(
                "sim-usb51-out",
                "Aurora USB 5.1",
                AudioDeviceDirection::Output,
                &[44_100, 48_000, 96_000],
                &[6],
                256,
                CallbackSizePolicy::RandomBounded {
                    minimum: 64,
                    maximum: 512,
                },
                -25,
                3,
            ),
            vec![],
        ),
        "usb-7-1" => (
            device(
                "sim-usb71-in",
                "Aurora USB 7.1",
                AudioDeviceDirection::Input,
                &[48_000, 96_000],
                &[8],
                192,
                CallbackSizePolicy::Alternating {
                    first: 128,
                    second: 384,
                },
                12,
                2,
            ),
            device(
                "sim-usb71-out",
                "Aurora USB 7.1",
                AudioDeviceDirection::Output,
                &[48_000, 96_000],
                &[8],
                192,
                CallbackSizePolicy::RandomBounded {
                    minimum: 64,
                    maximum: 512,
                },
                -13,
                2,
            ),
            vec![],
        ),
        "development-12" => (
            device(
                "sim-dev12-in",
                "Aurora Development 12",
                AudioDeviceDirection::Input,
                &[48_000],
                &[12],
                128,
                fixed(128),
                5,
                0,
            ),
            device(
                "sim-dev12-out",
                "Aurora Development 12",
                AudioDeviceDirection::Output,
                &[48_000],
                &[12],
                128,
                fixed(128),
                -5,
                0,
            ),
            vec![],
        ),
        "broken-driver" => (
            device(
                "sim-broken-in",
                "Duplicate USB Audio",
                AudioDeviceDirection::Input,
                &[48_000],
                &[2, 8],
                512,
                CallbackSizePolicy::RandomBounded {
                    minimum: 1,
                    maximum: 1024,
                },
                180,
                96,
            ),
            device(
                "sim-broken-out",
                "Duplicate USB Audio",
                AudioDeviceDirection::Output,
                &[48_000],
                &[2, 8],
                512,
                CallbackSizePolicy::RandomBounded {
                    minimum: 1,
                    maximum: 1024,
                },
                -170,
                96,
            ),
            vec![
                FaultEvent {
                    at_milliseconds: 30_000,
                    action: FaultAction::OutputLoss,
                    duration_milliseconds: 5_000,
                    value: 0,
                },
                FaultEvent {
                    at_milliseconds: 60_000,
                    action: FaultAction::StreamFreeze,
                    duration_milliseconds: 500,
                    value: 0,
                },
                FaultEvent {
                    at_milliseconds: 90_000,
                    action: FaultAction::ClockJump,
                    duration_milliseconds: 10_000,
                    value: 300,
                },
            ],
        ),
        _ => return None,
    };
    let additional_devices = if name == "broken-driver" {
        vec![device(
            "sim-broken-out-duplicate",
            "Duplicate USB Audio",
            AudioDeviceDirection::Output,
            &[48_000],
            &[2, 8],
            512,
            CallbackSizePolicy::RandomBounded {
                minimum: 1,
                maximum: 1024,
            },
            -165,
            96,
        )]
    } else {
        vec![]
    };
    Some(SimulationProfile {
        name: name.to_owned(),
        input,
        output,
        faults,
        additional_devices,
    })
}

/// Loads a scripted JSON fault timeline.
pub fn load_fault_timeline(
    path: impl AsRef<std::path::Path>,
) -> Result<Vec<FaultEvent>, std::io::Error> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fault_timelines_parse() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/simulation/fault_scenarios");
        for name in [
            "output_loss.json",
            "input_callback_stall.json",
            "callback_size_change.json",
            "clock_jump.json",
            "unsupported_format.json",
            "repeated_stream_errors.json",
        ] {
            let events = load_fault_timeline(root.join(name)).unwrap();
            assert!(!events.is_empty(), "{name}");
        }
    }
}
