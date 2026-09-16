//! Worker-side runtime that binds Aurora's six-stream 7.1.4 fanout to
//! AVDECC-owned NXP GenAVB talkers.
//!
//! This crate is intentionally standalone and platform-gated. It owns the
//! AVDECC control channel, the fail-closed six-stream session gate, six GenAVB
//! talkers and the already-proven Aurora 7.1.4 -> six-stereo fanout. No method
//! here belongs in Aurora's realtime render callback.

use std::fmt;
use std::path::PathBuf;

use aurora_network_esp_avb::{
    EspAvbFanoutError, EspAvbFanoutPlan, PreparedEspAvbFanout, ESP_AVB_7_1_4_ENDPOINTS,
};
use aurora_network_genavb::{GenAvbAdapterLoadError, GenAvbNetworkTransport};
use aurora_network_genavb_avdecc::{
    GenAvbAvdeccControl, GenAvbAvdeccError, GenAvbAvdeccEventKind, GenAvbAvdeccLoadError,
};
use aurora_network_genavb_session::{
    ExpectedStream, SessionError, SessionUpdate, SixStreamAvdeccSession, IMMERSIVE_STREAM_COUNT,
};
use aurora_realtime_audio_api::{
    NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport, NetworkClockDiscipline,
    NetworkStreamConfig, NetworkTimingPolicy, NetworkTransportError, AURORA_NETWORK_MEDIA_RATE,
};

const GENAVB_BLOCK_FRAMES: usize = 48;
const GENAVB_CHANNELS: usize = 2;
const CANONICAL_PAIRS: [[&str; 2]; ESP_AVB_7_1_4_ENDPOINTS] = [
    ["FL", "FR"],
    ["FC", "LFE"],
    ["SL", "SR"],
    ["SBL", "SBR"],
    ["TFL", "TFR"],
    ["TRL", "TRR"],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLifecycle {
    Loaded,
    ControlOpen,
    Prepared,
    Started,
}

#[derive(Debug)]
pub enum GenAvbRuntimeLoadError {
    Control(GenAvbAvdeccLoadError),
    Talker {
        endpoint: usize,
        source: GenAvbAdapterLoadError,
    },
    Session(SessionError),
    Fanout(EspAvbFanoutError),
    InvalidBlockFrames(usize),
    NonCanonicalFanout {
        endpoint: usize,
    },
}

impl fmt::Display for GenAvbRuntimeLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control(source) => {
                write!(formatter, "GenAVB AVDECC control load failed: {source}")
            }
            Self::Talker { endpoint, source } => {
                write!(formatter, "GenAVB talker {endpoint} load failed: {source}")
            }
            Self::Session(source) => {
                write!(formatter, "GenAVB AVDECC session setup failed: {source}")
            }
            Self::Fanout(source) => write!(formatter, "GenAVB fanout setup failed: {source}"),
            Self::InvalidBlockFrames(frames) => {
                write!(
                    formatter,
                    "GenAVB runtime requires 48-frame blocks, got {frames}"
                )
            }
            Self::NonCanonicalFanout { endpoint } => write!(
                formatter,
                "GenAVB endpoint {endpoint} is not the canonical Aurora 7.1.4 stereo pair"
            ),
        }
    }
}

impl std::error::Error for GenAvbRuntimeLoadError {}

#[derive(Debug)]
pub enum GenAvbRuntimeError {
    InvalidLifecycle {
        expected: RuntimeLifecycle,
        actual: RuntimeLifecycle,
    },
    InvalidTiming,
    Control(GenAvbAvdeccError),
    Session(SessionError),
    Fanout(EspAvbFanoutError),
    Endpoint {
        endpoint: usize,
        source: NetworkTransportError,
    },
}

impl fmt::Display for GenAvbRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLifecycle { expected, actual } => {
                write!(
                    formatter,
                    "GenAVB runtime expected {expected:?}, got {actual:?}"
                )
            }
            Self::InvalidTiming => formatter.write_str("invalid GenAVB scheduled latency"),
            Self::Control(source) => write!(formatter, "GenAVB AVDECC control failed: {source}"),
            Self::Session(source) => write!(formatter, "GenAVB AVDECC session failed: {source}"),
            Self::Fanout(source) => write!(formatter, "GenAVB fanout failed: {source}"),
            Self::Endpoint { endpoint, source } => {
                write!(formatter, "GenAVB endpoint {endpoint} failed: {source}")
            }
        }
    }
}

impl std::error::Error for GenAvbRuntimeError {}

/// Concrete worker-side runtime for six AVDECC-owned GenAVB AAF talkers.
pub struct SixStreamGenAvbRuntime {
    control: GenAvbAvdeccControl,
    session: SixStreamAvdeccSession,
    talkers: [GenAvbNetworkTransport; ESP_AVB_7_1_4_ENDPOINTS],
    fanout: PreparedEspAvbFanout,
    endpoint_format: NetworkAudioFormat,
    lifecycle: RuntimeLifecycle,
}

impl SixStreamGenAvbRuntime {
    pub fn load(
        shim_path: impl Into<PathBuf>,
        expected_streams: [ExpectedStream; IMMERSIVE_STREAM_COUNT],
        fanout_plan: EspAvbFanoutPlan,
        block_frames: usize,
    ) -> Result<Self, GenAvbRuntimeLoadError> {
        if block_frames != GENAVB_BLOCK_FRAMES {
            return Err(GenAvbRuntimeLoadError::InvalidBlockFrames(block_frames));
        }
        validate_fanout_order(&fanout_plan)?;

        let shim_path = shim_path.into();
        let control = GenAvbAvdeccControl::load(shim_path.clone())
            .map_err(GenAvbRuntimeLoadError::Control)?;
        let session = SixStreamAvdeccSession::new(expected_streams)
            .map_err(GenAvbRuntimeLoadError::Session)?;
        let fanout = PreparedEspAvbFanout::prepare(fanout_plan, block_frames)
            .map_err(GenAvbRuntimeLoadError::Fanout)?;

        let mut loaded = Vec::with_capacity(ESP_AVB_7_1_4_ENDPOINTS);
        for endpoint in 0..ESP_AVB_7_1_4_ENDPOINTS {
            let talker = GenAvbNetworkTransport::load_avdecc(shim_path.clone())
                .map_err(|source| GenAvbRuntimeLoadError::Talker { endpoint, source })?;
            loaded.push(talker);
        }
        let talkers = match loaded.try_into() {
            Ok(talkers) => talkers,
            Err(_) => unreachable!("exactly six GenAVB talkers are loaded"),
        };

        Ok(Self {
            control,
            session,
            talkers,
            fanout,
            endpoint_format: NetworkAudioFormat {
                sample_rate: AURORA_NETWORK_MEDIA_RATE,
                channels: GENAVB_CHANNELS,
                block_frames,
            },
            lifecycle: RuntimeLifecycle::Loaded,
        })
    }

    pub const fn lifecycle(&self) -> RuntimeLifecycle {
        self.lifecycle
    }

    pub const fn connected_count(&self) -> usize {
        self.session.connected_count()
    }

    pub const fn session_complete(&self) -> bool {
        self.session.is_complete()
    }

    pub fn open_control(&mut self) -> Result<(), GenAvbRuntimeError> {
        match self.lifecycle {
            RuntimeLifecycle::Loaded | RuntimeLifecycle::ControlOpen => {}
            actual => {
                return Err(GenAvbRuntimeError::InvalidLifecycle {
                    expected: RuntimeLifecycle::Loaded,
                    actual,
                })
            }
        }
        self.control.open().map_err(GenAvbRuntimeError::Control)?;
        self.lifecycle = RuntimeLifecycle::ControlOpen;
        Ok(())
    }

    pub fn poll_fd(&self) -> Result<i32, GenAvbRuntimeError> {
        self.control.poll_fd().map_err(GenAvbRuntimeError::Control)
    }

    /// Consumes one AVDECC media-stack indication after the worker observed
    /// readiness on `poll_fd()`.
    ///
    /// Any invalid session transition fails closed. A valid DISCONNECT aborts
    /// all active/prepared audio streams immediately and requires a fresh
    /// six-stream completion + prepare cycle.
    pub fn receive_control(&mut self) -> Result<Option<SessionUpdate>, GenAvbRuntimeError> {
        if self.lifecycle == RuntimeLifecycle::Loaded {
            return Err(GenAvbRuntimeError::InvalidLifecycle {
                expected: RuntimeLifecycle::ControlOpen,
                actual: self.lifecycle,
            });
        }

        let event = match self
            .control
            .receive()
            .map_err(GenAvbRuntimeError::Control)?
        {
            Some(event) => event,
            None => return Ok(None),
        };
        let kind = event.kind;
        let update = match self.session.apply(event) {
            Ok(update) => update,
            Err(source) => {
                self.abort_data_plane();
                self.session.reset();
                self.lifecycle = RuntimeLifecycle::ControlOpen;
                return Err(GenAvbRuntimeError::Session(source));
            }
        };

        if kind == GenAvbAvdeccEventKind::Disconnect {
            self.abort_data_plane();
            self.lifecycle = RuntimeLifecycle::ControlOpen;
        }
        Ok(Some(update))
    }

    /// Prepares all six GenAVB talkers from the exact AVDECC CONNECT parameters.
    pub fn prepare_complete(
        &mut self,
        scheduled_latency_frames: u32,
    ) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle != RuntimeLifecycle::ControlOpen {
            return Err(GenAvbRuntimeError::InvalidLifecycle {
                expected: RuntimeLifecycle::ControlOpen,
                actual: self.lifecycle,
            });
        }
        let connected = self
            .session
            .require_complete()
            .map_err(GenAvbRuntimeError::Session)?;
        let config = endpoint_config(self.endpoint_format, scheduled_latency_frames)?;

        for endpoint in 0..ESP_AVB_7_1_4_ENDPOINTS {
            if let Err(source) = self.talkers[endpoint].prepare_from_avdecc(
                &self.control,
                connected[endpoint].stream_index,
                config,
            ) {
                self.abort_data_plane();
                self.lifecycle = RuntimeLifecycle::ControlOpen;
                return Err(GenAvbRuntimeError::Endpoint { endpoint, source });
            }
        }

        self.fanout.reset();
        self.lifecycle = RuntimeLifecycle::Prepared;
        Ok(())
    }

    /// Starts all six talkers as one fail-closed stream epoch.
    pub fn start(&mut self) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle != RuntimeLifecycle::Prepared {
            return Err(GenAvbRuntimeError::InvalidLifecycle {
                expected: RuntimeLifecycle::Prepared,
                actual: self.lifecycle,
            });
        }

        for endpoint in 0..ESP_AVB_7_1_4_ENDPOINTS {
            if let Err(source) = self.talkers[endpoint].start() {
                self.abort_data_plane();
                self.lifecycle = RuntimeLifecycle::ControlOpen;
                return Err(GenAvbRuntimeError::Endpoint { endpoint, source });
            }
        }

        self.fanout.reset();
        self.lifecycle = RuntimeLifecycle::Started;
        Ok(())
    }

    /// Splits one canonical Aurora 7.1.4 block and submits six timestamp-identical
    /// stereo AAF blocks. Any endpoint failure aborts the whole stream epoch.
    pub fn submit(&mut self, block: &NetworkAudioBlock<'_>) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle != RuntimeLifecycle::Started {
            return Err(GenAvbRuntimeError::InvalidLifecycle {
                expected: RuntimeLifecycle::Started,
                actual: self.lifecycle,
            });
        }

        if let Err(source) = self.fanout.split(block) {
            self.abort_data_plane();
            self.lifecycle = RuntimeLifecycle::ControlOpen;
            return Err(GenAvbRuntimeError::Fanout(source));
        }
        for endpoint in 0..ESP_AVB_7_1_4_ENDPOINTS {
            let endpoint_block = self
                .fanout
                .endpoint_block(endpoint)
                .map_err(GenAvbRuntimeError::Fanout)?;
            if let Err(source) = self.talkers[endpoint].submit(endpoint_block) {
                self.abort_data_plane();
                self.lifecycle = RuntimeLifecycle::ControlOpen;
                return Err(GenAvbRuntimeError::Endpoint { endpoint, source });
            }
        }
        Ok(())
    }

    /// Stops the active epoch while preserving the AVDECC-owned prepared streams.
    pub fn stop(&mut self) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle != RuntimeLifecycle::Started {
            return Err(GenAvbRuntimeError::InvalidLifecycle {
                expected: RuntimeLifecycle::Started,
                actual: self.lifecycle,
            });
        }

        let mut first_error = None;
        for endpoint in 0..ESP_AVB_7_1_4_ENDPOINTS {
            if let Err(source) = self.talkers[endpoint].stop() {
                if first_error.is_none() {
                    first_error = Some(GenAvbRuntimeError::Endpoint { endpoint, source });
                }
            }
        }
        self.fanout.reset();
        self.lifecycle = RuntimeLifecycle::Prepared;

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Invalidates the current data-plane preparation without dropping AVDECC.
    pub fn reset_data_plane(&mut self) {
        self.abort_data_plane();
        if self.lifecycle != RuntimeLifecycle::Loaded {
            self.lifecycle = RuntimeLifecycle::ControlOpen;
        }
    }

    pub fn close_control(&mut self) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle != RuntimeLifecycle::Loaded {
            self.abort_data_plane();
            self.session.reset();
            self.control.close().map_err(GenAvbRuntimeError::Control)?;
            self.lifecycle = RuntimeLifecycle::Loaded;
        }
        Ok(())
    }

    fn abort_data_plane(&mut self) {
        for talker in &mut self.talkers {
            let _ = talker.stop();
            talker.reset();
        }
        self.fanout.reset();
    }
}

fn endpoint_config(
    format: NetworkAudioFormat,
    scheduled_latency_frames: u32,
) -> Result<NetworkStreamConfig, GenAvbRuntimeError> {
    if scheduled_latency_frames == 0 {
        return Err(GenAvbRuntimeError::InvalidTiming);
    }
    let timing = NetworkTimingPolicy {
        target_latency_frames: scheduled_latency_frames,
        minimum_latency_frames: scheduled_latency_frames,
        maximum_latency_frames: scheduled_latency_frames,
        maximum_rate_correction_ppm: 0.0,
    };
    timing
        .validate()
        .map_err(|_| GenAvbRuntimeError::InvalidTiming)?;
    Ok(NetworkStreamConfig {
        format,
        clock_discipline: NetworkClockDiscipline::PtpFollower,
        timing,
    })
}

fn validate_fanout_order(plan: &EspAvbFanoutPlan) -> Result<(), GenAvbRuntimeLoadError> {
    plan.validate().map_err(GenAvbRuntimeLoadError::Fanout)?;
    for (endpoint, expected) in CANONICAL_PAIRS.iter().enumerate() {
        let actual = plan
            .role_pair(endpoint)
            .map_err(GenAvbRuntimeLoadError::Fanout)?;
        if &actual != expected {
            return Err(GenAvbRuntimeLoadError::NonCanonicalFanout { endpoint });
        }
    }
    Ok(())
}

impl Drop for SixStreamGenAvbRuntime {
    fn drop(&mut self) {
        let _ = self.close_control();
    }
}

#[cfg(test)]
mod tests {
    use aurora_network_esp_avb::{EspAvbEndpointMedium, EspAvbFanoutPlan};

    use super::*;

    #[test]
    fn canonical_fanout_order_is_required() {
        let canonical = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
        assert!(validate_fanout_order(&canonical).is_ok());

        let mut swapped = canonical;
        swapped.endpoints.swap(0, 1);
        assert!(matches!(
            validate_fanout_order(&swapped),
            Err(GenAvbRuntimeLoadError::NonCanonicalFanout { endpoint: 0 })
        ));
    }

    #[test]
    fn endpoint_timing_is_fixed_and_ptp_disciplined() {
        let format = NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: GENAVB_CHANNELS,
            block_frames: GENAVB_BLOCK_FRAMES,
        };
        assert!(matches!(
            endpoint_config(format, 0),
            Err(GenAvbRuntimeError::InvalidTiming)
        ));

        let config = endpoint_config(format, 480).unwrap();
        assert_eq!(config.format, format);
        assert_eq!(config.clock_discipline, NetworkClockDiscipline::PtpFollower);
        assert_eq!(config.timing.target_latency_frames, 480);
        assert_eq!(config.timing.minimum_latency_frames, 480);
        assert_eq!(config.timing.maximum_latency_frames, 480);
        assert_eq!(config.timing.maximum_rate_correction_ppm, 0.0);
    }
}
