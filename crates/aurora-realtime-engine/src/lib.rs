//! Allocation-free real-time block engine for Aurora local audio.

mod asrc;
mod device_state;
mod drift;
mod drift_controller;
mod duplex;
mod latency;
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
pub use latency::{
    estimate_repeated_latency, generate_measurement_sequence, LatencyEstimateError,
    LatencyMeasurementReport,
};
pub use transport::{TransportKind, TransportPrototype};
pub use aurora_renderer_basic::BasicRendererMode;

use std::time::{Duration, Instant};

use aurora_core::{ChannelRole, StandardLayout, Vector3};
use aurora_dsp_basic::{BasicDspError, DelayProcessor};
use aurora_renderer_api::{RenderObject, Renderer, RendererError, RendererScratch, SpeakerGain};
use aurora_renderer_basic::{calculate_geometric_delays, BasicRenderer, BasicRendererMode};
use aurora_scene::RenderScene;
use thiserror::Error;

const TIMING_HISTOGRAM_BUCKETS: usize = 16;

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
    /// Renderer mode to use.
    pub renderer_mode: BasicRendererMode,
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

/// Setup-time real-time engine errors.
#[derive(Debug, Error)]
pub enum RealTimeEngineError {
    /// Renderer setup failed.
    #[error("renderer error: {0}")]
    Renderer(#[from] RendererError),
    /// DSP setup failed.
    #[error("dsp error: {0}")]
    Dsp(#[from] BasicDspError),
    /// Scene setup failed.
    #[error("scene error: {0}")]
    Scene(#[from] aurora_scene::SceneError),
    /// Invalid configuration.
    #[error("invalid real-time engine config: {0}")]
    InvalidConfig(String),
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
#[derive(Debug)]
pub struct RealTimeEngine {
    config: RealTimeEngineConfig,
    renderer: BasicRenderer,
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
    delay_processor: DelayProcessor,
    metrics: RealTimeMetrics,
    frame_cursor: u64,
    phase: f32,
    noise_state: u32,
    noise_filter_state: f32,
    impulse_emitted: bool,
    delay_reports: Vec<RealTimeDelayReport>,
}

impl RealTimeEngine {
    /// Creates a real-time engine and allocates all steady-state buffers.
    pub fn new(
        scene: RenderScene,
        config: RealTimeEngineConfig,
        estimated_device_latency_frames: usize,
    ) -> Result<Self, RealTimeEngineError> {
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
        let ordered_speakers = scene.ordered_speakers()?;
        if ordered_speakers.is_empty() {
            return Err(RealTimeEngineError::InvalidConfig(
                "scene must contain at least one output speaker".to_owned(),
            ));
        }
        let output_roles = ordered_speakers
            .iter()
            .map(|speaker| speaker.channel_role.clone())
            .collect::<Vec<_>>();
        let mut renderer =
            BasicRenderer::new(config.renderer_mode).with_smoothing(0.35);
        renderer.configure(
            ordered_speakers.clone(),
            config.sample_rate,
            config.block_size,
            1,
        )?;
        let renderer_scratch = RendererScratch::new(renderer.required_scratch_size()?);

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
            vec![0.0; ordered_speakers.len()]
        };
        let max_delay = delays.iter().copied().fold(0.0_f32, f32::max).ceil() + 2.0;
        let mut delay_processor = DelayProcessor::new(ordered_speakers.len(), max_delay);
        delay_processor.set_delays(delays)?;
        let dsp_latency_frames = delay_processor.latency_frames();
        let renderer_latency_frames = renderer.latency_frames();
        let block_duration_budget =
            Duration::from_secs_f64(config.block_size as f64 / f64::from(config.sample_rate));
        let metrics = RealTimeMetrics {
            renderer_latency_frames,
            dsp_latency_frames,
            estimated_device_latency_frames,
            estimated_end_to_end_latency_frames: renderer_latency_frames
                + dsp_latency_frames
                + estimated_device_latency_frames,
            block_duration_budget,
            ..RealTimeMetrics::default()
        };
        let channel_count = ordered_speakers.len();

        Ok(Self {
            mono: vec![0.0; config.block_size],
            planar: vec![vec![0.0; config.block_size]; channel_count],
            delayed: vec![vec![0.0; config.block_size]; channel_count],
            gains: vec![SpeakerGain::default(); channel_count],
            delays_scratch: vec![0.0; channel_count],
            renderer_scratch,
            delay_processor,
            output_roles,
            renderer,
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
        let shape_valid = output_channels > 0
            && chunk_samples > 0
            && output.len() % output_channels == 0
            && !output.is_empty();
        let result = if shape_valid {
            let mut result = Ok(());
            for chunk in output.chunks_mut(chunk_samples) {
                if let Err(fault) = self.process_inner(input, chunk) {
                    result = Err(fault);
                    break;
                }
                self.metrics.processed_blocks = self.metrics.processed_blocks.saturating_add(1);
            }
            result
        } else {
            Err(RealTimeFault::OutputBuffer)
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
        self.fill_mono(input, frame_count)?;
        self.render_planar(frame_count)?;
        self.delay_processor
            .process_block_into(&self.planar, &mut self.delayed, frame_count)
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
        self.renderer
            .render_gains(
                &self.listener,
                std::slice::from_ref(&object),
                &mut self.gains,
                &mut self.renderer_scratch,
            )
            .map_err(|_| RealTimeFault::Renderer)?;

        if self.config.apply_geometric_delay || self.config.renderer_mode == BasicRendererMode::Binaural {
            for (i, gain) in self.gains.iter().enumerate() {
                self.delays_scratch[i] = gain.delay_samples;
            }
            self.delay_processor
                .set_delays_slice(&self.delays_scratch)
                .map_err(|_| RealTimeFault::Dsp)?;
        }

        for channel in &mut self.planar {
            channel
                .iter_mut()
                .take(frame_count)
                .for_each(|sample| *sample = 0.0);
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
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    use super::*;
    use aurora_core::{Listener, Speaker};
    use aurora_scene::{SceneObject, Trajectory};

    struct ThreadCountingAllocator;

    thread_local! {
        static TRACK_ALLOCATIONS: Cell<bool> = const { Cell::new(false) };
        static ALLOCATION_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    unsafe impl GlobalAlloc for ThreadCountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            TRACK_ALLOCATIONS.with(|tracking| {
                if tracking.get() {
                    ALLOCATION_COUNT.with(|count| count.set(count.get().saturating_add(1)));
                }
            });
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            TRACK_ALLOCATIONS.with(|tracking| {
                if tracking.get() {
                    ALLOCATION_COUNT.with(|count| count.set(count.get().saturating_add(1)));
                }
            });
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static TEST_ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

    fn measured_allocations(operation: impl FnOnce()) -> usize {
        ALLOCATION_COUNT.with(|count| count.set(0));
        TRACK_ALLOCATIONS.with(|tracking| tracking.set(true));
        operation();
        TRACK_ALLOCATIONS.with(|tracking| tracking.set(false));
        ALLOCATION_COUNT.with(Cell::get)
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
        let mut engine = RealTimeEngine::new(
            scene(),
            RealTimeEngineConfig {
                sample_rate: 48_000,
                block_size: 64,
                input_channels: 2,
                apply_geometric_delay: false,
                speed_of_sound: 343.0,
                test_signal: TestSignal::None,
                renderer_mode: BasicRendererMode::InverseDistance,
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
            let engine =
                RealTimeEngine::new(scene, config(TestSignal::Silence, false), 64).unwrap();
            assert_eq!(engine.output_roles(), layout.canonical_roles());
        }
    }

    #[test]
    fn shutdown_by_drop_has_no_deadlock() {
        drop(engine(TestSignal::Silence, false));
    }

    fn config(test_signal: TestSignal, apply_delay: bool) -> RealTimeEngineConfig {
        RealTimeEngineConfig {
            sample_rate: 48_000,
            block_size: 64,
            input_channels: 0,
            apply_geometric_delay: apply_delay,
            speed_of_sound: 343.0,
            test_signal,
            renderer_mode: BasicRendererMode::InverseDistance,
        }
    }

    fn engine(test_signal: TestSignal, apply_delay: bool) -> RealTimeEngine {
        RealTimeEngine::new(scene(), config(test_signal, apply_delay), 64).unwrap()
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
