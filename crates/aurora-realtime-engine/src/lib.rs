//! Allocation-free real-time block engine for Aurora local audio.

mod asrc;
mod device_state;
mod drift;
mod drift_controller;
mod duplex;
mod head_pose_delivery;
mod latency;
mod network_bridge;
mod transport;

pub use asrc::{AsrcError, AsrcProcessReport, AsynchronousResampler, RubatoAsrc};
pub use device_state::{
    DuplexStateEvent, DuplexStateMachine, DuplexStateTransitionError, DuplexStreamState,
};
pub use drift::{
    correction_artifact_metrics, simulate_clock_drift, CorrectionArtifactMetrics,
    CorrectionTransition, DriftCompensator, DriftContext, DriftCorrection, DriftSimulationReport,
    DuplexFaultPolicy, DuplexHealth, ThresholdDriftCompensator,
};
pub use drift_controller::{
    simulate_adaptive_drift, AdaptiveDriftSimulationReport, DriftController, DriftControllerConfig,
    DriftControllerFault, DriftControllerReport,
};
pub use duplex::{
    create_adaptive_duplex_bridge, create_duplex_bridge, create_duplex_bridge_with_compensator,
    AdaptiveDuplexConsumer, AdaptiveDuplexFault, AdaptiveDuplexSnapshot, AdaptiveDuplexStatus,
    DuplexBridgeConfig, DuplexBridgeError, DuplexConsumer, DuplexFault, DuplexProducer,
    DuplexSnapshot, DuplexStatus,
};
pub use head_pose_delivery::{
    create_head_pose_delivery_bridge, HeadPoseControlEvent, HeadPoseDeliveryControl,
    HeadPoseDeliveryCreateError, HeadPoseIngressEvent, HeadPoseIngressProducer,
    HeadPoseIngressPushError,
};
pub use latency::{
    estimate_repeated_latency, generate_measurement_sequence, LatencyEstimateError,
    LatencyMeasurementReport,
};
pub use network_bridge::{
    create_network_transport_bridge, NetworkBlockConsumer, NetworkBlockMetadata,
    NetworkBlockProducer, NetworkBridgePopError, NetworkBridgePushError,
};
pub use transport::{TransportKind, TransportPrototype};

use std::time::{Duration, Instant};

use aurora_core::{ChannelRole, Speaker, StandardLayout, Vector3};
use aurora_dsp_api::{RealtimeDelayProcessor, RealtimeDspFault};
use aurora_renderer_api::{
    ObjectPcmBlock, ObjectPcmRenderer, PcmRendererError, RenderObject, Renderer,
    RendererCapabilities, RendererError, RendererScratch, RendererScratchSize, SpeakerGain,
};
use aurora_scene::RenderScene;
use thiserror::Error;

const TIMING_HISTOGRAM_BUCKETS: usize = 16;
const CURRENT_DYNAMIC_DELAY_CAPACITY_SAMPLES: f32 = 1_024.0;

/// Test signal generated when no live input is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestSignal {
    /// Use live input mapping.
    None,
    /// Sine source at the current object position.
    Sine,
    /// Deterministic pink-ish noise.
    PinkNoise,
    /// Single impulse followed by silence.
    Impulse,
    /// Silent source.
    Silence,
    /// Sine source rotating continuously around the listener.
    RotatingSine,
}

/// Real-time engine configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct RealTimeEngineConfig {
    /// Sample rate in hertz.
    pub sample_rate: u32,
    /// Maximum callback block size in frames.
    pub block_size: usize,
    /// Number of input channels.
    pub input_channels: usize,
    /// Whether to apply listener-relative geometric delay.
    pub apply_geometric_delay: bool,
    /// Speed of sound in meters per second.
    pub speed_of_sound: f32,
    /// Test signal mode.
    pub test_signal: TestSignal,
}

/// Per-channel delay report for real-time status.
#[derive(Debug, Clone, PartialEq)]
pub struct RealTimeDelayReport {
    /// Canonical output index.
    pub output_index: usize,
    /// Channel role.
    pub channel_role: ChannelRole,
    /// Distance from listener to speaker in meters.
    pub distance_meters: f32,
    /// Delay in milliseconds.
    pub delay_milliseconds: f32,
    /// Delay in samples.
    pub delay_samples: f32,
}

/// Fixed fault codes published by the audio path without string allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum RealTimeFault {
    /// No active fault.
    #[default]
    None = 0,
    /// Output buffer shape did not match the configured stream.
    OutputBuffer = 1,
    /// Input buffer shape did not match the configured stream.
    InputBuffer = 2,
    /// Renderer rejected a steady-state block.
    Renderer = 3,
    /// DSP rejected a steady-state block.
    Dsp = 4,
}

/// Result of one callback block without an allocated error payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    /// Block completed successfully.
    Ok,
    /// Block was replaced by silence and the fault was recorded.
    Fault(RealTimeFault),
}

/// Callback and scheduling metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct RealTimeMetrics {
    /// Processed audio blocks.
    pub processed_blocks: u64,
    /// Total callback invocations.
    pub callback_count: u64,
    /// Input underrun count.
    pub input_underruns: u64,
    /// Output underrun count.
    pub output_underruns: u64,
    /// Dropped block count.
    pub dropped_blocks: u64,
    /// Last callback duration.
    pub last_callback_duration: Duration,
    /// Maximum callback duration.
    pub max_callback_duration: Duration,
    /// Average callback duration.
    pub average_callback_duration: Duration,
    /// Approximate p95 callback duration from a fixed histogram.
    pub p95_callback_duration: Duration,
    /// Duration available for one configured block.
    pub block_duration_budget: Duration,
    /// Average callback use of the block budget.
    pub average_budget_usage_percent: f64,
    /// Maximum callback use of the block budget.
    pub maximum_budget_usage_percent: f64,
    /// Estimated end-to-end latency in frames.
    pub estimated_end_to_end_latency_frames: usize,
    /// Renderer latency in frames.
    pub renderer_latency_frames: usize,
    /// DSP latency in frames.
    pub dsp_latency_frames: usize,
    /// Estimated device buffering in frames; never a physical measurement.
    pub estimated_device_latency_frames: usize,
    /// Last persistent fault status.
    pub fault: RealTimeFault,
    timing_histogram: [u64; TIMING_HISTOGRAM_BUCKETS],
}

impl Default for RealTimeMetrics {
    fn default() -> Self {
        Self {
            processed_blocks: 0,
            callback_count: 0,
            input_underruns: 0,
            output_underruns: 0,
            dropped_blocks: 0,
            last_callback_duration: Duration::ZERO,
            max_callback_duration: Duration::ZERO,
            average_callback_duration: Duration::ZERO,
            p95_callback_duration: Duration::ZERO,
            block_duration_budget: Duration::ZERO,
            average_budget_usage_percent: 0.0,
            maximum_budget_usage_percent: 0.0,
            estimated_end_to_end_latency_frames: 0,
            renderer_latency_frames: 0,
            dsp_latency_frames: 0,
            estimated_device_latency_frames: 0,
            fault: RealTimeFault::None,
            timing_histogram: [0; TIMING_HISTOGRAM_BUCKETS],
        }
    }
}

/// Setup-time validation errors for a caller-supplied gain renderer.
#[derive(Debug, Error, PartialEq)]
pub enum PreparedRendererError {
    /// Prepared renderer output channel count does not match the scene topology.
    #[error("prepared renderer has {actual} output channels, expected {expected}")]
    ChannelCount { expected: usize, actual: usize },
    /// Prepared renderer did not expose a valid configured scratch requirement.
    #[error("prepared renderer contract error: {0}")]
    Contract(#[source] RendererError),
}

/// Setup-time validation errors for a caller-supplied object-PCM renderer.
#[derive(Debug, Error, PartialEq)]
pub enum PreparedPcmRendererError {
    /// Prepared renderer output channel count does not match the scene topology.
    #[error("prepared PCM renderer has {actual} output channels, expected {expected}")]
    ChannelCount { expected: usize, actual: usize },
}

/// Setup-time validation errors for a caller-supplied realtime delay processor.
#[derive(Debug, Error, PartialEq)]
pub enum PreparedDspError {
    /// Prepared DSP channel count does not match the rendered output topology.
    #[error("prepared realtime DSP has {actual} channels, expected {expected}")]
    ChannelCount { expected: usize, actual: usize },
    /// Prepared DSP cannot represent Aurora's required dynamic delay capacity.
    #[error("prepared realtime DSP delay capacity {actual} is below required {required} samples")]
    DelayCapacity { required: f32, actual: f32 },
    /// Prepared DSP rejected the initial per-channel delay shape.
    #[error("prepared realtime DSP rejected the initial delay shape")]
    InitialDelayShape,
    /// Prepared DSP rejected one or more initial delay values.
    #[error("prepared realtime DSP rejected an initial delay value")]
    InitialDelayValue,
    /// Prepared DSP returned a callback-only buffer-shape fault during setup.
    #[error("prepared realtime DSP returned an unexpected buffer-shape fault during setup")]
    UnexpectedInitializationFault,
}

impl PreparedDspError {
    fn from_initial_fault(fault: RealtimeDspFault) -> Self {
        match fault {
            RealtimeDspFault::DelayShape => Self::InitialDelayShape,
            RealtimeDspFault::DelayValue => Self::InitialDelayValue,
            RealtimeDspFault::BufferShape => Self::UnexpectedInitializationFault,
        }
    }
}

/// Setup-time real-time engine errors.
#[derive(Debug, Error)]
pub enum RealTimeEngineError {
    /// Gain-renderer setup failed.
    #[error("renderer error: {0}")]
    Renderer(#[from] RendererError),
    /// Object-PCM renderer setup failed.
    #[error("PCM renderer error: {0}")]
    PcmRenderer(#[from] PcmRendererError),
    /// DSP setup failed on the compatibility/default implementation path.
    #[error("realtime DSP setup fault: {0}")]
    Dsp(#[from] RealtimeDspFault),
    /// Caller-supplied gain renderer failed setup validation.
    #[error("prepared renderer setup error: {0}")]
    PreparedRenderer(#[from] PreparedRendererError),
    /// Caller-supplied object-PCM renderer failed setup validation.
    #[error("prepared PCM renderer setup error: {0}")]
    PreparedPcmRenderer(#[from] PreparedPcmRendererError),
    /// Caller-supplied realtime DSP failed setup validation.
    #[error("prepared realtime DSP setup error: {0}")]
    PreparedDsp(#[from] PreparedDspError),
    /// Scene setup failed.
    #[error("scene error: {0}")]
    Scene(#[from] aurora_scene::SceneError),
    /// Invalid configuration.
    #[error("invalid real-time engine config: {0}")]
    InvalidConfig(String),
}

/// Realtime delay capabilities required before a prepared DSP can be activated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealtimeDelayRequirements {
    channel_count: usize,
    max_delay_samples: f32,
}

impl RealtimeDelayRequirements {
    /// Required rendered output channel count.
    pub const fn channel_count(&self) -> usize {
        self.channel_count
    }

    /// Minimum supported per-channel delay range in samples.
    pub const fn max_delay_samples(&self) -> f32 {
        self.max_delay_samples
    }
}

/// Fixed buffer capacities exposed for steady-state allocation guards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferCapacities {
    /// Mono input/source storage capacity.
    pub mono: usize,
    /// Sum of planar render channel capacities.
    pub planar: usize,
    /// Sum of delayed output channel capacities.
    pub delayed: usize,
    /// Per-speaker gain capacity.
    pub gains: usize,
    /// Renderer scratch float capacity.
    pub renderer_scratch: usize,
}

/// Bounded single-thread ring buffer for preallocated control-side staging.
#[derive(Debug, Clone, PartialEq)]
pub struct RingBuffer<T: Copy + Default> {
    buffer: Vec<T>,
    read: usize,
    write: usize,
    len: usize,
}

impl<T: Copy + Default> RingBuffer<T> {
    /// Creates a fixed-capacity ring buffer. Zero capacity remains safely full.
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![T::default(); capacity],
            read: 0,
            write: 0,
            len: 0,
        }
    }

    /// Returns capacity.
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Returns current length.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true when empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Pushes one value, returning it when the buffer is full or has zero capacity.
    pub fn push(&mut self, value: T) -> Result<(), T> {
        if self.len == self.buffer.len() || self.buffer.is_empty() {
            return Err(value);
        }
        if let Some(slot) = self.buffer.get_mut(self.write) {
            *slot = value;
        } else {
            return Err(value);
        }
        self.write = (self.write + 1) % self.buffer.len();
        self.len += 1;
        Ok(())
    }

    /// Pops one value.
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 || self.buffer.is_empty() {
            return None;
        }
        let value = self.buffer.get(self.read).copied()?;
        self.read = (self.read + 1) % self.buffer.len();
        self.len -= 1;
        Some(value)
    }
}

/// Preallocated real-time block processor.
///
/// Exactly one render backend is active for the lifetime of an engine: either
/// the legacy gain renderer or an object-PCM renderer. Both feed the same
/// post-render DSP, delay, interleave, and transport-facing output boundary.
pub struct RealTimeEngine {
    config: RealTimeEngineConfig,
    renderer: Option<Box<dyn Renderer>>,
    pcm_renderer: Option<Box<dyn ObjectPcmRenderer>>,
    renderer_dynamic_delay_values: bool,
    listener: aurora_core::Listener,
    trajectory: aurora_scene::Trajectory,
    object_gain: f32,
    output_roles: Vec<ChannelRole>,
    mono: Vec<f32>,
    planar: Vec<Vec<f32>>,
    delayed: Vec<Vec<f32>>,
    gains: Vec<SpeakerGain>,
    delays_scratch: Vec<f32>,
    renderer_scratch: RendererScratch,
    delay_processor: Box<dyn RealtimeDelayProcessor>,
    metrics: RealTimeMetrics,
    frame_cursor: u64,
    phase: f32,
    noise_state: u32,
    noise_filter_state: f32,
    impulse_emitted: bool,
    delay_reports: Vec<RealTimeDelayReport>,
}

impl RealTimeEngine {
    /// Returns the delay capabilities required for this scene, configuration, and renderer.
    ///
    /// Materializers call this on the control thread after preparing a renderer and before
    /// constructing a concrete DSP.
    pub fn delay_requirements(
        scene: &RenderScene,
        config: &RealTimeEngineConfig,
        renderer_capabilities: RendererCapabilities,
    ) -> Result<RealtimeDelayRequirements, RealTimeEngineError> {
        validate_scene_config(scene, config)?;
        let ordered_speakers = scene.ordered_speakers()?;
        let geometric = calculate_geometric_delays(
            &ordered_speakers,
            scene.listener,
            config.sample_rate,
            config.speed_of_sound,
        );
        let delays = if config.apply_geometric_delay {
            geometric
                .iter()
                .map(|delay| delay.delay_samples)
                .collect::<Vec<_>>()
        } else {
            vec![0.0; ordered_speakers.len()]
        };
        let mut max_delay_samples = delays.iter().copied().fold(0.0_f32, f32::max).ceil() + 2.0;
        if config.apply_geometric_delay || renderer_capabilities.dynamic_delay_values() {
            max_delay_samples = max_delay_samples.max(CURRENT_DYNAMIC_DELAY_CAPACITY_SAMPLES);
        }
        Ok(RealtimeDelayRequirements {
            channel_count: ordered_speakers.len(),
            max_delay_samples,
        })
    }

    /// Creates a real-time engine from caller-supplied prepared gain renderer and DSP components.
    pub fn new_with_prepared_components(
        scene: RenderScene,
        config: RealTimeEngineConfig,
        estimated_device_latency_frames: usize,
        renderer: Box<dyn Renderer>,
        delay_processor: Box<dyn RealtimeDelayProcessor>,
    ) -> Result<Self, RealTimeEngineError> {
        Self::build_gain(
            scene,
            config,
            estimated_device_latency_frames,
            renderer,
            delay_processor,
        )
    }

    /// Creates a real-time engine from a prepared object-PCM renderer and DSP.
    ///
    /// This is mutually exclusive with the gain-renderer path. The PCM renderer
    /// writes the engine's preallocated planar buffers directly; the same DSP,
    /// delay and interleave stages then process those buffers.
    pub fn new_with_prepared_pcm_components(
        scene: RenderScene,
        config: RealTimeEngineConfig,
        estimated_device_latency_frames: usize,
        renderer: Box<dyn ObjectPcmRenderer>,
        delay_processor: Box<dyn RealtimeDelayProcessor>,
    ) -> Result<Self, RealTimeEngineError> {
        Self::build_pcm(
            scene,
            config,
            estimated_device_latency_frames,
            renderer,
            delay_processor,
        )
    }

    fn build_gain(
        scene: RenderScene,
        config: RealTimeEngineConfig,
        estimated_device_latency_frames: usize,
        renderer: Box<dyn Renderer>,
        mut delay_processor: Box<dyn RealtimeDelayProcessor>,
    ) -> Result<Self, RealTimeEngineError> {
        validate_scene_config(&scene, &config)?;
        let ordered_speakers = scene.ordered_speakers()?;
        let channel_count = ordered_speakers.len();
        let output_roles = ordered_speakers
            .iter()
            .map(|speaker| speaker.channel_role.clone())
            .collect::<Vec<_>>();

        let actual_renderer_channels = renderer.output_channel_count();
        if actual_renderer_channels != channel_count {
            return Err(PreparedRendererError::ChannelCount {
                expected: channel_count,
                actual: actual_renderer_channels,
            }
            .into());
        }
        let renderer_capabilities = renderer.capabilities();
        let renderer_scratch_size = renderer
            .required_scratch_size()
            .map_err(PreparedRendererError::Contract)?;
        let renderer_scratch = RendererScratch::new(renderer_scratch_size);

        let geometric = calculate_geometric_delays(
            &ordered_speakers,
            scene.listener,
            config.sample_rate,
            config.speed_of_sound,
        );
        let delay_reports = geometric
            .iter()
            .enumerate()
            .map(|(output_index, delay)| RealTimeDelayReport {
                output_index,
                channel_role: delay.channel_role.clone(),
                distance_meters: delay.distance_meters,
                delay_milliseconds: delay.delay_milliseconds,
                delay_samples: delay.delay_samples,
            })
            .collect::<Vec<_>>();
        let delays = if config.apply_geometric_delay {
            geometric
                .iter()
                .map(|delay| delay.delay_samples)
                .collect::<Vec<_>>()
        } else {
            vec![0.0; channel_count]
        };
        let mut max_delay = delays.iter().copied().fold(0.0_f32, f32::max).ceil() + 2.0;
        if config.apply_geometric_delay || renderer_capabilities.dynamic_delay_values() {
            max_delay = max_delay.max(CURRENT_DYNAMIC_DELAY_CAPACITY_SAMPLES);
        }
        validate_delay_processor(&mut *delay_processor, channel_count, max_delay, &delays)?;
        let dsp_latency_frames = delay_processor.latency_frames();
        let renderer_latency_frames = renderer.latency_frames();
        let metrics = make_metrics(
            &config,
            renderer_latency_frames,
            dsp_latency_frames,
            estimated_device_latency_frames,
        );
        Ok(Self {
            mono: vec![0.0; config.block_size],
            planar: vec![vec![0.0; config.block_size]; channel_count],
            delayed: vec![vec![0.0; config.block_size]; channel_count],
            gains: vec![SpeakerGain::default(); channel_count],
            delays_scratch: vec![0.0; channel_count],
            renderer_scratch,
            delay_processor,
            output_roles,
            renderer: Some(renderer),
            pcm_renderer: None,
            renderer_dynamic_delay_values: renderer_capabilities.dynamic_delay_values(),
            listener: scene.listener,
            trajectory: scene.trajectory,
            object_gain: db_to_gain(scene.object.gain_db),
            config,
            metrics,
            frame_cursor: 0,
            phase: 0.0,
            noise_state: 0x1234_ABCD,
            noise_filter_state: 0.0,
            impulse_emitted: false,
            delay_reports,
        })
    }

    fn build_pcm(
        scene: RenderScene,
        config: RealTimeEngineConfig,
        estimated_device_latency_frames: usize,
        renderer: Box<dyn ObjectPcmRenderer>,
        mut delay_processor: Box<dyn RealtimeDelayProcessor>,
    ) -> Result<Self, RealTimeEngineError> {
        validate_scene_config(&scene, &config)?;
        let ordered_speakers = scene.ordered_speakers()?;
        let channel_count = ordered_speakers.len();
        let output_roles = ordered_speakers
            .iter()
            .map(|speaker| speaker.channel_role.clone())
            .collect::<Vec<_>>();
        let actual_renderer_channels = renderer.output_channel_count();
        if actual_renderer_channels != channel_count {
            return Err(PreparedPcmRendererError::ChannelCount {
                expected: channel_count,
                actual: actual_renderer_channels,
            }
            .into());
        }

        let geometric = calculate_geometric_delays(
            &ordered_speakers,
            scene.listener,
            config.sample_rate,
            config.speed_of_sound,
        );
        let delay_reports = geometric
            .iter()
            .enumerate()
            .map(|(output_index, delay)| RealTimeDelayReport {
                output_index,
                channel_role: delay.channel_role.clone(),
                distance_meters: delay.distance_meters,
                delay_milliseconds: delay.delay_milliseconds,
                delay_samples: delay.delay_samples,
            })
            .collect::<Vec<_>>();
        let delays = if config.apply_geometric_delay {
            geometric
                .iter()
                .map(|delay| delay.delay_samples)
                .collect::<Vec<_>>()
        } else {
            vec![0.0; channel_count]
        };
        let mut max_delay = delays.iter().copied().fold(0.0_f32, f32::max).ceil() + 2.0;
        if config.apply_geometric_delay {
            max_delay = max_delay.max(CURRENT_DYNAMIC_DELAY_CAPACITY_SAMPLES);
        }
        validate_delay_processor(&mut *delay_processor, channel_count, max_delay, &delays)?;
        let dsp_latency_frames = delay_processor.latency_frames();
        let renderer_latency_frames = renderer.latency_frames();
        let metrics = make_metrics(
            &config,
            renderer_latency_frames,
            dsp_latency_frames,
            estimated_device_latency_frames,
        );
        Ok(Self {
            mono: vec![0.0; config.block_size],
            planar: vec![vec![0.0; config.block_size]; channel_count],
            delayed: vec![vec![0.0; config.block_size]; channel_count],
            gains: vec![SpeakerGain::default(); channel_count],
            delays_scratch: vec![0.0; channel_count],
            renderer_scratch: RendererScratch::new(RendererScratchSize { float_count: 0 }),
            delay_processor,
            output_roles,
            renderer: None,
            pcm_renderer: Some(renderer),
            renderer_dynamic_delay_values: false,
            listener: scene.listener,
            trajectory: scene.trajectory,
            object_gain: db_to_gain(scene.object.gain_db),
            config,
            metrics,
            frame_cursor: 0,
            phase: 0.0,
            noise_state: 0x1234_ABCD,
            noise_filter_state: 0.0,
            impulse_emitted: false,
            delay_reports,
        })
    }

    /// Returns output channel roles.
    pub fn output_roles(&self) -> &[ChannelRole] {
        &self.output_roles
    }

    /// Returns geometric delay reports.
    pub fn delay_reports(&self) -> &[RealTimeDelayReport] {
        &self.delay_reports
    }

    /// Returns current metrics.
    pub fn metrics(&self) -> &RealTimeMetrics {
        &self.metrics
    }

    /// Returns capacities of all callback-relevant growable buffers.
    pub fn buffer_capacities(&self) -> BufferCapacities {
        BufferCapacities {
            mono: self.mono.capacity(),
            planar: self.planar.iter().map(Vec::capacity).sum(),
            delayed: self.delayed.iter().map(Vec::capacity).sum(),
            gains: self.gains.capacity(),
            renderer_scratch: self.renderer_scratch.float_capacity(),
        }
    }

    /// Processes one callback block, replacing invalid blocks with silence.
    pub fn process_interleaved(
        &mut self,
        input: Option<&[f32]>,
        output: &mut [f32],
    ) -> ProcessStatus {
        let started = Instant::now();
        self.metrics.callback_count = self.metrics.callback_count.saturating_add(1);
        let output_channels = self.output_roles.len();
        let chunk_samples = self.config.block_size.saturating_mul(output_channels);
        let full_block_only = self.pcm_renderer.is_some();
        let shape_valid = output_channels > 0
            && chunk_samples > 0
            && output.len() % output_channels == 0
            && !output.is_empty()
            && (!full_block_only || output.len() % chunk_samples == 0);
        // Validate the entire external-input callback before advancing renderer,
        // delay or media state. A later short block must not replay its prefix.
        let uses_input = self.config.test_signal == TestSignal::None;
        let input_valid = !uses_input
            || (shape_valid
                && self.config.input_channels > 0
                && (output.len() / output_channels)
                    .checked_mul(self.config.input_channels)
                    .is_some_and(|required| {
                        input.is_some_and(|samples| samples.len() >= required)
                    }));
        let result = if !shape_valid {
            Err(RealTimeFault::OutputBuffer)
        } else if !input_valid {
            Err(RealTimeFault::InputBuffer)
        } else {
            let mut result = Ok(());
            let mut input_offset = 0;
            for chunk in output.chunks_mut(chunk_samples) {
                let block_input = if uses_input {
                    let count = (chunk.len() / output_channels) * self.config.input_channels;
                    let block = input.map(|samples| &samples[input_offset..input_offset + count]);
                    input_offset += count;
                    block
                } else {
                    input
                };
                if let Err(fault) = self.process_inner(block_input, chunk) {
                    result = Err(fault);
                    break;
                }
                self.metrics.processed_blocks = self.metrics.processed_blocks.saturating_add(1);
            }
            result
        };
        let status = if let Err(fault) = result {
            output.fill(0.0);
            self.metrics.dropped_blocks = self.metrics.dropped_blocks.saturating_add(1);
            self.metrics.fault = fault;
            if fault == RealTimeFault::OutputBuffer {
                self.metrics.output_underruns = self.metrics.output_underruns.saturating_add(1);
            }
            if fault == RealTimeFault::InputBuffer {
                self.metrics.input_underruns = self.metrics.input_underruns.saturating_add(1);
            }
            ProcessStatus::Fault(fault)
        } else {
            ProcessStatus::Ok
        };
        self.record_timing(started.elapsed());
        status
    }

    fn process_inner(
        &mut self,
        input: Option<&[f32]>,
        output: &mut [f32],
    ) -> Result<(), RealTimeFault> {
        let output_channels = self.output_roles.len();
        if output_channels == 0 || output.len() % output_channels != 0 {
            return Err(RealTimeFault::OutputBuffer);
        }
        let frame_count = output.len() / output_channels;
        if frame_count == 0 || frame_count > self.config.block_size {
            return Err(RealTimeFault::OutputBuffer);
        }
        if self.pcm_renderer.is_some() && frame_count != self.config.block_size {
            return Err(RealTimeFault::OutputBuffer);
        }
        self.fill_mono(input, frame_count)?;
        self.render_planar(frame_count)?;
        self.delay_processor
            .process_planar(&self.planar, &mut self.delayed, frame_count)
            .map_err(|_| RealTimeFault::Dsp)?;
        if !interleave(&self.delayed, frame_count, output) {
            return Err(RealTimeFault::OutputBuffer);
        }
        self.frame_cursor = self.frame_cursor.saturating_add(frame_count as u64);
        Ok(())
    }

    fn fill_mono(
        &mut self,
        input: Option<&[f32]>,
        frame_count: usize,
    ) -> Result<(), RealTimeFault> {
        match self.config.test_signal {
            TestSignal::None => {
                let input = input.ok_or(RealTimeFault::InputBuffer)?;
                let required = frame_count.saturating_mul(self.config.input_channels);
                if input.len() < required || self.config.input_channels == 0 {
                    return Err(RealTimeFault::InputBuffer);
                }
                for (target, frame) in self
                    .mono
                    .iter_mut()
                    .take(frame_count)
                    .zip(input.chunks_exact(self.config.input_channels))
                {
                    let mut sum = 0.0_f32;
                    for sample in frame {
                        sum += *sample;
                    }
                    *target = sum / self.config.input_channels as f32;
                }
            }
            TestSignal::Sine | TestSignal::RotatingSine => self.fill_sine(frame_count),
            TestSignal::PinkNoise => self.fill_pinkish_noise(frame_count),
            TestSignal::Impulse => {
                self.mono
                    .iter_mut()
                    .take(frame_count)
                    .for_each(|sample| *sample = 0.0);
                if !self.impulse_emitted {
                    if let Some(sample) = self.mono.first_mut() {
                        *sample = 1.0;
                        self.impulse_emitted = true;
                    }
                }
            }
            TestSignal::Silence => self
                .mono
                .iter_mut()
                .take(frame_count)
                .for_each(|sample| *sample = 0.0),
        }
        Ok(())
    }

    fn fill_sine(&mut self, frame_count: usize) {
        let increment = std::f32::consts::TAU * 440.0 / self.config.sample_rate as f32;
        for sample in self.mono.iter_mut().take(frame_count) {
            *sample = self.phase.sin() * 0.2;
            self.phase = (self.phase + increment) % std::f32::consts::TAU;
        }
    }

    fn fill_pinkish_noise(&mut self, frame_count: usize) {
        for sample in self.mono.iter_mut().take(frame_count) {
            self.noise_state = self
                .noise_state
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            let white = ((self.noise_state >> 8) as f32 / 16_777_216.0) * 2.0 - 1.0;
            self.noise_filter_state = self.noise_filter_state * 0.98 + white * 0.02;
            *sample = self.noise_filter_state * 0.2;
        }
    }

    fn render_planar(&mut self, frame_count: usize) -> Result<(), RealTimeFault> {
        let midpoint = self.frame_cursor.saturating_add(frame_count as u64 / 2);
        let time_seconds = midpoint as f64 / f64::from(self.config.sample_rate);
        let position = if self.config.test_signal == TestSignal::RotatingSine {
            rotating_position(time_seconds)
        } else {
            self.trajectory.position_at_time(time_seconds)
        };
        let object = RenderObject {
            position,
            gain: self.object_gain,
        };

        for channel in &mut self.planar {
            channel
                .iter_mut()
                .take(frame_count)
                .for_each(|sample| *sample = 0.0);
        }

        if let Some(renderer) = self.pcm_renderer.as_mut() {
            let block = [ObjectPcmBlock {
                object,
                samples: &self.mono[..frame_count],
            }];
            renderer
                .render_pcm(&self.listener, &block, &mut self.planar)
                .map_err(|_| RealTimeFault::Renderer)?;
            return Ok(());
        }

        let renderer = self.renderer.as_mut().ok_or(RealTimeFault::Renderer)?;
        renderer
            .render_gains(
                &self.listener,
                std::slice::from_ref(&object),
                &mut self.gains,
                &mut self.renderer_scratch,
            )
            .map_err(|_| RealTimeFault::Renderer)?;

        if self.config.apply_geometric_delay || self.renderer_dynamic_delay_values {
            for (i, gain) in self.gains.iter().enumerate() {
                self.delays_scratch[i] = gain.delay_samples;
            }
            self.delay_processor
                .set_delays(&self.delays_scratch)
                .map_err(|_| RealTimeFault::Dsp)?;
        }

        for (channel, gain) in self.planar.iter_mut().zip(self.gains.iter()) {
            for (target, mono) in channel.iter_mut().zip(self.mono.iter()).take(frame_count) {
                *target = *mono * gain.gain;
            }
        }
        Ok(())
    }

    fn record_timing(&mut self, elapsed: Duration) {
        self.metrics.last_callback_duration = elapsed;
        self.metrics.max_callback_duration = self.metrics.max_callback_duration.max(elapsed);
        let count = self.metrics.callback_count;
        let previous_total = self
            .metrics
            .average_callback_duration
            .as_nanos()
            .saturating_mul(u128::from(count.saturating_sub(1)));
        let average_nanos = previous_total
            .saturating_add(elapsed.as_nanos())
            .checked_div(u128::from(count))
            .unwrap_or(0)
            .min(u128::from(u64::MAX));
        self.metrics.average_callback_duration = Duration::from_nanos(average_nanos as u64);

        let budget_nanos = self.metrics.block_duration_budget.as_nanos().max(1);
        let scaled = elapsed
            .as_nanos()
            .saturating_mul(TIMING_HISTOGRAM_BUCKETS as u128)
            / budget_nanos;
        let bucket = usize::try_from(scaled)
            .unwrap_or(usize::MAX)
            .min(TIMING_HISTOGRAM_BUCKETS - 1);
        if let Some(count) = self.metrics.timing_histogram.get_mut(bucket) {
            *count = count.saturating_add(1);
        }
        self.metrics.p95_callback_duration = histogram_percentile(
            &self.metrics.timing_histogram,
            self.metrics.block_duration_budget,
            95,
        );
        self.metrics.average_budget_usage_percent = duration_percent(
            self.metrics.average_callback_duration,
            self.metrics.block_duration_budget,
        );
        self.metrics.maximum_budget_usage_percent = duration_percent(
            self.metrics.max_callback_duration,
            self.metrics.block_duration_budget,
        );
    }
}

fn validate_delay_processor(
    delay_processor: &mut dyn RealtimeDelayProcessor,
    channel_count: usize,
    max_delay: f32,
    delays: &[f32],
) -> Result<(), RealTimeEngineError> {
    let actual_channels = delay_processor.channel_count();
    if actual_channels != channel_count {
        return Err(PreparedDspError::ChannelCount {
            expected: channel_count,
            actual: actual_channels,
        }
        .into());
    }
    let actual_capacity = delay_processor.max_delay_samples();
    if !actual_capacity.is_finite() || actual_capacity < max_delay {
        return Err(PreparedDspError::DelayCapacity {
            required: max_delay,
            actual: actual_capacity,
        }
        .into());
    }
    delay_processor
        .set_delays(delays)
        .map_err(PreparedDspError::from_initial_fault)?;
    Ok(())
}

fn make_metrics(
    config: &RealTimeEngineConfig,
    renderer_latency_frames: usize,
    dsp_latency_frames: usize,
    estimated_device_latency_frames: usize,
) -> RealTimeMetrics {
    let block_duration_budget =
        Duration::from_secs_f64(config.block_size as f64 / f64::from(config.sample_rate));
    RealTimeMetrics {
        renderer_latency_frames,
        dsp_latency_frames,
        estimated_device_latency_frames,
        estimated_end_to_end_latency_frames: renderer_latency_frames
            + dsp_latency_frames
            + estimated_device_latency_frames,
        block_duration_budget,
        ..RealTimeMetrics::default()
    }
}

/// Interleaves fixed planar channels, returning false for inconsistent shapes.
pub fn interleave(channels: &[Vec<f32>], frame_count: usize, output: &mut [f32]) -> bool {
    let channel_count = channels.len();
    if channel_count == 0 || output.len() != frame_count.saturating_mul(channel_count) {
        return false;
    }
    if channels.iter().any(|channel| channel.len() < frame_count) {
        return false;
    }
    for (frame_index, output_frame) in output.chunks_exact_mut(channel_count).enumerate() {
        for (target, channel) in output_frame.iter_mut().zip(channels.iter()) {
            if let Some(sample) = channel.get(frame_index) {
                *target = *sample;
            } else {
                return false;
            }
        }
    }
    true
}

/// Returns channel roles for a standard speaker-identification layout.
pub fn identify_roles(layout: StandardLayout) -> &'static [ChannelRole] {
    layout.canonical_roles()
}

#[derive(Debug, Clone, PartialEq)]
struct GeometricDelay {
    channel_role: ChannelRole,
    distance_meters: f32,
    delay_samples: f32,
    delay_milliseconds: f32,
}

fn validate_scene_config(
    scene: &RenderScene,
    config: &RealTimeEngineConfig,
) -> Result<(), RealTimeEngineError> {
    if config.sample_rate == 0 || config.block_size == 0 {
        return Err(RealTimeEngineError::InvalidConfig(
            "sample_rate and block_size must be greater than zero".to_owned(),
        ));
    }
    if config.test_signal == TestSignal::None && config.input_channels == 0 {
        return Err(RealTimeEngineError::InvalidConfig(
            "live input requires at least one input channel".to_owned(),
        ));
    }
    if scene.ordered_speakers()?.is_empty() {
        return Err(RealTimeEngineError::InvalidConfig(
            "scene must contain at least one output speaker".to_owned(),
        ));
    }
    Ok(())
}

fn calculate_geometric_delays(
    speakers: &[Speaker],
    listener: aurora_core::Listener,
    sample_rate: u32,
    speed_of_sound_meters_per_second: f32,
) -> Vec<GeometricDelay> {
    let max_distance = speakers
        .iter()
        .filter(|speaker| speaker.enabled)
        .map(|speaker| speaker.position.distance_to(listener.position))
        .fold(0.0_f32, f32::max);
    speakers
        .iter()
        .map(|speaker| {
            let distance_meters = speaker.position.distance_to(listener.position);
            let delay_seconds = if speaker.enabled && speed_of_sound_meters_per_second > 0.0 {
                (max_distance - distance_meters).max(0.0) / speed_of_sound_meters_per_second
            } else {
                0.0
            };
            GeometricDelay {
                channel_role: speaker.channel_role.clone(),
                distance_meters,
                delay_samples: delay_seconds * sample_rate as f32,
                delay_milliseconds: delay_seconds * 1_000.0,
            }
        })
        .collect()
}

fn rotating_position(time_seconds: f64) -> Vector3 {
    let angle = time_seconds as f32 * std::f32::consts::TAU * 0.25;
    Vector3::new(angle.cos(), angle.sin(), 0.0)
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn duration_percent(value: Duration, budget: Duration) -> f64 {
    if budget.is_zero() {
        return 0.0;
    }
    value.as_secs_f64() / budget.as_secs_f64() * 100.0
}

fn histogram_percentile(
    histogram: &[u64; TIMING_HISTOGRAM_BUCKETS],
    budget: Duration,
    percentile: u64,
) -> Duration {
    let total = histogram.iter().copied().sum::<u64>();
    if total == 0 {
        return Duration::ZERO;
    }
    let threshold = total.saturating_mul(percentile).div_ceil(100);
    let mut cumulative = 0_u64;
    for (index, count) in histogram.iter().copied().enumerate() {
        cumulative = cumulative.saturating_add(count);
        if cumulative >= threshold {
            return budget.mul_f64((index + 1) as f64 / TIMING_HISTOGRAM_BUCKETS as f64);
        }
    }
    budget
}

#[cfg(test)]
mod tests {
    use aurora_test_alloc::{count_allocations as measured_allocations, CountingAllocator};

    use super::*;
    use aurora_core::{Listener, Speaker};
    use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};
    use aurora_scene::{SceneObject, Trajectory};

    #[global_allocator]
    static TEST_ALLOCATOR: CountingAllocator = CountingAllocator;

    #[derive(Debug)]
    struct PassThroughPcmRenderer {
        channels: usize,
        block_size: usize,
        latency_frames: usize,
    }

    impl ObjectPcmRenderer for PassThroughPcmRenderer {
        fn configure(
            &mut self,
            layout: Vec<Speaker>,
            _sample_rate: u32,
            block_size: usize,
            _max_objects: usize,
        ) -> Result<(), PcmRendererError> {
            self.channels = layout.iter().filter(|speaker| speaker.enabled).count();
            self.block_size = block_size;
            Ok(())
        }

        fn render_pcm(
            &mut self,
            _listener: &Listener,
            objects: &[ObjectPcmBlock<'_>],
            output: &mut [Vec<f32>],
        ) -> Result<(), PcmRendererError> {
            if output.len() != self.channels {
                return Err(PcmRendererError::OutputChannelCount {
                    required: self.channels,
                    actual: output.len(),
                });
            }
            let Some(object) = objects.first() else {
                return Ok(());
            };
            if object.samples.len() != self.block_size {
                return Err(PcmRendererError::InputBlockSize {
                    object_index: 0,
                    required: self.block_size,
                    actual: object.samples.len(),
                });
            }
            for (index, channel) in output.iter_mut().enumerate() {
                if channel.len() != self.block_size {
                    return Err(PcmRendererError::OutputBlockSize {
                        channel: index,
                        required: self.block_size,
                        actual: channel.len(),
                    });
                }
                channel.fill(0.0);
            }
            if let Some(first) = output.first_mut() {
                first.copy_from_slice(object.samples);
            }
            Ok(())
        }

        fn reset(&mut self) {}

        fn latency_frames(&self) -> usize {
            self.latency_frames
        }

        fn output_channel_count(&self) -> usize {
            self.channels
        }
    }

    fn record_process_status(all_blocks_processed: &mut bool, status: ProcessStatus) {
        *all_blocks_processed &= status == ProcessStatus::Ok;
    }

    #[test]
    fn ring_buffer_behavior_and_zero_capacity_are_safe() {
        let mut ring = RingBuffer::<u32>::new(2);
        assert_eq!(ring.push(1), Ok(()));
        assert_eq!(ring.push(2), Ok(()));
        assert_eq!(ring.push(3), Err(3));
        assert_eq!(ring.pop(), Some(1));
        assert_eq!(RingBuffer::<u32>::new(0).push(1), Err(1));
    }

    #[test]
    fn control_update_delivery_uses_bounded_preallocated_storage() {
        let mut updates = RingBuffer::<u32>::new(4);
        let capacity = updates.capacity();
        for update in 1..=4 {
            assert_eq!(updates.push(update), Ok(()));
        }
        assert_eq!(updates.push(5), Err(5));
        assert_eq!(updates.pop(), Some(1));
        assert_eq!(updates.capacity(), capacity);
    }

    #[test]
    fn block_scheduling_and_metrics_are_reported() {
        let mut engine = engine(TestSignal::Silence, false);
        let mut output = vec![0.0; 128];
        assert_eq!(
            engine.process_interleaved(None, &mut output),
            ProcessStatus::Ok
        );
        assert_eq!(engine.metrics().processed_blocks, 1);
        assert_eq!(engine.metrics().callback_count, 1);
        assert!(!engine.metrics().block_duration_budget.is_zero());
    }

    #[test]
    fn steady_state_capacities_remain_unchanged() {
        let mut engine = engine(TestSignal::Sine, true);
        let before = engine.buffer_capacities();
        let mut output = vec![0.0; 128];
        for _ in 0..10_000 {
            assert_eq!(
                engine.process_interleaved(None, &mut output),
                ProcessStatus::Ok
            );
        }
        assert_eq!(engine.buffer_capacities(), before);
    }

    #[test]
    fn warmed_up_steady_state_processing_allocates_zero_times_on_test_thread() {
        let mut engine = engine(TestSignal::RotatingSine, true);
        let mut output = vec![0.0; 128];
        for _ in 0..16 {
            let _ = engine.process_interleaved(None, &mut output);
        }
        let allocations = measured_allocations(|| {
            for _ in 0..1_000 {
                let _ = engine.process_interleaved(None, &mut output);
            }
        });
        assert_eq!(allocations, 0);
    }

    #[test]
    fn pcm_renderer_uses_same_post_render_pipeline_and_reports_latency() {
        let mut engine = pcm_engine(TestSignal::Sine, false, 64);
        let mut output = vec![0.0; 128];
        assert_eq!(engine.metrics().renderer_latency_frames, 17);
        assert_eq!(
            engine.process_interleaved(None, &mut output),
            ProcessStatus::Ok
        );
        assert!(output.iter().any(|sample| sample.abs() > f32::EPSILON));
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn pcm_renderer_rejects_partial_host_tail_fail_closed() {
        let mut engine = pcm_engine(TestSignal::Sine, false, 64);
        let mut output = vec![1.0; 64 * 2 + 16 * 2];
        assert_eq!(
            engine.process_interleaved(None, &mut output),
            ProcessStatus::Fault(RealTimeFault::OutputBuffer)
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn pcm_renderer_accepts_multi_block_host_callback_without_allocating() {
        let mut engine = pcm_engine(TestSignal::RotatingSine, false, 64);
        let mut output = vec![0.0; 64 * 2 * 3];
        for _ in 0..8 {
            assert_eq!(
                engine.process_interleaved(None, &mut output),
                ProcessStatus::Ok
            );
        }
        let allocations = measured_allocations(|| {
            for _ in 0..1_000 {
                let _ = engine.process_interleaved(None, &mut output);
            }
        });
        assert_eq!(allocations, 0);
    }

    #[test]
    fn external_pcm_multi_block_callback_preserves_each_input_frame_without_allocating() {
        let mut engine = pcm_engine(TestSignal::None, false, 64);
        let input: Vec<f32> = (0..192)
            .flat_map(|frame| [frame as f32 / 512.0, frame as f32 / 256.0])
            .collect();
        let mut output = vec![0.0; 192 * 2];
        let mut status = ProcessStatus::Ok;
        let allocations = measured_allocations(|| {
            status = engine.process_interleaved(Some(&input), &mut output);
        });
        assert_eq!(status, ProcessStatus::Ok);
        assert_eq!(allocations, 0);
        for (source, rendered) in input.chunks_exact(2).zip(output.chunks_exact(2)) {
            assert_eq!(rendered[0], (source[0] + source[1]) / 2.0);
            assert_eq!(rendered[1], 0.0);
        }
        assert_eq!(engine.metrics().processed_blocks, 3);
    }

    #[test]
    fn short_multi_block_input_is_rejected_before_advancing_media_state() {
        let mut engine = pcm_engine(TestSignal::None, false, 64);
        let input = vec![0.25; 64 * 2];
        let mut output = vec![1.0; 192 * 2];
        assert_eq!(
            engine.process_interleaved(Some(&input), &mut output),
            ProcessStatus::Fault(RealTimeFault::InputBuffer)
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert_eq!(engine.metrics().processed_blocks, 0);
        assert_eq!(engine.frame_cursor, 0);
        assert_eq!(engine.metrics().input_underruns, 1);
    }

    #[test]
    fn gain_renderer_external_input_matches_separate_callbacks_including_partial_tail() {
        let mut config = config(TestSignal::None, false);
        config.input_channels = 2;
        let mut batched = test_engine(scene(), config.clone(), 0).unwrap();
        let mut separate = test_engine(scene(), config, 0).unwrap();
        let input: Vec<f32> = (0..145 * 2).map(|i| i as f32 / 1024.0).collect();
        let mut actual = vec![0.0; 145 * 2];
        let mut expected = vec![0.0; 145 * 2];
        assert_eq!(
            batched.process_interleaved(Some(&input), &mut actual),
            ProcessStatus::Ok
        );
        for (source, target) in input.chunks(128).zip(expected.chunks_mut(128)) {
            assert_eq!(
                separate.process_interleaved(Some(source), target),
                ProcessStatus::Ok
            );
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn geometric_binaural_processing_allocates_zero_times_after_startup() {
        let mut engine = test_engine_with_mode(
            scene(),
            config(TestSignal::RotatingSine, false),
            64,
            BasicRendererMode::GeometricBinaural,
        )
        .unwrap();
        let mut output = vec![0.0; 128];
        for _ in 0..16 {
            assert_eq!(
                engine.process_interleaved(None, &mut output),
                ProcessStatus::Ok
            );
        }

        let mut all_blocks_processed = true;
        let allocations = measured_allocations(|| {
            for _ in 0..1_000 {
                record_process_status(
                    &mut all_blocks_processed,
                    engine.process_interleaved(None, &mut output),
                );
            }
        });

        assert!(all_blocks_processed);
        assert_eq!(allocations, 0);
    }

    #[test]
    fn allocation_status_accumulator_rejects_any_process_fault() {
        let mut all_blocks_processed = true;
        record_process_status(&mut all_blocks_processed, ProcessStatus::Ok);
        record_process_status(
            &mut all_blocks_processed,
            ProcessStatus::Fault(RealTimeFault::Renderer),
        );
        record_process_status(&mut all_blocks_processed, ProcessStatus::Ok);

        assert!(!all_blocks_processed);
    }

    #[test]
    fn duplex_input_and_output_callbacks_allocate_zero_times_after_startup() {
        let (producer, mut consumer, _status) = create_duplex_bridge(DuplexBridgeConfig {
            channels: 2,
            capacity_frames: 1_024,
            target_fill_frames: 256,
            correction_threshold_frames: 16,
        })
        .unwrap();
        let input = vec![0.1_f32; 128 * 2];
        let mut output = vec![0.0_f32; 128 * 2];
        for _ in 0..4 {
            producer.push_interleaved(&input, 2);
        }
        consumer.read_interleaved(&mut output, 2);

        let allocations = measured_allocations(|| {
            for _ in 0..1_000 {
                producer.push_interleaved(&input, 2);
                consumer.read_interleaved(&mut output, 2);
            }
        });
        assert_eq!(allocations, 0);
    }

    #[test]
    fn transport_prototypes_allocate_zero_times_after_startup() {
        let input = vec![0.1_f32; 128 * 6];
        let mut output = vec![0.0_f32; 128 * 6];
        for kind in [
            TransportKind::SampleArrayQueue,
            TransportKind::FixedBlockPool,
            TransportKind::ContiguousFrameRing,
        ] {
            let transport = TransportPrototype::new(kind, input.len(), 4).unwrap();
            let allocations = measured_allocations(|| {
                for _ in 0..1_000 {
                    assert!(transport.try_push_block(&input));
                    assert!(transport.try_pop_block(&mut output));
                }
            });
            assert_eq!(allocations, 0, "{} allocated", kind.label());
        }
    }

    #[test]
    fn asrc_and_adaptive_callbacks_allocate_zero_times_after_startup() {
        let mut resampler = RubatoAsrc::default();
        resampler.configure(48_000, 48_000, 2, 256).unwrap();
        let mut input = vec![0.1_f32; 2_048];
        let mut output = vec![0.0_f32; 256 * 2];
        let allocations = measured_allocations(|| {
            for _ in 0..100 {
                let required = resampler.required_input_frames() * 2;
                resampler.process(&input[..required], &mut output).unwrap();
            }
        });
        assert_eq!(allocations, 0);

        let config = DuplexBridgeConfig {
            channels: 2,
            capacity_frames: 4_096,
            target_fill_frames: 1_024,
            correction_threshold_frames: 128,
        };
        let (producer, mut consumer, _) = create_adaptive_duplex_bridge(
            config,
            DuplexFaultPolicy {
                maximum_excursion_frames: 4_096,
                ..DuplexFaultPolicy::default()
            },
            DriftControllerConfig {
                target_fill_frames: 1_024,
                ..DriftControllerConfig::default()
            },
            Box::new(RubatoAsrc::default()),
            256,
        )
        .unwrap();
        input.resize(256 * 2, 0.1);
        producer.push_interleaved(&vec![0.1; 1_024 * 2], 2);
        let allocations = measured_allocations(|| {
            for _ in 0..100 {
                producer.push_interleaved(&input, 2);
                consumer.read_interleaved(&mut output, 2);
            }
        });
        assert_eq!(allocations, 0);
    }

    #[test]
    fn malformed_output_propagates_fault_without_panic() {
        let mut engine = engine(TestSignal::Silence, false);
        let mut output = vec![1.0; 127];
        assert_eq!(
            engine.process_interleaved(None, &mut output),
            ProcessStatus::Fault(RealTimeFault::OutputBuffer)
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert_eq!(engine.metrics().fault, RealTimeFault::OutputBuffer);
    }

    #[test]
    fn oversized_host_callback_is_processed_as_fixed_blocks_and_tail() {
        let mut engine = engine(TestSignal::Silence, false);
        let mut output = vec![1.0; 960];
        assert_eq!(
            engine.process_interleaved(None, &mut output),
            ProcessStatus::Ok
        );
        assert_eq!(engine.metrics().callback_count, 1);
        assert_eq!(engine.metrics().processed_blocks, 8);
        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn malformed_live_input_propagates_fault_without_panic() {
        let mut engine = test_engine(
            scene(),
            RealTimeEngineConfig {
                sample_rate: 48_000,
                block_size: 64,
                input_channels: 2,
                apply_geometric_delay: false,
                speed_of_sound: 343.0,
                test_signal: TestSignal::None,
            },
            64,
        )
        .unwrap();
        let input = [0.0_f32; 3];
        let mut output = vec![1.0; 128];
        assert_eq!(
            engine.process_interleaved(Some(&input), &mut output),
            ProcessStatus::Fault(RealTimeFault::InputBuffer)
        );
        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn delay_state_is_continuous_across_blocks() {
        let mut engine = engine(TestSignal::Impulse, true);
        let mut first = vec![0.0; 128];
        let mut second = vec![0.0; 128];
        engine.process_interleaved(None, &mut first);
        engine.process_interleaved(None, &mut second);
        assert!(first
            .iter()
            .chain(second.iter())
            .any(|sample| *sample != 0.0));
    }

    #[test]
    fn rotating_output_is_deterministic_finite_and_nonallocating_by_capacity() {
        let mut first = engine(TestSignal::RotatingSine, false);
        let mut second = engine(TestSignal::RotatingSine, false);
        let before = first.buffer_capacities();
        let mut first_output = vec![0.0; 128];
        let mut second_output = vec![0.0; 128];
        for _ in 0..32 {
            first.process_interleaved(None, &mut first_output);
            second.process_interleaved(None, &mut second_output);
            assert_eq!(first_output, second_output);
            assert!(first_output.iter().all(|sample| sample.is_finite()));
        }
        assert_eq!(first.buffer_capacities(), before);
    }

    #[test]
    fn silence_remains_silence() {
        let mut engine = engine(TestSignal::Silence, true);
        let mut output = vec![1.0; 128];
        engine.process_interleaved(None, &mut output);
        assert!(output.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn standard_layout_regressions_preserve_canonical_order() {
        for (json, layout) in [
            (
                include_str!("../../../fixtures/scenes/stereo_circle.json"),
                StandardLayout::Stereo,
            ),
            (
                include_str!("../../../fixtures/scenes/circle.json"),
                StandardLayout::FiveOne,
            ),
            (
                include_str!("../../../fixtures/scenes/circle_7_1.json"),
                StandardLayout::SevenOne,
            ),
            (
                include_str!("../../../fixtures/scenes/5_1_2_upfiring.json"),
                StandardLayout::FiveOneTwo,
            ),
        ] {
            let scene: RenderScene = serde_json::from_str(json).unwrap();
            let engine = test_engine(scene, config(TestSignal::Silence, false), 64).unwrap();
            assert_eq!(engine.output_roles(), layout.canonical_roles());
        }
    }

    #[test]
    fn shutdown_by_drop_has_no_deadlock() {
        drop(engine(TestSignal::Silence, false));
        drop(pcm_engine(TestSignal::Silence, false, 64));
    }

    fn config(test_signal: TestSignal, apply_delay: bool) -> RealTimeEngineConfig {
        RealTimeEngineConfig {
            sample_rate: 48_000,
            block_size: 64,
            input_channels: 0,
            apply_geometric_delay: apply_delay,
            speed_of_sound: 343.0,
            test_signal,
        }
    }

    fn test_engine(
        scene: RenderScene,
        config: RealTimeEngineConfig,
        estimated_device_latency_frames: usize,
    ) -> Result<RealTimeEngine, RealTimeEngineError> {
        test_engine_with_mode(
            scene,
            config,
            estimated_device_latency_frames,
            BasicRendererMode::InverseDistance,
        )
    }

    fn test_engine_with_mode(
        scene: RenderScene,
        config: RealTimeEngineConfig,
        estimated_device_latency_frames: usize,
        mode: BasicRendererMode,
    ) -> Result<RealTimeEngine, RealTimeEngineError> {
        let speakers = scene.ordered_speakers()?;
        let mut renderer = BasicRenderer::new(mode).with_smoothing(0.35);
        renderer.configure(speakers, config.sample_rate, config.block_size, 1)?;
        let renderer_capabilities = renderer.capabilities();
        let requirements =
            RealTimeEngine::delay_requirements(&scene, &config, renderer_capabilities)?;
        let delay = aurora_dsp_basic::DelayProcessor::new(
            requirements.channel_count(),
            requirements.max_delay_samples(),
        );
        RealTimeEngine::new_with_prepared_components(
            scene,
            config,
            estimated_device_latency_frames,
            Box::new(renderer),
            Box::new(delay),
        )
    }

    fn pcm_engine(test_signal: TestSignal, apply_delay: bool, block_size: usize) -> RealTimeEngine {
        let scene = scene();
        let config = RealTimeEngineConfig {
            sample_rate: 48_000,
            block_size,
            input_channels: if test_signal == TestSignal::None {
                2
            } else {
                0
            },
            apply_geometric_delay: apply_delay,
            speed_of_sound: 343.0,
            test_signal,
        };
        let renderer = PassThroughPcmRenderer {
            channels: 2,
            block_size,
            latency_frames: 17,
        };
        let requirements =
            RealTimeEngine::delay_requirements(&scene, &config, RendererCapabilities::default())
                .unwrap();
        let delay = aurora_dsp_basic::DelayProcessor::new(
            requirements.channel_count(),
            requirements.max_delay_samples(),
        );
        RealTimeEngine::new_with_prepared_pcm_components(
            scene,
            config,
            64,
            Box::new(renderer),
            Box::new(delay),
        )
        .unwrap()
    }

    fn engine(test_signal: TestSignal, apply_delay: bool) -> RealTimeEngine {
        test_engine(scene(), config(test_signal, apply_delay), 64).unwrap()
    }

    fn scene() -> RenderScene {
        RenderScene {
            layout: StandardLayout::Stereo,
            listener: Listener {
                position: Vector3::ZERO,
                orientation: Vector3::new(0.0, 1.0, 0.0),
                ear_height: 1.2,
            },
            speakers: vec![
                speaker("left", ChannelRole::FrontLeft, -1.0),
                speaker("right", ChannelRole::FrontRight, 1.0),
            ],
            object: SceneObject {
                id: "source".to_owned(),
                gain_db: 0.0,
                spread: 0.0,
            },
            trajectory: Trajectory::Circle {
                center: Vector3::ZERO,
                radius: 0.5,
                z: 0.0,
                start_degrees: 0.0,
                revolutions_per_second: 0.25,
            },
            block_size: 64,
        }
    }

    fn speaker(id: &str, role: ChannelRole, x: f32) -> Speaker {
        Speaker {
            id: id.to_owned(),
            label: id.to_owned(),
            channel_role: role,
            position: Vector3::new(x, 0.0, 0.0),
            orientation: Vector3::ZERO,
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        }
    }
}
