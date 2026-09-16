//! Worker-side six-talker GenAVB runtime for Aurora immersive output.
//!
//! This layer joins the AVDECC control wrapper, the six-stream identity/session
//! gate, and six AVDECC-driven GenAVB talkers. It performs network/control work
//! only on a worker thread. Aurora's realtime callback remains outside this
//! boundary and publishes bounded PCM blocks through the existing worker path.

use std::fmt;
use std::path::{Path, PathBuf};

use aurora_network_genavb::{GenAvbAdapterLoadError, GenAvbNetworkTransport};
use aurora_network_genavb_avdecc::{
    GenAvbAvdeccControl, GenAvbAvdeccError, GenAvbAvdeccEventKind, GenAvbAvdeccLoadError,
};
use aurora_network_genavb_session::{
    ExpectedStream, SessionError, SixStreamAvdeccSession, StereoEndpointRole,
    IMMERSIVE_STREAM_COUNT,
};
use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport,
    NetworkClockDiscipline, NetworkStreamConfig, NetworkTransportError, AURORA_NETWORK_MEDIA_RATE,
};

const REQUIRED_CHANNELS: usize = 2;
const REQUIRED_BLOCK_FRAMES: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    Loaded,
    AwaitingConnections,
    Ready,
    Started,
    NeedsReprepare,
    Faulted,
}

/// Six timestamp-identical stereo PCM blocks in canonical Aurora pair order:
/// Front, Center/LFE, Surround, Back Surround, Top Front, Top Rear.
pub struct GenAvbStereoBlockSet<'a> {
    pub sequence: u64,
    pub timestamp: MediaTimestamp,
    pub format: NetworkAudioFormat,
    pub samples: [&'a [f32]; IMMERSIVE_STREAM_COUNT],
}

impl GenAvbStereoBlockSet<'_> {
    fn validate(&self) -> Result<(), GenAvbRuntimeError> {
        if self.format.sample_rate != AURORA_NETWORK_MEDIA_RATE
            || self.format.channels != REQUIRED_CHANNELS
            || self.format.block_frames != REQUIRED_BLOCK_FRAMES
            || self.timestamp.sample_rate != self.format.sample_rate
        {
            return Err(GenAvbRuntimeError::InvalidBlockSet);
        }
        let required = self
            .format
            .samples_per_block()
            .ok_or(GenAvbRuntimeError::InvalidBlockSet)?;
        if self.samples.iter().any(|samples| samples.len() != required) {
            return Err(GenAvbRuntimeError::InvalidBlockSet);
        }
        Ok(())
    }
}

/// One accepted AVDECC connection-state transition after runtime processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenAvbRuntimeUpdate {
    pub role: StereoEndpointRole,
    pub connected: bool,
    pub session_complete: bool,
    pub ready: bool,
    pub stopped_active_set: bool,
}

#[derive(Debug)]
pub enum GenAvbRuntimeLoadError {
    InvalidConfig,
    Control(GenAvbAvdeccLoadError),
    Session(SessionError),
    Talker {
        endpoint: usize,
        source: GenAvbAdapterLoadError,
    },
}

impl fmt::Display for GenAvbRuntimeLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig => formatter.write_str("invalid six-talker GenAVB runtime config"),
            Self::Control(source) => write!(formatter, "GenAVB AVDECC control load failed: {source}"),
            Self::Session(source) => write!(formatter, "GenAVB AVDECC session setup failed: {source}"),
            Self::Talker { endpoint, source } => {
                write!(formatter, "GenAVB talker {endpoint} load failed: {source}")
            }
        }
    }
}

impl std::error::Error for GenAvbRuntimeLoadError {}

#[derive(Debug)]
pub enum GenAvbRuntimeError {
    InvalidLifecycle,
    InvalidBlockSet,
    Control(GenAvbAvdeccError),
    Session(SessionError),
    Endpoint {
        endpoint: usize,
        source: NetworkTransportError,
    },
}

impl fmt::Display for GenAvbRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLifecycle => formatter.write_str("invalid six-talker GenAVB lifecycle"),
            Self::InvalidBlockSet => formatter.write_str("invalid six-talker stereo PCM block set"),
            Self::Control(source) => write!(formatter, "GenAVB AVDECC control failed: {source}"),
            Self::Session(source) => write!(formatter, "GenAVB AVDECC session failed: {source}"),
            Self::Endpoint { endpoint, source } => {
                write!(formatter, "GenAVB talker {endpoint} failed: {source}")
            }
        }
    }
}

impl std::error::Error for GenAvbRuntimeError {}

/// One worker-owned AVDECC control channel plus six GenAVB AAF talkers.
pub struct GenAvbSixTalkerRuntime {
    control: GenAvbAvdeccControl,
    session: SixStreamAvdeccSession,
    talkers: [GenAvbNetworkTransport; IMMERSIVE_STREAM_COUNT],
    stream_config: NetworkStreamConfig,
    prepared_epoch: [bool; IMMERSIVE_STREAM_COUNT],
    lifecycle: Lifecycle,
}

impl GenAvbSixTalkerRuntime {
    pub fn load(
        shim_path: impl Into<PathBuf>,
        expected_streams: [ExpectedStream; IMMERSIVE_STREAM_COUNT],
        stream_config: NetworkStreamConfig,
    ) -> Result<Self, GenAvbRuntimeLoadError> {
        validate_stream_config(stream_config)?;
        let shim_path = shim_path.into();
        let session = SixStreamAvdeccSession::new(expected_streams)
            .map_err(GenAvbRuntimeLoadError::Session)?;
        let control = GenAvbAvdeccControl::load(shim_path.clone())
            .map_err(GenAvbRuntimeLoadError::Control)?;

        let talker0 = load_talker(&shim_path, 0)?;
        let talker1 = load_talker(&shim_path, 1)?;
        let talker2 = load_talker(&shim_path, 2)?;
        let talker3 = load_talker(&shim_path, 3)?;
        let talker4 = load_talker(&shim_path, 4)?;
        let talker5 = load_talker(&shim_path, 5)?;

        Ok(Self {
            control,
            session,
            talkers: [talker0, talker1, talker2, talker3, talker4, talker5],
            stream_config,
            prepared_epoch: [false; IMMERSIVE_STREAM_COUNT],
            lifecycle: Lifecycle::Loaded,
        })
    }

    /// Opens the AVDECC media-stack control channel and begins a fresh epoch.
    pub fn open(&mut self) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle != Lifecycle::Loaded {
            return Err(GenAvbRuntimeError::InvalidLifecycle);
        }
        self.control.open().map_err(GenAvbRuntimeError::Control)?;
        self.session.reset();
        self.clear_prepared_epoch();
        self.lifecycle = Lifecycle::AwaitingConnections;
        Ok(())
    }

    pub fn poll_fd(&self) -> Result<i32, GenAvbRuntimeError> {
        self.control.poll_fd().map_err(GenAvbRuntimeError::Control)
    }

    /// Processes one readable AVDECC control indication.
    ///
    /// Once the sixth canonical CONNECT is accepted, all six talkers are
    /// transactionally prepared from the current AVDECC cache. A disconnect
    /// invalidates the whole prepare epoch; reconnect therefore causes a fresh
    /// six-talker prepare before start eligibility returns.
    pub fn receive_one(&mut self) -> Result<Option<GenAvbRuntimeUpdate>, GenAvbRuntimeError> {
        if matches!(self.lifecycle, Lifecycle::Loaded | Lifecycle::Faulted) {
            return Err(GenAvbRuntimeError::InvalidLifecycle);
        }

        let event = match self.control.receive() {
            Ok(Some(event)) => event,
            Ok(None) => return Ok(None),
            Err(source) => {
                self.fault_control_epoch();
                return Err(GenAvbRuntimeError::Control(source));
            }
        };
        let kind = event.kind;
        let update = match self.session.apply(event) {
            Ok(update) => update,
            Err(source) => {
                self.fault_control_epoch();
                return Err(GenAvbRuntimeError::Session(source));
            }
        };

        let mut stopped_active_set = false;
        match kind {
            GenAvbAvdeccEventKind::Connect => {
                if update.complete {
                    if let Err(error) = self.prepare_complete_set() {
                        self.lifecycle = Lifecycle::NeedsReprepare;
                        return Err(error);
                    }
                    self.lifecycle = Lifecycle::Ready;
                } else if self.lifecycle != Lifecycle::Started {
                    self.lifecycle = Lifecycle::AwaitingConnections;
                }
            }
            GenAvbAvdeccEventKind::Disconnect => {
                if self.lifecycle == Lifecycle::Started {
                    if let Some(error) = self.stop_all_best_effort() {
                        self.clear_prepared_epoch();
                        self.lifecycle = Lifecycle::Faulted;
                        return Err(error);
                    }
                    stopped_active_set = true;
                }
                self.clear_prepared_epoch();
                self.lifecycle = Lifecycle::AwaitingConnections;
            }
        }

        Ok(Some(GenAvbRuntimeUpdate {
            role: update.role,
            connected: update.connected,
            session_complete: update.complete,
            ready: self.lifecycle == Lifecycle::Ready,
            stopped_active_set,
        }))
    }

    /// Re-prepares all six currently connected streams after a transport fault.
    pub fn reprepare_connected(&mut self) -> Result<(), GenAvbRuntimeError> {
        if matches!(
            self.lifecycle,
            Lifecycle::Loaded | Lifecycle::Started | Lifecycle::Faulted
        ) {
            return Err(GenAvbRuntimeError::InvalidLifecycle);
        }
        self.prepare_complete_set()?;
        self.lifecycle = Lifecycle::Ready;
        Ok(())
    }

    /// Starts all six current-epoch talkers. Partial start is rolled back.
    pub fn start(&mut self) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle != Lifecycle::Ready
            || !self.session.is_complete()
            || self.prepared_epoch.iter().any(|prepared| !prepared)
        {
            return Err(GenAvbRuntimeError::InvalidLifecycle);
        }

        for endpoint in 0..IMMERSIVE_STREAM_COUNT {
            if let Err(source) = self.talkers[endpoint].start() {
                for started in 0..endpoint {
                    let _ = self.talkers[started].stop();
                    self.talkers[started].reset();
                }
                self.lifecycle = Lifecycle::Ready;
                return Err(GenAvbRuntimeError::Endpoint { endpoint, source });
            }
        }
        self.lifecycle = Lifecycle::Started;
        Ok(())
    }

    /// Submits six stereo blocks that share one Aurora sequence/timestamp.
    /// Any one-talker failure stops the set and requires `reprepare_connected()`.
    pub fn submit(&mut self, blocks: &GenAvbStereoBlockSet<'_>) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle != Lifecycle::Started {
            return Err(GenAvbRuntimeError::InvalidLifecycle);
        }
        blocks.validate()?;
        if blocks.format != self.stream_config.format {
            return Err(GenAvbRuntimeError::InvalidBlockSet);
        }

        for endpoint in 0..IMMERSIVE_STREAM_COUNT {
            let block = NetworkAudioBlock {
                sequence: blocks.sequence,
                timestamp: blocks.timestamp,
                format: blocks.format,
                samples: blocks.samples[endpoint],
            };
            if let Err(source) = self.talkers[endpoint].submit(block) {
                let _ = self.stop_all_best_effort();
                for talker in &mut self.talkers {
                    talker.reset();
                }
                self.clear_prepared_epoch();
                self.lifecycle = Lifecycle::NeedsReprepare;
                return Err(GenAvbRuntimeError::Endpoint { endpoint, source });
            }
        }
        Ok(())
    }

    /// Stops all six talkers while retaining the current AVDECC connection set.
    pub fn stop(&mut self) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle != Lifecycle::Started {
            return Err(GenAvbRuntimeError::InvalidLifecycle);
        }
        if let Some(error) = self.stop_all_best_effort() {
            self.clear_prepared_epoch();
            self.lifecycle = Lifecycle::NeedsReprepare;
            return Err(error);
        }
        self.lifecycle = Lifecycle::Ready;
        Ok(())
    }

    /// Closes the control channel and clears all session/epoch eligibility.
    pub fn close(&mut self) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle == Lifecycle::Loaded {
            return Ok(());
        }
        if self.lifecycle == Lifecycle::Started {
            let _ = self.stop_all_best_effort();
        }
        self.control.close().map_err(GenAvbRuntimeError::Control)?;
        self.session.reset();
        self.clear_prepared_epoch();
        for talker in &mut self.talkers {
            talker.reset();
        }
        self.lifecycle = Lifecycle::Loaded;
        Ok(())
    }

    /// Reopens the AVDECC control channel after a fatal control/session fault.
    /// Native close clears cached stream parameters, so fresh CONNECT state is
    /// required before the six-stream session can become ready again.
    pub fn recover_control(&mut self) -> Result<(), GenAvbRuntimeError> {
        if self.lifecycle == Lifecycle::Started {
            let _ = self.stop_all_best_effort();
        }
        let _ = self.control.close();
        self.session.reset();
        self.clear_prepared_epoch();
        for talker in &mut self.talkers {
            talker.reset();
        }
        self.control.open().map_err(GenAvbRuntimeError::Control)?;
        self.lifecycle = Lifecycle::AwaitingConnections;
        Ok(())
    }

    pub const fn connected_count(&self) -> usize {
        self.session.connected_count()
    }

    pub fn prepared_count(&self) -> usize {
        self.prepared_epoch.iter().filter(|prepared| **prepared).count()
    }

    pub const fn is_ready(&self) -> bool {
        matches!(self.lifecycle, Lifecycle::Ready | Lifecycle::Started)
    }

    pub const fn is_started(&self) -> bool {
        self.lifecycle == Lifecycle::Started
    }

    fn prepare_complete_set(&mut self) -> Result<(), GenAvbRuntimeError> {
        let streams = self
            .session
            .require_complete()
            .map_err(GenAvbRuntimeError::Session)?;
        self.clear_prepared_epoch();

        for (endpoint, stream) in streams.iter().enumerate() {
            debug_assert_eq!(role_slot(stream.role), endpoint);
            if let Err(source) = self.talkers[endpoint].prepare_from_avdecc(
                &self.control,
                stream.stream_index,
                self.stream_config,
            ) {
                for prepared in 0..endpoint {
                    self.talkers[prepared].reset();
                }
                self.clear_prepared_epoch();
                return Err(GenAvbRuntimeError::Endpoint { endpoint, source });
            }
            self.prepared_epoch[endpoint] = true;
        }
        Ok(())
    }

    fn stop_all_best_effort(&mut self) -> Option<GenAvbRuntimeError> {
        let mut first_error = None;
        for endpoint in 0..IMMERSIVE_STREAM_COUNT {
            if let Err(source) = self.talkers[endpoint].stop() {
                if first_error.is_none() {
                    first_error = Some(GenAvbRuntimeError::Endpoint { endpoint, source });
                }
            }
            self.talkers[endpoint].reset();
        }
        first_error
    }

    fn clear_prepared_epoch(&mut self) {
        self.prepared_epoch = [false; IMMERSIVE_STREAM_COUNT];
    }

    fn fault_control_epoch(&mut self) {
        if self.lifecycle == Lifecycle::Started {
            let _ = self.stop_all_best_effort();
        }
        self.clear_prepared_epoch();
        self.lifecycle = Lifecycle::Faulted;
    }
}

fn load_talker(
    shim_path: &Path,
    endpoint: usize,
) -> Result<GenAvbNetworkTransport, GenAvbRuntimeLoadError> {
    GenAvbNetworkTransport::load_avdecc(shim_path.to_path_buf())
        .map_err(|source| GenAvbRuntimeLoadError::Talker { endpoint, source })
}

fn validate_stream_config(config: NetworkStreamConfig) -> Result<(), GenAvbRuntimeLoadError> {
    if config.format.sample_rate != AURORA_NETWORK_MEDIA_RATE
        || config.format.channels != REQUIRED_CHANNELS
        || config.format.block_frames != REQUIRED_BLOCK_FRAMES
        || config.clock_discipline != NetworkClockDiscipline::PtpFollower
        || config.timing.minimum_latency_frames != config.timing.target_latency_frames
        || config.timing.maximum_latency_frames != config.timing.target_latency_frames
        || config.timing.target_latency_frames == 0
        || config.timing.maximum_rate_correction_ppm != 0.0
        || config.timing.validate().is_err()
    {
        return Err(GenAvbRuntimeLoadError::InvalidConfig);
    }
    Ok(())
}

const fn role_slot(role: StereoEndpointRole) -> usize {
    match role {
        StereoEndpointRole::Front => 0,
        StereoEndpointRole::CenterLfe => 1,
        StereoEndpointRole::Surround => 2,
        StereoEndpointRole::BackSurround => 3,
        StereoEndpointRole::TopFront => 4,
        StereoEndpointRole::TopRear => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_realtime_audio_api::NetworkTimingPolicy;

    fn valid_config() -> NetworkStreamConfig {
        NetworkStreamConfig {
            format: NetworkAudioFormat {
                sample_rate: AURORA_NETWORK_MEDIA_RATE,
                channels: REQUIRED_CHANNELS,
                block_frames: REQUIRED_BLOCK_FRAMES,
            },
            clock_discipline: NetworkClockDiscipline::PtpFollower,
            timing: NetworkTimingPolicy {
                target_latency_frames: 480,
                minimum_latency_frames: 480,
                maximum_latency_frames: 480,
                maximum_rate_correction_ppm: 0.0,
            },
        }
    }

    #[test]
    fn runtime_config_is_exact_and_fail_closed() {
        assert!(validate_stream_config(valid_config()).is_ok());
        let mut invalid = valid_config();
        invalid.format.block_frames = 256;
        assert!(matches!(
            validate_stream_config(invalid),
            Err(GenAvbRuntimeLoadError::InvalidConfig)
        ));
    }

    #[test]
    fn stereo_block_set_requires_six_exact_stereo_blocks() {
        let samples = [0.0_f32; REQUIRED_CHANNELS * REQUIRED_BLOCK_FRAMES];
        let set = GenAvbStereoBlockSet {
            sequence: 7,
            timestamp: MediaTimestamp::new(9_600, AURORA_NETWORK_MEDIA_RATE).unwrap(),
            format: valid_config().format,
            samples: [&samples; IMMERSIVE_STREAM_COUNT],
        };
        assert!(set.validate().is_ok());

        let short = [0.0_f32; 2];
        let mut bad_samples = [&samples[..]; IMMERSIVE_STREAM_COUNT];
        bad_samples[4] = &short;
        let bad = GenAvbStereoBlockSet {
            samples: bad_samples,
            ..set
        };
        assert!(matches!(bad.validate(), Err(GenAvbRuntimeError::InvalidBlockSet)));
    }

    #[test]
    fn canonical_role_slots_match_session_order() {
        assert_eq!(role_slot(StereoEndpointRole::Front), 0);
        assert_eq!(role_slot(StereoEndpointRole::CenterLfe), 1);
        assert_eq!(role_slot(StereoEndpointRole::Surround), 2);
        assert_eq!(role_slot(StereoEndpointRole::BackSurround), 3);
        assert_eq!(role_slot(StereoEndpointRole::TopFront), 4);
        assert_eq!(role_slot(StereoEndpointRole::TopRear), 5);
    }
}
