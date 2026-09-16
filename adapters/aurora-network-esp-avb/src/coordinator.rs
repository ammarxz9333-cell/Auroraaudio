use std::fmt;

use aurora_realtime_audio_api::{
    NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport, NetworkClockDiscipline,
    NetworkStreamConfig, NetworkTimingPolicy, NetworkTransportError, NetworkTransportEvent,
    NetworkTransportFamily, AURORA_NETWORK_MEDIA_RATE,
};

use crate::{
    EspAvbFanoutError, EspAvbFanoutPlan, PreparedEspAvbFanout, ESP_AVB_7_1_4_ENDPOINTS,
    ESP_AVB_CHANNELS_PER_STREAM,
};

/// The six worker-side transports required by the current stereo-per-stream
/// ESP-AVB topology.
pub type EspAvbTransportArray = [Box<dyn NetworkAudioTransport>; ESP_AVB_7_1_4_ENDPOINTS];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    New,
    Prepared,
    Started,
}

/// Aurora-side coordinator for six synchronized stereo AVB transports.
///
/// A concrete host AVB sender can implement `NetworkAudioTransport` and be
/// inserted here without leaking ESP-IDF or upstream AVB types into Aurora's
/// public contracts. All endpoints stay on one Aurora media timeline, and any
/// endpoint fault invalidates the whole stream rather than silently allowing
/// one speaker pair to diverge.
pub struct EspAvbTransportSet {
    fanout: PreparedEspAvbFanout,
    transports: EspAvbTransportArray,
    endpoint_format: NetworkAudioFormat,
    lifecycle: Lifecycle,
}

impl EspAvbTransportSet {
    /// Validates endpoint capabilities and allocates worker-side fanout storage.
    pub fn new(
        plan: EspAvbFanoutPlan,
        transports: EspAvbTransportArray,
        block_frames: usize,
    ) -> Result<Self, EspAvbTransportSetError> {
        plan.validate().map_err(EspAvbTransportSetError::Fanout)?;
        let fanout = PreparedEspAvbFanout::prepare(plan, block_frames)
            .map_err(EspAvbTransportSetError::Fanout)?;
        let endpoint_format = NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: ESP_AVB_CHANNELS_PER_STREAM,
            block_frames,
        };

        for (endpoint, transport) in transports.iter().enumerate() {
            let capabilities = transport.capabilities();
            if capabilities.family != NetworkTransportFamily::AvbTsn
                || capabilities.max_channels < ESP_AVB_CHANNELS_PER_STREAM
                || !capabilities.scheduled_playout
                || capabilities.adaptive_rate_matching
            {
                return Err(EspAvbTransportSetError::Capability { endpoint });
            }
        }

        Ok(Self {
            fanout,
            transports,
            endpoint_format,
            lifecycle: Lifecycle::New,
        })
    }

    /// Prepares all endpoints with one stereo format and PTP-follower policy.
    pub fn prepare(
        &mut self,
        scheduled_latency_frames: u32,
    ) -> Result<(), EspAvbTransportSetError> {
        if self.lifecycle == Lifecycle::Started {
            return Err(EspAvbTransportSetError::InvalidLifecycle);
        }
        if scheduled_latency_frames == 0 {
            return Err(EspAvbTransportSetError::InvalidTiming);
        }

        let timing = NetworkTimingPolicy {
            target_latency_frames: scheduled_latency_frames,
            minimum_latency_frames: scheduled_latency_frames,
            maximum_latency_frames: scheduled_latency_frames,
            maximum_rate_correction_ppm: 0.0,
        };
        timing
            .validate()
            .map_err(|_| EspAvbTransportSetError::InvalidTiming)?;
        let config = NetworkStreamConfig {
            format: self.endpoint_format,
            clock_discipline: NetworkClockDiscipline::PtpFollower,
            timing,
        };

        for endpoint in 0..ESP_AVB_7_1_4_ENDPOINTS {
            if let Err(source) = self.transports[endpoint].prepare(config) {
                for prepared in 0..endpoint {
                    self.transports[prepared].reset();
                }
                self.lifecycle = Lifecycle::New;
                self.fanout.reset();
                return Err(EspAvbTransportSetError::Endpoint { endpoint, source });
            }
        }

        self.lifecycle = Lifecycle::Prepared;
        self.fanout.reset();
        Ok(())
    }

    /// Starts all endpoints and rolls back a partial start immediately.
    pub fn start(&mut self) -> Result<(), EspAvbTransportSetError> {
        if self.lifecycle != Lifecycle::Prepared {
            return Err(EspAvbTransportSetError::InvalidLifecycle);
        }

        for endpoint in 0..ESP_AVB_7_1_4_ENDPOINTS {
            if let Err(source) = self.transports[endpoint].start() {
                for started in 0..endpoint {
                    let _ = self.transports[started].stop();
                    self.transports[started].reset();
                }
                self.lifecycle = Lifecycle::Prepared;
                self.fanout.reset();
                return Err(EspAvbTransportSetError::Endpoint { endpoint, source });
            }
        }

        self.lifecycle = Lifecycle::Started;
        self.fanout.reset();
        Ok(())
    }

    /// Splits one 7.1.4 block and submits six timestamp-identical stereo blocks.
    ///
    /// Any endpoint submission failure aborts every endpoint and requires an
    /// explicit prepare/start cycle before additional audio is accepted.
    pub fn submit(&mut self, block: &NetworkAudioBlock<'_>) -> Result<(), EspAvbTransportSetError> {
        if self.lifecycle != Lifecycle::Started {
            return Err(EspAvbTransportSetError::InvalidLifecycle);
        }

        self.fanout
            .split(block)
            .map_err(EspAvbTransportSetError::Fanout)?;
        for endpoint in 0..ESP_AVB_7_1_4_ENDPOINTS {
            let endpoint_block = self
                .fanout
                .endpoint_block(endpoint)
                .map_err(EspAvbTransportSetError::Fanout)?;
            if let Err(source) = self.transports[endpoint].submit(endpoint_block) {
                self.abort_stream();
                return Err(EspAvbTransportSetError::Endpoint { endpoint, source });
            }
        }
        Ok(())
    }

    /// Polls at most one fixed event per endpoint without temporary allocation.
    pub fn poll_events(
        &mut self,
        output: &mut [Option<NetworkTransportEvent>; ESP_AVB_7_1_4_ENDPOINTS],
    ) {
        for (slot, transport) in output.iter_mut().zip(self.transports.iter_mut()) {
            *slot = transport.poll_event();
        }
    }

    /// Stops all active endpoints while retaining prepared backend capacity.
    pub fn stop(&mut self) -> Result<(), EspAvbTransportSetError> {
        if self.lifecycle != Lifecycle::Started {
            return Err(EspAvbTransportSetError::InvalidLifecycle);
        }

        let mut first_error = None;
        for endpoint in 0..ESP_AVB_7_1_4_ENDPOINTS {
            if let Err(source) = self.transports[endpoint].stop() {
                if first_error.is_none() {
                    first_error = Some(EspAvbTransportSetError::Endpoint { endpoint, source });
                }
            }
            self.transports[endpoint].reset();
        }
        self.lifecycle = Lifecycle::Prepared;
        self.fanout.reset();

        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Clears endpoint and fanout timeline history without network I/O.
    pub fn reset(&mut self) {
        for transport in &mut self.transports {
            transport.reset();
        }
        self.fanout.reset();
    }

    fn abort_stream(&mut self) {
        for transport in &mut self.transports {
            let _ = transport.stop();
            transport.reset();
        }
        self.lifecycle = Lifecycle::New;
        self.fanout.reset();
    }
}

/// Fail-closed errors for six-endpoint AVB orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EspAvbTransportSetError {
    /// Channel/stream fanout plan is invalid.
    Fanout(EspAvbFanoutError),
    /// One endpoint advertises an incompatible transport capability.
    Capability { endpoint: usize },
    /// One endpoint failed an Aurora network-transport operation.
    Endpoint {
        endpoint: usize,
        source: NetworkTransportError,
    },
    /// Scheduled latency is invalid.
    InvalidTiming,
    /// Operation is not legal in the current lifecycle.
    InvalidLifecycle,
}

impl fmt::Display for EspAvbTransportSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fanout(error) => write!(formatter, "ESP-AVB fanout failed: {error}"),
            Self::Capability { endpoint } => {
                write!(
                    formatter,
                    "ESP-AVB endpoint {endpoint} has incompatible capabilities"
                )
            }
            Self::Endpoint { endpoint, source } => {
                write!(formatter, "ESP-AVB endpoint {endpoint} failed: {source}")
            }
            Self::InvalidTiming => formatter.write_str("invalid ESP-AVB scheduled latency"),
            Self::InvalidLifecycle => {
                formatter.write_str("invalid ESP-AVB transport-set lifecycle")
            }
        }
    }
}

impl std::error::Error for EspAvbTransportSetError {}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use aurora_realtime_audio_api::{
        MediaTimestamp, NetworkTransportCapabilities, AURORA_NETWORK_MEDIA_RATE,
    };

    use super::*;
    use crate::{EspAvbEndpointMedium, AURORA_7_1_4_CHANNELS};

    #[derive(Debug, Default)]
    struct ProbeState {
        prepared: usize,
        started: usize,
        stopped: usize,
        reset: usize,
        submissions: Vec<(u64, u64, Vec<f32>)>,
    }

    struct FakeAvbTransport {
        state: Arc<Mutex<ProbeState>>,
        fail_submit: bool,
        prepared: bool,
        started: bool,
    }

    impl FakeAvbTransport {
        fn new(state: Arc<Mutex<ProbeState>>, fail_submit: bool) -> Self {
            Self {
                state,
                fail_submit,
                prepared: false,
                started: false,
            }
        }
    }

    impl NetworkAudioTransport for FakeAvbTransport {
        fn capabilities(&self) -> NetworkTransportCapabilities {
            NetworkTransportCapabilities {
                family: NetworkTransportFamily::AvbTsn,
                max_channels: 2,
                scheduled_playout: true,
                hardware_timestamps: false,
                adaptive_rate_matching: false,
                packet_repair: false,
            }
        }

        fn prepare(&mut self, config: NetworkStreamConfig) -> Result<(), NetworkTransportError> {
            if config.format.sample_rate != AURORA_NETWORK_MEDIA_RATE
                || config.format.channels != 2
                || config.clock_discipline != NetworkClockDiscipline::PtpFollower
                || config.timing.maximum_rate_correction_ppm != 0.0
            {
                return Err(NetworkTransportError::WorkerFault);
            }
            self.prepared = true;
            self.started = false;
            self.state.lock().unwrap().prepared += 1;
            Ok(())
        }

        fn start(&mut self) -> Result<(), NetworkTransportError> {
            if !self.prepared {
                return Err(NetworkTransportError::NotPrepared);
            }
            self.started = true;
            self.state.lock().unwrap().started += 1;
            Ok(())
        }

        fn submit(&mut self, block: NetworkAudioBlock<'_>) -> Result<(), NetworkTransportError> {
            if !self.started {
                return Err(NetworkTransportError::NotPrepared);
            }
            if self.fail_submit {
                return Err(NetworkTransportError::WorkerFault);
            }
            block.validate()?;
            self.state.lock().unwrap().submissions.push((
                block.sequence,
                block.timestamp.frame_index,
                block.samples.to_vec(),
            ));
            Ok(())
        }

        fn poll_event(&mut self) -> Option<NetworkTransportEvent> {
            None
        }

        fn stop(&mut self) -> Result<(), NetworkTransportError> {
            if !self.prepared {
                return Err(NetworkTransportError::NotPrepared);
            }
            self.started = false;
            self.state.lock().unwrap().stopped += 1;
            Ok(())
        }

        fn reset(&mut self) {
            self.state.lock().unwrap().reset += 1;
        }
    }

    fn states() -> [Arc<Mutex<ProbeState>>; ESP_AVB_7_1_4_ENDPOINTS] {
        std::array::from_fn(|_| Arc::new(Mutex::new(ProbeState::default())))
    }

    fn transport_array(
        states: &[Arc<Mutex<ProbeState>>; ESP_AVB_7_1_4_ENDPOINTS],
        fail_submit: Option<usize>,
    ) -> EspAvbTransportArray {
        std::array::from_fn(|index| {
            Box::new(FakeAvbTransport::new(
                Arc::clone(&states[index]),
                fail_submit == Some(index),
            )) as Box<dyn NetworkAudioTransport>
        })
    }

    fn input_samples() -> Vec<f32> {
        (0..AURORA_7_1_4_CHANNELS * 2)
            .map(|value| value as f32)
            .collect()
    }

    fn input_block(samples: &[f32]) -> NetworkAudioBlock<'_> {
        NetworkAudioBlock {
            sequence: 4,
            timestamp: MediaTimestamp::new(9_600, AURORA_NETWORK_MEDIA_RATE).unwrap(),
            format: NetworkAudioFormat {
                sample_rate: AURORA_NETWORK_MEDIA_RATE,
                channels: AURORA_7_1_4_CHANNELS,
                block_frames: 2,
            },
            samples,
        }
    }

    #[test]
    fn six_transports_receive_same_timeline_and_channel_pairs() {
        let states = states();
        let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
        let mut set = EspAvbTransportSet::new(plan, transport_array(&states, None), 2).unwrap();
        set.prepare(480).unwrap();
        set.start().unwrap();

        let samples = input_samples();
        set.submit(&input_block(&samples)).unwrap();

        for (endpoint, state) in states.iter().enumerate() {
            let state = state.lock().unwrap();
            assert_eq!(state.prepared, 1);
            assert_eq!(state.started, 1);
            assert_eq!(state.submissions.len(), 1);
            let submission = &state.submissions[0];
            assert_eq!(submission.0, 4);
            assert_eq!(submission.1, 9_600);
            assert_eq!(submission.2.len(), 4);
            assert_eq!(submission.2[0], samples[endpoint * 2]);
            assert_eq!(submission.2[1], samples[endpoint * 2 + 1]);
            assert_eq!(submission.2[2], samples[12 + endpoint * 2]);
            assert_eq!(submission.2[3], samples[12 + endpoint * 2 + 1]);
        }
    }

    #[test]
    fn unsafe_capability_is_rejected_before_prepare() {
        struct AdaptiveTransport(FakeAvbTransport);

        impl NetworkAudioTransport for AdaptiveTransport {
            fn capabilities(&self) -> NetworkTransportCapabilities {
                let mut capabilities = self.0.capabilities();
                capabilities.adaptive_rate_matching = true;
                capabilities
            }

            fn prepare(
                &mut self,
                config: NetworkStreamConfig,
            ) -> Result<(), NetworkTransportError> {
                self.0.prepare(config)
            }

            fn start(&mut self) -> Result<(), NetworkTransportError> {
                self.0.start()
            }

            fn submit(
                &mut self,
                block: NetworkAudioBlock<'_>,
            ) -> Result<(), NetworkTransportError> {
                self.0.submit(block)
            }

            fn poll_event(&mut self) -> Option<NetworkTransportEvent> {
                self.0.poll_event()
            }

            fn stop(&mut self) -> Result<(), NetworkTransportError> {
                self.0.stop()
            }

            fn reset(&mut self) {
                self.0.reset();
            }
        }

        let states = states();
        let mut transports = transport_array(&states, None);
        transports[3] = Box::new(AdaptiveTransport(FakeAvbTransport::new(
            Arc::clone(&states[3]),
            false,
        )));
        let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
        assert!(matches!(
            EspAvbTransportSet::new(plan, transports, 48),
            Err(EspAvbTransportSetError::Capability { endpoint: 3 })
        ));
        assert!(states
            .iter()
            .all(|state| state.lock().unwrap().prepared == 0));
    }

    #[test]
    fn endpoint_submit_failure_aborts_whole_stream_and_requires_reprepare() {
        let states = states();
        let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
        let mut set = EspAvbTransportSet::new(plan, transport_array(&states, Some(3)), 2).unwrap();
        set.prepare(480).unwrap();
        set.start().unwrap();

        let samples = input_samples();
        assert!(matches!(
            set.submit(&input_block(&samples)),
            Err(EspAvbTransportSetError::Endpoint { endpoint: 3, .. })
        ));
        assert!(matches!(
            set.start(),
            Err(EspAvbTransportSetError::InvalidLifecycle)
        ));

        for state in &states {
            let state = state.lock().unwrap();
            assert_eq!(state.stopped, 1);
            assert_eq!(state.reset, 1);
        }
    }
}
