use std::sync::{
    atomic::{AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    Arc,
};

use thiserror::Error;

use crate::{
    asrc::AsynchronousResampler,
    drift::{
        CorrectionTransition, DriftCompensator, DriftContext, DriftCorrection, DuplexFaultPolicy,
        DuplexHealth, ThresholdDriftCompensator,
    },
    drift_controller::{DriftController, DriftControllerConfig, DriftControllerFault},
    transport::SpscSampleRing,
};

/// Numeric adaptive duplex processing fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum AdaptiveDuplexFault {
    /// No adaptive processing fault.
    #[default]
    None = 0,
    /// The adaptive controller exceeded its supported correction range.
    Controller = 1,
    /// The asynchronous resampler rejected a callback operation.
    Resampler = 2,
    /// Fixed scratch cannot satisfy the resampler's requested input shape.
    InputCapacity = 3,
}

/// Control-thread snapshot of adaptive ratio behavior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptiveDuplexSnapshot {
    /// Underlying ring and callback counters.
    pub duplex: DuplexSnapshot,
    /// Current absolute output/input ratio.
    pub current_ratio: f64,
    /// Minimum ratio sent to the ASRC.
    pub minimum_ratio: f64,
    /// Maximum ratio sent to the ASRC.
    pub maximum_ratio: f64,
    /// Adaptive correction relative to nominal.
    pub correction_ppm: f64,
    /// Total controller saturation updates.
    pub controller_saturation_count: u64,
    /// ASRC algorithmic latency in output frames.
    pub resampler_latency_frames: usize,
    /// Numeric adaptive processing fault.
    pub fault: AdaptiveDuplexFault,
}

/// Shared numeric status for the adaptive duplex consumer.
#[derive(Debug)]
pub struct AdaptiveDuplexStatus {
    duplex: Arc<DuplexStatus>,
    current_ratio_bits: AtomicU64,
    minimum_ratio_bits: AtomicU64,
    maximum_ratio_bits: AtomicU64,
    correction_ppm_bits: AtomicU64,
    controller_saturation_count: AtomicU64,
    resampler_latency_frames: AtomicUsize,
    fault: AtomicU32,
}

impl AdaptiveDuplexStatus {
    fn new(duplex: Arc<DuplexStatus>, nominal_ratio: f64, latency_frames: usize) -> Self {
        Self {
            duplex,
            current_ratio_bits: AtomicU64::new(nominal_ratio.to_bits()),
            minimum_ratio_bits: AtomicU64::new(nominal_ratio.to_bits()),
            maximum_ratio_bits: AtomicU64::new(nominal_ratio.to_bits()),
            correction_ppm_bits: AtomicU64::new(0.0_f64.to_bits()),
            controller_saturation_count: AtomicU64::new(0),
            resampler_latency_frames: AtomicUsize::new(latency_frames),
            fault: AtomicU32::new(AdaptiveDuplexFault::None as u32),
        }
    }

    /// Returns numeric ring, ratio, and ASRC status.
    pub fn snapshot(&self) -> AdaptiveDuplexSnapshot {
        AdaptiveDuplexSnapshot {
            duplex: self.duplex.snapshot(),
            current_ratio: f64::from_bits(self.current_ratio_bits.load(Ordering::Acquire)),
            minimum_ratio: f64::from_bits(self.minimum_ratio_bits.load(Ordering::Relaxed)),
            maximum_ratio: f64::from_bits(self.maximum_ratio_bits.load(Ordering::Relaxed)),
            correction_ppm: f64::from_bits(self.correction_ppm_bits.load(Ordering::Relaxed)),
            controller_saturation_count: self.controller_saturation_count.load(Ordering::Relaxed),
            resampler_latency_frames: self.resampler_latency_frames.load(Ordering::Relaxed),
            fault: decode_adaptive_fault(self.fault.load(Ordering::Acquire)),
        }
    }
}

/// Fixed duplex bridge configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplexBridgeConfig {
    /// Number of interleaved input channels.
    pub channels: usize,
    /// Total bounded queue capacity in frames.
    pub capacity_frames: usize,
    /// Desired elastic-buffer fill in frames.
    pub target_fill_frames: usize,
    /// Fill deviation before one-frame drift correction is applied.
    pub correction_threshold_frames: usize,
}

/// Duplex bridge setup errors.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DuplexBridgeError {
    /// A fixed configuration value is invalid.
    #[error("invalid duplex bridge configuration")]
    InvalidConfiguration,
}

/// Numeric duplex callback fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum DuplexFault {
    /// No active duplex fault.
    #[default]
    None = 0,
    /// Input callback channel or sample shape changed.
    InputFormat = 1,
    /// Output callback channel or sample shape changed.
    OutputFormat = 2,
}

/// Control-thread snapshot of duplex clocks and elastic-buffer behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplexSnapshot {
    /// Input frames observed by the producer callback.
    pub input_frames_received: u64,
    /// Output frames requested by the consumer callback.
    pub output_frames_requested: u64,
    /// Current queue fill in frames.
    pub fill_frames: usize,
    /// Minimum observed queue fill in frames.
    pub min_fill_frames: usize,
    /// Maximum observed queue fill in frames.
    pub max_fill_frames: usize,
    /// Signed current deviation from target fill.
    pub drift_trend_frames: i64,
    /// Duplicated/interpolated frames used for a slow input clock.
    pub sample_slips_inserted: u64,
    /// Discarded frames used for a fast input clock.
    pub sample_slips_removed: u64,
    /// Input frames rejected because the queue was full.
    pub overflow_count: u64,
    /// Output blocks that could not obtain enough input.
    pub underflow_count: u64,
    /// Current consecutive underflow callbacks.
    pub consecutive_underflows: u64,
    /// Current consecutive overflow callbacks.
    pub consecutive_overflows: u64,
    /// Smallest observed output-frame interval between corrections.
    pub minimum_correction_interval_frames: u64,
    /// Largest absolute fill excursion from the target.
    pub maximum_excursion_frames: usize,
    /// Numeric health state derived from callback-visible policy.
    pub health: DuplexHealth,
    /// Numeric shape fault.
    pub fault: DuplexFault,
}

/// Shared lock-free duplex metrics updated by callbacks.
#[derive(Debug)]
pub struct DuplexStatus {
    input_frames_received: AtomicU64,
    output_frames_requested: AtomicU64,
    fill_frames: AtomicUsize,
    min_fill_frames: AtomicUsize,
    max_fill_frames: AtomicUsize,
    drift_trend_frames: AtomicI64,
    sample_slips_inserted: AtomicU64,
    sample_slips_removed: AtomicU64,
    overflow_count: AtomicU64,
    underflow_count: AtomicU64,
    consecutive_underflows: AtomicU64,
    consecutive_overflows: AtomicU64,
    last_correction_output_frame: AtomicU64,
    minimum_correction_interval_frames: AtomicU64,
    maximum_excursion_frames: AtomicUsize,
    health: AtomicU32,
    fault: AtomicU32,
}

impl DuplexStatus {
    fn new(capacity_frames: usize) -> Self {
        Self {
            input_frames_received: AtomicU64::new(0),
            output_frames_requested: AtomicU64::new(0),
            fill_frames: AtomicUsize::new(0),
            min_fill_frames: AtomicUsize::new(capacity_frames),
            max_fill_frames: AtomicUsize::new(0),
            drift_trend_frames: AtomicI64::new(0),
            sample_slips_inserted: AtomicU64::new(0),
            sample_slips_removed: AtomicU64::new(0),
            overflow_count: AtomicU64::new(0),
            underflow_count: AtomicU64::new(0),
            consecutive_underflows: AtomicU64::new(0),
            consecutive_overflows: AtomicU64::new(0),
            last_correction_output_frame: AtomicU64::new(0),
            minimum_correction_interval_frames: AtomicU64::new(u64::MAX),
            maximum_excursion_frames: AtomicUsize::new(0),
            health: AtomicU32::new(DuplexHealth::Normal as u32),
            fault: AtomicU32::new(DuplexFault::None as u32),
        }
    }

    /// Returns one coherent-enough lock-free metrics snapshot.
    pub fn snapshot(&self) -> DuplexSnapshot {
        DuplexSnapshot {
            input_frames_received: self.input_frames_received.load(Ordering::Relaxed),
            output_frames_requested: self.output_frames_requested.load(Ordering::Relaxed),
            fill_frames: self.fill_frames.load(Ordering::Acquire),
            min_fill_frames: self.min_fill_frames.load(Ordering::Relaxed),
            max_fill_frames: self.max_fill_frames.load(Ordering::Relaxed),
            drift_trend_frames: self.drift_trend_frames.load(Ordering::Relaxed),
            sample_slips_inserted: self.sample_slips_inserted.load(Ordering::Relaxed),
            sample_slips_removed: self.sample_slips_removed.load(Ordering::Relaxed),
            overflow_count: self.overflow_count.load(Ordering::Relaxed),
            underflow_count: self.underflow_count.load(Ordering::Relaxed),
            consecutive_underflows: self.consecutive_underflows.load(Ordering::Relaxed),
            consecutive_overflows: self.consecutive_overflows.load(Ordering::Relaxed),
            minimum_correction_interval_frames: self
                .minimum_correction_interval_frames
                .load(Ordering::Relaxed),
            maximum_excursion_frames: self.maximum_excursion_frames.load(Ordering::Relaxed),
            health: decode_health(self.health.load(Ordering::Acquire)),
            fault: decode_duplex_fault(self.fault.load(Ordering::Acquire)),
        }
    }

    fn observe_fill(&self, fill_frames: usize, target: usize, policy: DuplexFaultPolicy) {
        self.fill_frames.store(fill_frames, Ordering::Release);
        self.min_fill_frames
            .fetch_min(fill_frames, Ordering::Relaxed);
        self.max_fill_frames
            .fetch_max(fill_frames, Ordering::Relaxed);
        let trend = fill_frames as i128 - target as i128;
        self.drift_trend_frames.store(
            trend.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
            Ordering::Relaxed,
        );
        let excursion = trend.unsigned_abs().min(usize::MAX as u128) as usize;
        self.maximum_excursion_frames
            .fetch_max(excursion, Ordering::Relaxed);
        if excursion > policy.maximum_excursion_frames {
            self.raise_health(DuplexHealth::Fatal);
        }
    }

    fn observe_correction(&self, policy: DuplexFaultPolicy) {
        let current = self.output_frames_requested.load(Ordering::Relaxed);
        let previous = self
            .last_correction_output_frame
            .swap(current, Ordering::Relaxed);
        if previous == 0 {
            return;
        }
        let interval = current.saturating_sub(previous);
        self.minimum_correction_interval_frames
            .fetch_min(interval, Ordering::Relaxed);
        if interval < policy.degraded_correction_interval_frames {
            self.raise_health(DuplexHealth::Degraded);
        } else if interval < policy.warning_correction_interval_frames {
            self.raise_health(DuplexHealth::Warning);
        }
    }

    fn raise_health(&self, health: DuplexHealth) {
        self.health.fetch_max(health as u32, Ordering::Relaxed);
    }
}

/// Input-callback side of the selected bounded SPSC frame ring.
pub struct DuplexProducer {
    queue: Arc<SpscSampleRing>,
    status: Arc<DuplexStatus>,
    config: DuplexBridgeConfig,
    policy: DuplexFaultPolicy,
}

impl DuplexProducer {
    /// Pushes one interleaved input callback without allocating or blocking.
    pub fn push_interleaved(&self, input: &[f32], channels: usize) {
        if channels != self.config.channels || channels == 0 || input.len() % channels != 0 {
            self.status
                .fault
                .store(DuplexFault::InputFormat as u32, Ordering::Release);
            return;
        }
        let frames = input.len() / channels;
        self.status
            .input_frames_received
            .fetch_add(frames as u64, Ordering::Relaxed);
        let available_frames = self
            .queue
            .capacity()
            .saturating_sub(self.queue.len_samples())
            / channels;
        let accepted_frames = frames.min(available_frames);
        let accepted_samples = accepted_frames * channels;
        let pushed = self.queue.try_push_samples(&input[..accepted_samples]);
        debug_assert!(pushed);
        let rejected = frames - accepted_frames;
        if rejected > 0 {
            self.status
                .overflow_count
                .fetch_add(rejected as u64, Ordering::Relaxed);
            let consecutive = self
                .status
                .consecutive_overflows
                .fetch_add(1, Ordering::Relaxed)
                + 1;
            if consecutive > self.policy.maximum_consecutive_overflows {
                self.status.raise_health(DuplexHealth::Fatal);
            }
        } else {
            self.status
                .consecutive_overflows
                .store(0, Ordering::Relaxed);
        }
        self.status.observe_fill(
            self.queue.len_samples() / channels,
            self.config.target_fill_frames,
            self.policy,
        );
    }
}

/// Output-callback side of the selected bounded SPSC frame ring.
pub struct DuplexConsumer {
    queue: Arc<SpscSampleRing>,
    status: Arc<DuplexStatus>,
    config: DuplexBridgeConfig,
    policy: DuplexFaultPolicy,
    compensator: Box<dyn DriftCompensator>,
    last_frame: Vec<f32>,
    peek_frame: Vec<f32>,
    transition_frames: Vec<f32>,
    has_last_frame: bool,
}

impl DuplexConsumer {
    /// Reads interleaved input for one output block with bounded correction.
    pub fn read_interleaved(&mut self, output: &mut [f32], channels: usize) {
        if channels != self.config.channels || channels == 0 || output.len() % channels != 0 {
            output.fill(0.0);
            self.status
                .fault
                .store(DuplexFault::OutputFormat as u32, Ordering::Release);
            return;
        }
        let frame_count = output.len() / channels;
        self.status
            .output_frames_requested
            .fetch_add(frame_count as u64, Ordering::Relaxed);
        let fill_before = self.queue.len_samples() / channels;
        let mut correction = self.compensator.select_correction(DriftContext {
            fill_frames: fill_before,
            output_frames: frame_count,
            target_fill_frames: self.config.target_fill_frames,
            threshold_frames: self.config.correction_threshold_frames,
        });
        if !self.has_last_frame && correction == DriftCorrection::Insert {
            correction = DriftCorrection::None;
        }
        if self.compensator.transition() == CorrectionTransition::ZeroCrossing
            && correction != DriftCorrection::None
            && !self.is_coherent_zero_crossing()
        {
            correction = DriftCorrection::None;
        }

        let consumed_frames = match correction {
            DriftCorrection::None => frame_count,
            DriftCorrection::Insert => frame_count.saturating_sub(1),
            DriftCorrection::Remove => frame_count.saturating_add(1),
        };
        if self.queue.len_samples() / channels < consumed_frames {
            output.fill(0.0);
            self.observe_underflow();
            self.observe_fill();
            return;
        }

        let success = match correction {
            DriftCorrection::None => self.queue.try_pop_samples(output),
            DriftCorrection::Insert => self.render_insertion(output, channels),
            DriftCorrection::Remove => self.render_removal(output, channels),
        };
        if !success {
            output.fill(0.0);
            self.observe_underflow();
        } else {
            self.status
                .consecutive_underflows
                .store(0, Ordering::Relaxed);
            if let Some(last) = output.chunks_exact(channels).last() {
                self.last_frame.copy_from_slice(last);
                self.has_last_frame = true;
            }
        }
        self.observe_fill();
    }

    /// Returns fixed callback-owned storage capacity for allocation guards.
    pub fn fixed_capacity(&self) -> usize {
        self.queue.capacity()
            + self.last_frame.capacity()
            + self.peek_frame.capacity()
            + self.transition_frames.capacity()
    }

    fn render_insertion(&mut self, output: &mut [f32], channels: usize) -> bool {
        let success = match self.compensator.transition() {
            CorrectionTransition::Raw => {
                output[..channels].copy_from_slice(&self.last_frame);
                self.queue.try_pop_samples(&mut output[channels..])
            }
            CorrectionTransition::LinearInterpolation | CorrectionTransition::ZeroCrossing => {
                self.render_smoothed_insertion(output, channels, 2)
            }
            CorrectionTransition::Crossfade => self.render_smoothed_insertion(
                output,
                channels,
                self.compensator.correction_window_frames(),
            ),
        };
        if success {
            self.status
                .sample_slips_inserted
                .fetch_add(1, Ordering::Relaxed);
            self.status.observe_correction(self.policy);
        }
        success
    }

    fn render_smoothed_insertion(
        &mut self,
        output: &mut [f32],
        channels: usize,
        requested_window: usize,
    ) -> bool {
        let output_frames = output.len() / channels;
        if output_frames < 2 {
            output.copy_from_slice(&self.last_frame);
            return true;
        }
        let window = requested_window.min(output_frames).max(2);
        let mut consumed = 0;
        let mut frames = output.chunks_exact_mut(channels);
        for frame_index in 0..window {
            let source_position = frame_index as f32 * (window - 1) as f32 / window as f32;
            let source_floor = source_position.floor() as usize;
            while consumed < source_floor {
                if !self.queue.discard_samples(channels) {
                    return false;
                }
                consumed += 1;
            }
            if !self.queue.peek_samples(&mut self.transition_frames) {
                return false;
            }
            let fraction = source_position - source_floor as f32;
            let Some(frame) = frames.next() else {
                return false;
            };
            for (channel, sample) in frame.iter_mut().enumerate() {
                let current = self.transition_frames[channel];
                let next = self.transition_frames[channels + channel];
                *sample = current.mul_add(1.0 - fraction, next * fraction);
            }
        }
        while consumed < window - 1 {
            if !self.queue.discard_samples(channels) {
                return false;
            }
            consumed += 1;
        }
        for frame in frames {
            if !self.queue.try_pop_samples(frame) {
                return false;
            }
        }
        true
    }

    fn render_removal(&mut self, output: &mut [f32], channels: usize) -> bool {
        let success = match self.compensator.transition() {
            CorrectionTransition::Raw => {
                self.queue.discard_samples(channels) && self.queue.try_pop_samples(output)
            }
            CorrectionTransition::LinearInterpolation | CorrectionTransition::ZeroCrossing => {
                self.render_smoothed_removal(output, channels, 1)
            }
            CorrectionTransition::Crossfade => self.render_smoothed_removal(
                output,
                channels,
                self.compensator.correction_window_frames(),
            ),
        };
        if !success {
            return false;
        }
        self.status
            .sample_slips_removed
            .fetch_add(1, Ordering::Relaxed);
        self.status.observe_correction(self.policy);
        true
    }

    fn render_smoothed_removal(
        &mut self,
        output: &mut [f32],
        channels: usize,
        requested_window: usize,
    ) -> bool {
        let window = requested_window.min(output.len() / channels).max(1);
        let mut frames = output.chunks_exact_mut(channels);
        for frame_index in 0..window {
            let Some(frame) = frames.next() else {
                return false;
            };
            if !self.queue.peek_samples(&mut self.transition_frames) {
                return false;
            }
            let alpha = (frame_index + 1) as f32 / (window + 1) as f32;
            for (channel, sample) in frame.iter_mut().enumerate() {
                let current = self.transition_frames[channel];
                let next = self.transition_frames[channels + channel];
                *sample = current.mul_add(1.0 - alpha, next * alpha);
            }
            if !self.queue.discard_samples(channels) {
                return false;
            }
        }
        if !self.queue.discard_samples(channels) {
            return false;
        }
        for frame in frames {
            if !self.queue.try_pop_samples(frame) {
                return false;
            }
        }
        true
    }

    fn is_coherent_zero_crossing(&mut self) -> bool {
        self.queue.peek_samples(&mut self.peek_frame)
            && self
                .last_frame
                .iter()
                .zip(&self.peek_frame)
                .all(|(previous, next)| {
                    previous.abs() <= 0.001 || next.abs() <= 0.001 || previous * next <= 0.0
                })
    }

    fn observe_underflow(&self) {
        self.status.underflow_count.fetch_add(1, Ordering::Relaxed);
        let consecutive = self
            .status
            .consecutive_underflows
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        if consecutive > self.policy.maximum_consecutive_underflows {
            self.status.raise_health(DuplexHealth::Fatal);
        }
    }

    fn observe_fill(&self) {
        self.status.observe_fill(
            self.queue.len_samples() / self.config.channels,
            self.config.target_fill_frames,
            self.policy,
        );
    }
}

/// Output-callback consumer using adaptive asynchronous resampling.
pub struct AdaptiveDuplexConsumer {
    queue: Arc<SpscSampleRing>,
    duplex_status: Arc<DuplexStatus>,
    adaptive_status: Arc<AdaptiveDuplexStatus>,
    config: DuplexBridgeConfig,
    policy: DuplexFaultPolicy,
    resampler: Box<dyn AsynchronousResampler>,
    controller: DriftController,
    input_scratch: Vec<f32>,
    output_cache: Vec<f32>,
    cache_cursor_samples: usize,
}

impl AdaptiveDuplexConsumer {
    /// Fills captured-input frames for one output callback without raw slips.
    ///
    /// Host callback sizes may differ from the configured processing block. The
    /// ASRC always produces a fixed block into preallocated cache, and this
    /// method drains that cache across arbitrary borrowed output slices.
    pub fn read_interleaved(&mut self, output: &mut [f32], channels: usize) {
        if channels != self.config.channels || channels == 0 || output.len() % channels != 0 {
            output.fill(0.0);
            self.duplex_status
                .fault
                .store(DuplexFault::OutputFormat as u32, Ordering::Release);
            return;
        }
        let requested_frames = output.len() / channels;
        self.duplex_status
            .output_frames_requested
            .fetch_add(requested_frames as u64, Ordering::Relaxed);
        let mut output_cursor = 0;
        let mut underflow = false;
        while output_cursor < output.len() {
            if self.cache_cursor_samples >= self.output_cache.len() && !self.refill_cache() {
                output[output_cursor..].fill(0.0);
                underflow = true;
                break;
            }
            let available = self.output_cache.len() - self.cache_cursor_samples;
            let copied = available.min(output.len() - output_cursor);
            output[output_cursor..output_cursor + copied].copy_from_slice(
                &self.output_cache[self.cache_cursor_samples..self.cache_cursor_samples + copied],
            );
            self.cache_cursor_samples += copied;
            output_cursor += copied;
        }
        if underflow {
            self.duplex_status
                .underflow_count
                .fetch_add(1, Ordering::Relaxed);
            let consecutive = self
                .duplex_status
                .consecutive_underflows
                .fetch_add(1, Ordering::Relaxed)
                + 1;
            if consecutive > self.policy.maximum_consecutive_underflows {
                self.duplex_status.raise_health(DuplexHealth::Fatal);
            }
        } else {
            self.duplex_status
                .consecutive_underflows
                .store(0, Ordering::Relaxed);
        }
        self.duplex_status.observe_fill(
            self.queue.len_samples() / channels,
            self.config.target_fill_frames,
            self.policy,
        );
    }

    /// Returns all fixed callback-owned sample capacity.
    pub fn fixed_capacity(&self) -> usize {
        self.queue.capacity() + self.input_scratch.capacity() + self.output_cache.capacity()
    }

    fn refill_cache(&mut self) -> bool {
        let fill_frames = self.queue.len_samples() / self.config.channels;
        let fill_trend = fill_frames as i128 - self.config.target_fill_frames as i128;
        let controller = self.controller.update(
            fill_frames,
            fill_trend.clamp(i64::MIN as i128, i64::MAX as i128) as i64,
            self.output_cache.len() / self.config.channels,
        );
        let report = match controller {
            Ok(report) => report,
            Err(DriftControllerFault::CorrectionOutOfRange) => {
                self.adaptive_status
                    .fault
                    .store(AdaptiveDuplexFault::Controller as u32, Ordering::Release);
                self.duplex_status.raise_health(DuplexHealth::Fatal);
                return false;
            }
            Err(DriftControllerFault::InvalidConfiguration) => {
                self.adaptive_status
                    .fault
                    .store(AdaptiveDuplexFault::Controller as u32, Ordering::Release);
                return false;
            }
        };
        if self.resampler.set_ratio(report.ratio).is_err() {
            self.adaptive_status
                .fault
                .store(AdaptiveDuplexFault::Resampler as u32, Ordering::Release);
            return false;
        }
        self.adaptive_status
            .current_ratio_bits
            .store(report.ratio.to_bits(), Ordering::Release);
        self.adaptive_status
            .minimum_ratio_bits
            .store(report.minimum_ratio.to_bits(), Ordering::Relaxed);
        self.adaptive_status
            .maximum_ratio_bits
            .store(report.maximum_ratio.to_bits(), Ordering::Relaxed);
        self.adaptive_status
            .correction_ppm_bits
            .store(report.correction_ppm.to_bits(), Ordering::Relaxed);
        self.adaptive_status
            .controller_saturation_count
            .store(report.saturation_count, Ordering::Relaxed);

        let required_frames = self.resampler.required_input_frames();
        let required_samples = required_frames.saturating_mul(self.config.channels);
        if required_samples > self.input_scratch.len() {
            self.adaptive_status
                .fault
                .store(AdaptiveDuplexFault::InputCapacity as u32, Ordering::Release);
            return false;
        }
        if self.queue.len_samples() < required_samples
            || !self
                .queue
                .try_pop_samples(&mut self.input_scratch[..required_samples])
        {
            return false;
        }
        if self
            .resampler
            .process(
                &self.input_scratch[..required_samples],
                &mut self.output_cache,
            )
            .is_err()
        {
            self.adaptive_status
                .fault
                .store(AdaptiveDuplexFault::Resampler as u32, Ordering::Release);
            return false;
        }
        self.cache_cursor_samples = 0;
        true
    }
}

/// Creates the production-oriented adaptive duplex bridge.
pub fn create_adaptive_duplex_bridge(
    config: DuplexBridgeConfig,
    policy: DuplexFaultPolicy,
    controller_config: DriftControllerConfig,
    mut resampler: Box<dyn AsynchronousResampler>,
    max_output_block_frames: usize,
) -> Result<
    (
        DuplexProducer,
        AdaptiveDuplexConsumer,
        Arc<AdaptiveDuplexStatus>,
    ),
    DuplexBridgeError,
> {
    if config.channels == 0
        || config.capacity_frames == 0
        || config.target_fill_frames >= config.capacity_frames
        || max_output_block_frames == 0
        || controller_config.target_fill_frames != config.target_fill_frames
    {
        return Err(DuplexBridgeError::InvalidConfiguration);
    }
    let sample_capacity = config
        .capacity_frames
        .checked_mul(config.channels)
        .ok_or(DuplexBridgeError::InvalidConfiguration)?;
    resampler
        .configure(
            controller_config.input_rate,
            controller_config.output_rate,
            config.channels,
            max_output_block_frames,
        )
        .map_err(|_| DuplexBridgeError::InvalidConfiguration)?;
    let controller = DriftController::new(controller_config)
        .map_err(|_| DuplexBridgeError::InvalidConfiguration)?;
    let queue = Arc::new(
        SpscSampleRing::new(sample_capacity).ok_or(DuplexBridgeError::InvalidConfiguration)?,
    );
    let duplex_status = Arc::new(DuplexStatus::new(config.capacity_frames));
    let adaptive_status = Arc::new(AdaptiveDuplexStatus::new(
        Arc::clone(&duplex_status),
        controller.nominal_ratio(),
        resampler.latency_frames(),
    ));
    Ok((
        DuplexProducer {
            queue: Arc::clone(&queue),
            status: Arc::clone(&duplex_status),
            config,
            policy,
        },
        AdaptiveDuplexConsumer {
            queue,
            duplex_status,
            adaptive_status: Arc::clone(&adaptive_status),
            config,
            policy,
            resampler,
            controller,
            input_scratch: vec![0.0; sample_capacity],
            output_cache: vec![0.0; max_output_block_frames * config.channels],
            cache_cursor_samples: max_output_block_frames * config.channels,
        },
        adaptive_status,
    ))
}

/// Creates the selected frame-ring bridge with crossfade sample-slip reference compensation.
pub fn create_duplex_bridge(
    config: DuplexBridgeConfig,
) -> Result<(DuplexProducer, DuplexConsumer, Arc<DuplexStatus>), DuplexBridgeError> {
    create_duplex_bridge_with_compensator(
        config,
        DuplexFaultPolicy::default(),
        Box::new(ThresholdDriftCompensator::default()),
    )
}

/// Creates a frame-ring bridge with an Aurora-owned correction strategy and policy.
pub fn create_duplex_bridge_with_compensator(
    config: DuplexBridgeConfig,
    policy: DuplexFaultPolicy,
    compensator: Box<dyn DriftCompensator>,
) -> Result<(DuplexProducer, DuplexConsumer, Arc<DuplexStatus>), DuplexBridgeError> {
    if config.channels == 0
        || config.capacity_frames == 0
        || config.target_fill_frames >= config.capacity_frames
        || config.correction_threshold_frames >= config.capacity_frames
    {
        return Err(DuplexBridgeError::InvalidConfiguration);
    }
    let sample_capacity = config
        .capacity_frames
        .checked_mul(config.channels)
        .ok_or(DuplexBridgeError::InvalidConfiguration)?;
    let queue = Arc::new(
        SpscSampleRing::new(sample_capacity).ok_or(DuplexBridgeError::InvalidConfiguration)?,
    );
    let status = Arc::new(DuplexStatus::new(config.capacity_frames));
    Ok((
        DuplexProducer {
            queue: Arc::clone(&queue),
            status: Arc::clone(&status),
            config,
            policy,
        },
        DuplexConsumer {
            queue,
            status: Arc::clone(&status),
            config,
            policy,
            compensator,
            last_frame: vec![0.0; config.channels],
            peek_frame: vec![0.0; config.channels],
            transition_frames: vec![0.0; config.channels * 2],
            has_last_frame: false,
        },
        status,
    ))
}

fn decode_duplex_fault(value: u32) -> DuplexFault {
    match value {
        value if value == DuplexFault::InputFormat as u32 => DuplexFault::InputFormat,
        value if value == DuplexFault::OutputFormat as u32 => DuplexFault::OutputFormat,
        _ => DuplexFault::None,
    }
}

fn decode_health(value: u32) -> DuplexHealth {
    match value {
        value if value == DuplexHealth::Warning as u32 => DuplexHealth::Warning,
        value if value == DuplexHealth::Degraded as u32 => DuplexHealth::Degraded,
        value if value == DuplexHealth::Fatal as u32 => DuplexHealth::Fatal,
        _ => DuplexHealth::Normal,
    }
}

fn decode_adaptive_fault(value: u32) -> AdaptiveDuplexFault {
    match value {
        value if value == AdaptiveDuplexFault::Controller as u32 => AdaptiveDuplexFault::Controller,
        value if value == AdaptiveDuplexFault::Resampler as u32 => AdaptiveDuplexFault::Resampler,
        value if value == AdaptiveDuplexFault::InputCapacity as u32 => {
            AdaptiveDuplexFault::InputCapacity
        }
        _ => AdaptiveDuplexFault::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::correction_artifact_metrics;

    fn config(channels: usize) -> DuplexBridgeConfig {
        DuplexBridgeConfig {
            channels,
            capacity_frames: 512,
            target_fill_frames: 128,
            correction_threshold_frames: 8,
        }
    }

    fn bridge(
        channels: usize,
        transition: CorrectionTransition,
    ) -> (DuplexProducer, DuplexConsumer, Arc<DuplexStatus>) {
        create_duplex_bridge_with_compensator(
            config(channels),
            DuplexFaultPolicy {
                maximum_excursion_frames: 512,
                ..DuplexFaultPolicy::default()
            },
            Box::new(ThresholdDriftCompensator::new(transition, 16)),
        )
        .unwrap()
    }

    fn prefill(producer: &DuplexProducer, frames: usize, channels: usize) {
        producer.push_interleaved(&vec![0.0; frames * channels], channels);
    }

    #[test]
    fn duplex_ring_buffer_schedules_input_to_output() {
        let (producer, mut consumer, status) = bridge(2, CorrectionTransition::Raw);
        let input = (0..128).map(|sample| sample as f32).collect::<Vec<_>>();
        producer.push_interleaved(&input, 2);
        let mut output = vec![0.0; 128];
        consumer.read_interleaved(&mut output, 2);
        assert_eq!(output, input);
        assert_eq!(status.snapshot().underflow_count, 0);
    }

    #[test]
    fn input_clock_faster_than_output_removes_samples_and_stays_bounded() {
        let (producer, mut consumer, status) = bridge(1, CorrectionTransition::Raw);
        prefill(&producer, 128, 1);
        let input = vec![0.25; 65];
        let mut output = vec![0.0; 64];
        for _ in 0..20_000 {
            producer.push_interleaved(&input, 1);
            consumer.read_interleaved(&mut output, 1);
        }
        let snapshot = status.snapshot();
        assert_eq!(snapshot.sample_slips_removed, 19_992);
        assert_eq!(snapshot.overflow_count, 0);
        assert_eq!(snapshot.fill_frames, 136);
    }

    #[test]
    fn output_clock_faster_than_input_inserts_samples_and_stays_bounded() {
        let (producer, mut consumer, status) = bridge(1, CorrectionTransition::Raw);
        prefill(&producer, 128, 1);
        let input = vec![0.25; 63];
        let mut output = vec![0.0; 64];
        for _ in 0..20_000 {
            producer.push_interleaved(&input, 1);
            consumer.read_interleaved(&mut output, 1);
        }
        let snapshot = status.snapshot();
        assert_eq!(snapshot.sample_slips_inserted, 19_992);
        assert_eq!(snapshot.underflow_count, 0);
        assert_eq!(snapshot.fill_frames, 120);
    }

    #[test]
    fn prolonged_alternating_stress_has_no_unbounded_growth() {
        let (producer, mut consumer, status) = bridge(1, CorrectionTransition::Raw);
        prefill(&producer, 128, 1);
        let faster = vec![0.1; 65];
        let slower = vec![0.1; 63];
        let mut output = vec![0.0; 64];
        for cycle in 0..100_000 {
            producer.push_interleaved(if cycle % 2 == 0 { &faster } else { &slower }, 1);
            consumer.read_interleaved(&mut output, 1);
        }
        let snapshot = status.snapshot();
        assert_eq!(snapshot.overflow_count, 0);
        assert_eq!(snapshot.underflow_count, 0);
        assert_eq!(snapshot.fill_frames, 128);
    }

    #[test]
    fn underflow_outputs_silence_and_is_counted() {
        let (_producer, mut consumer, status) = bridge(2, CorrectionTransition::Crossfade);
        let mut output = vec![1.0; 128];
        consumer.read_interleaved(&mut output, 2);
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert_eq!(status.snapshot().underflow_count, 1);
    }

    #[test]
    fn overflow_is_counted_without_growing_capacity() {
        let mut small = config(2);
        small.capacity_frames = 16;
        small.target_fill_frames = 8;
        small.correction_threshold_frames = 2;
        let (producer, consumer, status) = create_duplex_bridge(small).unwrap();
        let capacity = consumer.fixed_capacity();
        producer.push_interleaved(&[0.0; 64], 2);
        assert_eq!(status.snapshot().overflow_count, 16);
        assert_eq!(consumer.fixed_capacity(), capacity);
    }

    #[test]
    fn correction_is_coherent_across_multichannel_frames() {
        let (producer, mut consumer, status) = bridge(2, CorrectionTransition::Crossfade);
        let mut input = Vec::new();
        for frame in 0..256 {
            input.push(frame as f32 * 0.001);
            input.push(10.0 + frame as f32 * 0.001);
        }
        producer.push_interleaved(&input, 2);
        let mut output = vec![0.0; 128];
        consumer.read_interleaved(&mut output, 2);
        producer.push_interleaved(&input, 2);
        consumer.read_interleaved(&mut output, 2);
        assert!(status.snapshot().sample_slips_removed > 0);
        for frame in output.chunks_exact(2) {
            assert!((frame[1] - frame[0] - 10.0).abs() < 0.000_01);
        }
    }

    #[test]
    fn insertion_is_coherent_across_multichannel_frames() {
        let (producer, mut consumer, status) = bridge(2, CorrectionTransition::Crossfade);
        let paired = (0..191)
            .flat_map(|frame| [frame as f32 * 0.001, 10.0 + frame as f32 * 0.001])
            .collect::<Vec<_>>();
        producer.push_interleaved(&paired[..128 * 2], 2);
        let mut output = vec![0.0; 64 * 2];
        consumer.read_interleaved(&mut output, 2);
        producer.push_interleaved(&paired[128 * 2..], 2);
        consumer.read_interleaved(&mut output, 2);
        assert!(status.snapshot().sample_slips_inserted > 0);
        for frame in output.chunks_exact(2) {
            assert!((frame[1] - frame[0] - 10.0).abs() < 0.000_01);
        }
    }

    #[test]
    fn correction_output_is_deterministic() {
        let first = corrected_sine(CorrectionTransition::Crossfade);
        let second = corrected_sine(CorrectionTransition::Crossfade);
        assert_eq!(first, second);
        assert!(first.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn crossfade_removal_reduces_discontinuity_against_raw() {
        let raw = corrected_sine(CorrectionTransition::Raw);
        let crossfade = corrected_sine(CorrectionTransition::Crossfade);
        let reference = (63..128)
            .map(|frame| (std::f32::consts::TAU * 750.0 * frame as f32 / 48_000.0).sin() * 0.5)
            .collect::<Vec<_>>();
        let raw_metrics = correction_artifact_metrics(&reference, &raw);
        let crossfade_metrics = correction_artifact_metrics(&reference, &crossfade);
        assert!(crossfade_metrics.maximum_discontinuity < raw_metrics.maximum_discontinuity);
    }

    #[test]
    fn silence_remains_silent_and_all_samples_are_finite() {
        let (producer, mut consumer, _) = bridge(6, CorrectionTransition::Crossfade);
        producer.push_interleaved(&vec![0.0; 256 * 6], 6);
        let mut output = vec![f32::NAN; 64 * 6];
        consumer.read_interleaved(&mut output, 6);
        assert!(output
            .iter()
            .all(|sample| *sample == 0.0 && sample.is_finite()));
    }

    #[test]
    fn malformed_shapes_set_fault_without_nan_or_panic() {
        let (producer, mut consumer, status) = bridge(1, CorrectionTransition::Raw);
        producer.push_interleaved(&[f32::NAN], 2);
        let mut output = [f32::NAN; 3];
        consumer.read_interleaved(&mut output, 2);
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert_eq!(status.snapshot().fault, DuplexFault::OutputFormat);
    }

    #[test]
    fn shutdown_by_drop_has_no_deadlock() {
        drop(bridge(2, CorrectionTransition::Crossfade));
    }

    #[test]
    fn adaptive_bridge_processes_rate_mismatch_without_raw_slips() {
        let adaptive_config = DuplexBridgeConfig {
            channels: 2,
            capacity_frames: 4_096,
            target_fill_frames: 1_024,
            correction_threshold_frames: 128,
        };
        let controller = DriftControllerConfig {
            input_rate: 44_100,
            output_rate: 48_000,
            target_fill_frames: 1_024,
            ..DriftControllerConfig::default()
        };
        let (producer, mut consumer, status) = create_adaptive_duplex_bridge(
            adaptive_config,
            DuplexFaultPolicy {
                maximum_excursion_frames: 4_096,
                ..DuplexFaultPolicy::default()
            },
            controller,
            Box::new(crate::RubatoAsrc::default()),
            256,
        )
        .unwrap();
        producer.push_interleaved(&vec![0.0; 2_048 * 2], 2);
        let capacity = consumer.fixed_capacity();
        let mut output = vec![1.0; 480 * 2];
        for _ in 0..4 {
            consumer.read_interleaved(&mut output, 2);
            assert!(output.iter().all(|sample| sample.is_finite()));
        }
        let snapshot = status.snapshot();
        assert_eq!(snapshot.duplex.sample_slips_inserted, 0);
        assert_eq!(snapshot.duplex.sample_slips_removed, 0);
        assert_eq!(snapshot.fault, AdaptiveDuplexFault::None);
        assert_eq!(consumer.fixed_capacity(), capacity);
    }

    fn corrected_sine(transition: CorrectionTransition) -> Vec<f32> {
        let (producer, mut consumer, _) = bridge(1, transition);
        let signal = (0..266)
            .map(|frame| (std::f32::consts::TAU * 750.0 * frame as f32 / 48_000.0).sin() * 0.5)
            .collect::<Vec<_>>();
        producer.push_interleaved(&signal[..128], 1);
        let mut warmup = vec![0.0; 64];
        consumer.read_interleaved(&mut warmup, 1);
        producer.push_interleaved(&signal[128..], 1);
        let mut output = vec![0.0; 64];
        consumer.read_interleaved(&mut output, 1);
        let mut with_boundary = Vec::with_capacity(output.len() + 1);
        with_boundary.push(*warmup.last().unwrap());
        with_boundary.extend(output);
        with_boundary
    }
}
