use std::{
    cell::UnsafeCell,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use aurora_audio_io::write_wav_f32;
use aurora_core::{ChannelRole, Listener, Speaker, StandardLayout, Vector3};
use aurora_realtime_audio_api::{
    AudioInputBackend, AudioOutputBackend, AudioStreamFault, RealTimeAudioConfig,
    RealTimeSampleFormat,
};
use aurora_realtime_audio_cpal::CpalAudioBackend;
use aurora_realtime_engine::{
    create_adaptive_duplex_bridge, estimate_repeated_latency, generate_measurement_sequence,
    AdaptiveDuplexFault, BasicRendererMode, DriftControllerConfig, DuplexBridgeConfig,
    DuplexFaultPolicy, DuplexHealth, DuplexStateEvent, DuplexStateMachine, DuplexStreamState,
    ProcessStatus, RealTimeEngine, RealTimeEngineConfig, RubatoAsrc, TestSignal,
};
use aurora_runtime_materialization::materialize_default_realtime_engine;
use aurora_scene::{RenderScene, SceneObject, Trajectory};
use serde::Serialize;

const CALLBACK_HISTOGRAM_BUCKETS: usize = 32;

pub struct DuplexOptions {
    pub input_device: String,
    pub output_device: String,
    pub requested_rate: u32,
    pub input_rate: u32,
    pub output_rate: u32,
    pub block_size: usize,
    pub channels: usize,
    pub duration_seconds: u64,
    pub restart_attempts: u32,
    pub print_status: bool,
}

pub struct LatencyOptions {
    pub input_device: String,
    pub output_device: String,
    pub sample_rate: u32,
    pub block_size: usize,
    pub channels: usize,
    pub duration_seconds: u64,
    pub save_capture: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplexSummary {
    pub requested_sample_rate: u32,
    pub negotiated_input_rate: u32,
    pub negotiated_output_rate: u32,
    pub channels: usize,
    pub requested_block_size: usize,
    pub actual_duration_seconds: f64,
    pub input_callbacks: u64,
    pub output_callbacks: u64,
    pub frames_captured: u64,
    pub frames_rendered: u64,
    pub ring_fill_current: usize,
    pub ring_fill_minimum: usize,
    pub ring_fill_maximum: usize,
    pub underflows: u64,
    pub overflows: u64,
    pub dropped_frames: u64,
    pub current_ratio: f64,
    pub correction_ppm: f64,
    pub minimum_ratio: f64,
    pub maximum_ratio: f64,
    pub controller_saturation_count: u64,
    pub input_average_callback_ms: f64,
    pub input_maximum_callback_ms: f64,
    pub input_p95_callback_ms: f64,
    pub output_average_callback_ms: f64,
    pub output_maximum_callback_ms: f64,
    pub output_p95_callback_ms: f64,
    pub renderer_latency_frames: usize,
    pub dsp_latency_frames: usize,
    pub resampler_latency_frames: usize,
    pub estimated_software_buffering_frames: usize,
    pub state: String,
    pub stream_fault: String,
    pub adaptive_fault: String,
    pub engine_fault_code: u64,
    pub memory_start_bytes: Option<u64>,
    pub memory_end_bytes: Option<u64>,
    pub memory_growth_bytes: Option<i64>,
}

pub fn run_duplex(options: DuplexOptions) -> Result<DuplexSummary> {
    let mut attempt = 0;
    loop {
        match run_duplex_once(&options) {
            Ok((summary, true)) if attempt < options.restart_attempts => {
                attempt += 1;
                eprintln!(
                    "duplex_restart_attempt={attempt} fault={}",
                    summary.stream_fault
                );
                std::thread::sleep(Duration::from_millis(500));
            }
            Ok((summary, _)) => return Ok(summary),
            Err(error) if attempt < options.restart_attempts => {
                attempt += 1;
                eprintln!("duplex_restart_attempt={attempt} error={error}");
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn run_soak(options: DuplexOptions, report_path: &Path) -> Result<DuplexSummary> {
    let summary = run_duplex(options)?;
    if let Some(parent) = report_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(report_path, serde_json::to_vec_pretty(&summary)?)?;
    Ok(summary)
}

fn run_duplex_once(options: &DuplexOptions) -> Result<(DuplexSummary, bool)> {
    if options.channels == 0 || options.block_size < 2 || options.duration_seconds == 0 {
        bail!("channels, block size, and duration must be greater than zero");
    }
    let mut state = DuplexStateMachine::default();
    state.transition(DuplexStateEvent::StartRequested)?;
    let target_fill = options.block_size.saturating_mul(8);
    let capacity = options.block_size.saturating_mul(32);
    let bridge_config = DuplexBridgeConfig {
        channels: options.channels,
        capacity_frames: capacity,
        target_fill_frames: target_fill,
        correction_threshold_frames: options.block_size,
    };
    let fault_policy = DuplexFaultPolicy {
        maximum_excursion_frames: capacity,
        maximum_consecutive_underflows: 3,
        maximum_consecutive_overflows: 1,
        ..DuplexFaultPolicy::default()
    };
    let controller_config = DriftControllerConfig {
        input_rate: options.input_rate,
        output_rate: options.output_rate,
        target_fill_frames: target_fill,
        ..DriftControllerConfig::default()
    };
    let (producer, mut consumer, adaptive_status) = create_adaptive_duplex_bridge(
        bridge_config,
        fault_policy,
        controller_config,
        Box::new(RubatoAsrc::default()),
        options.block_size,
    )
    .context("create adaptive duplex bridge")?;

    let scene = live_scene(options.channels, options.block_size)?;
    let mut engine = materialize_default_realtime_engine(
        scene,
        RealTimeEngineConfig {
            sample_rate: options.output_rate,
            block_size: options.block_size,
            input_channels: options.channels,
            apply_geometric_delay: false,
            speed_of_sound: 343.0,
            test_signal: TestSignal::None,
            renderer_mode: BasicRendererMode::InverseDistance,
        },
        options.block_size,
    )?;
    let renderer_latency_frames = engine.metrics().renderer_latency_frames;
    let dsp_latency_frames = engine.metrics().dsp_latency_frames;
    let input_metrics = Arc::new(CallbackMetrics::new());
    let output_metrics = Arc::new(CallbackMetrics::new());
    let input_for_callback = Arc::clone(&input_metrics);
    let output_for_callback = Arc::clone(&output_metrics);
    let backend = CpalAudioBackend::new();
    let audio_config = RealTimeAudioConfig {
        sample_rate: options.requested_rate,
        input_sample_rate: Some(options.input_rate),
        output_sample_rate: Some(options.output_rate),
        block_size: options.block_size,
        input_channels: options.channels,
        output_channels: options.channels,
        sample_format: RealTimeSampleFormat::F32,
        input_device_id: Some(options.input_device.clone()),
        output_device_id: Some(options.output_device.clone()),
    };
    let mut input_stream = backend.open_input(
        &audio_config,
        Box::new(move |input, channels| {
            let started = Instant::now();
            producer.push_interleaved(input, channels);
            input_for_callback.record(input.len() / channels.max(1), started.elapsed());
        }),
    )?;
    let mut input_scratch = vec![0.0_f32; options.block_size * options.channels];
    let mut output_stream = backend.open_output(
        &audio_config,
        Box::new(move |output, channels| {
            let started = Instant::now();
            let chunk_samples = input_scratch.len();
            let mut rendered_frames = 0;
            for chunk in output.chunks_mut(chunk_samples) {
                consumer.read_interleaved(&mut input_scratch[..chunk.len()], channels);
                let status = engine.process_interleaved(Some(&input_scratch[..chunk.len()]), chunk);
                if matches!(status, ProcessStatus::Fault(_)) {
                    chunk.fill(0.0);
                    output_for_callback.set_fault(engine.metrics().fault as u64);
                }
                rendered_frames += chunk.len() / channels.max(1);
            }
            output_for_callback.record(rendered_frames, started.elapsed());
        }),
    )?;
    let negotiated_input = input_stream.negotiated_config().clone();
    let negotiated_output = output_stream.negotiated_config().clone();
    if negotiated_input.sample_rate != options.input_rate
        || negotiated_output.sample_rate != options.output_rate
        || negotiated_input.channels != options.channels
        || negotiated_output.channels != options.channels
    {
        bail!("backend returned a format different from the validated adaptive configuration");
    }

    input_stream.start()?;
    let prefill_started = Instant::now();
    while adaptive_status.snapshot().duplex.fill_frames < target_fill
        && prefill_started.elapsed() < Duration::from_secs(2)
    {
        if input_stream.fault() != AudioStreamFault::None {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if adaptive_status.snapshot().duplex.fill_frames < target_fill {
        let _ = input_stream.stop();
        bail!("input stream did not provide enough captured frames for duplex startup");
    }
    output_stream.start()?;
    state.transition(DuplexStateEvent::StreamsStarted)?;
    let memory_start = current_working_set_bytes();
    let started = Instant::now();
    let mut terminal_fault = AudioStreamFault::None;
    while started.elapsed() < Duration::from_secs(options.duration_seconds) {
        std::thread::sleep(Duration::from_secs(1));
        let snapshot = adaptive_status.snapshot();
        terminal_fault = merge_faults(input_stream.fault(), output_stream.fault());
        if terminal_fault != AudioStreamFault::None
            || snapshot.fault != AdaptiveDuplexFault::None
            || snapshot.duplex.health == DuplexHealth::Fatal
            || output_metrics.fault.load(Ordering::Acquire) != 0
        {
            let fault = if terminal_fault == AudioStreamFault::None {
                AudioStreamFault::Callback
            } else {
                terminal_fault
            };
            state.transition(DuplexStateEvent::StreamFault(fault))?;
            break;
        }
        if snapshot.duplex.health == DuplexHealth::Degraded
            && state.state() == DuplexStreamState::Running
        {
            state.transition(DuplexStateEvent::QualityDegraded)?;
        }
        if options.print_status {
            print_live_status(
                started.elapsed(),
                &snapshot,
                &input_metrics,
                &output_metrics,
                state.state(),
            );
        }
    }
    let actual_duration = started.elapsed();
    state.transition(DuplexStateEvent::StopRequested)?;
    let output_stop = output_stream.stop();
    let input_stop = input_stream.stop();
    state.transition(DuplexStateEvent::StreamsStopped)?;
    output_stop?;
    input_stop?;
    let memory_end = current_working_set_bytes();
    let adaptive = adaptive_status.snapshot();
    let input_timing = input_metrics.snapshot();
    let output_timing = output_metrics.snapshot();
    let faulted = terminal_fault != AudioStreamFault::None
        || adaptive.fault != AdaptiveDuplexFault::None
        || adaptive.duplex.health == DuplexHealth::Fatal
        || output_timing.fault != 0;
    let summary = DuplexSummary {
        requested_sample_rate: options.requested_rate,
        negotiated_input_rate: negotiated_input.sample_rate,
        negotiated_output_rate: negotiated_output.sample_rate,
        channels: options.channels,
        requested_block_size: options.block_size,
        actual_duration_seconds: actual_duration.as_secs_f64(),
        input_callbacks: input_timing.callbacks,
        output_callbacks: output_timing.callbacks,
        frames_captured: input_timing.frames,
        frames_rendered: output_timing.frames,
        ring_fill_current: adaptive.duplex.fill_frames,
        ring_fill_minimum: adaptive.duplex.min_fill_frames,
        ring_fill_maximum: adaptive.duplex.max_fill_frames,
        underflows: adaptive.duplex.underflow_count,
        overflows: adaptive.duplex.overflow_count,
        dropped_frames: adaptive.duplex.overflow_count,
        current_ratio: adaptive.current_ratio,
        correction_ppm: adaptive.correction_ppm,
        minimum_ratio: adaptive.minimum_ratio,
        maximum_ratio: adaptive.maximum_ratio,
        controller_saturation_count: adaptive.controller_saturation_count,
        input_average_callback_ms: input_timing.average_ms,
        input_maximum_callback_ms: input_timing.maximum_ms,
        input_p95_callback_ms: input_timing.p95_ms,
        output_average_callback_ms: output_timing.average_ms,
        output_maximum_callback_ms: output_timing.maximum_ms,
        output_p95_callback_ms: output_timing.p95_ms,
        renderer_latency_frames,
        dsp_latency_frames,
        resampler_latency_frames: adaptive.resampler_latency_frames,
        estimated_software_buffering_frames: target_fill
            + renderer_latency_frames
            + dsp_latency_frames
            + adaptive.resampler_latency_frames,
        state: if faulted {
            "Faulted".to_owned()
        } else {
            format!("{:?}", state.state())
        },
        stream_fault: format!("{terminal_fault:?}"),
        adaptive_fault: format!("{:?}", adaptive.fault),
        engine_fault_code: output_timing.fault,
        memory_start_bytes: memory_start,
        memory_end_bytes: memory_end,
        memory_growth_bytes: memory_start
            .zip(memory_end)
            .map(|(start, end)| end as i128 - start as i128)
            .map(|growth| growth.clamp(i64::MIN as i128, i64::MAX as i128) as i64),
    };
    Ok((summary, faulted))
}

pub fn measure_physical_latency(options: LatencyOptions) -> Result<()> {
    if options.duration_seconds < 2 || options.channels == 0 {
        bail!("physical measurement requires at least two seconds and one channel");
    }
    let sample_capacity = options
        .duration_seconds
        .saturating_add(2)
        .saturating_mul(u64::from(options.sample_rate)) as usize;
    let capture = Arc::new(CaptureBuffer::new(sample_capacity));
    let capture_for_input = Arc::clone(&capture);
    let sequence = Arc::new(generate_measurement_sequence(127));
    let sequence_for_output = Arc::clone(&sequence);
    let capture_base = Arc::new(AtomicUsize::new(usize::MAX));
    let capture_base_for_output = Arc::clone(&capture_base);
    let output_cursor = Arc::new(AtomicUsize::new(0));
    let output_cursor_for_callback = Arc::clone(&output_cursor);
    let first_emission = options.sample_rate as usize / 2;
    let interval = options.sample_rate as usize * 2;
    let backend = CpalAudioBackend::new();
    let config = RealTimeAudioConfig {
        sample_rate: options.sample_rate,
        input_sample_rate: None,
        output_sample_rate: None,
        block_size: options.block_size,
        input_channels: options.channels,
        output_channels: options.channels,
        sample_format: RealTimeSampleFormat::F32,
        input_device_id: Some(options.input_device),
        output_device_id: Some(options.output_device),
    };
    let mut input_stream = backend.open_input(
        &config,
        Box::new(move |input, channels| capture_for_input.push_first_channel(input, channels)),
    )?;
    let capture_for_output = Arc::clone(&capture);
    let mut output_stream = backend.open_output(
        &config,
        Box::new(move |output, channels| {
            output.fill(0.0);
            let start = output_cursor_for_callback
                .fetch_add(output.len() / channels.max(1), Ordering::Relaxed);
            capture_base_for_output
                .compare_exchange(
                    usize::MAX,
                    capture_for_output.len(),
                    Ordering::Release,
                    Ordering::Relaxed,
                )
                .ok();
            for (frame_offset, frame) in output.chunks_exact_mut(channels).enumerate() {
                let absolute = start + frame_offset;
                if absolute >= first_emission {
                    let sequence_position = (absolute - first_emission) % interval;
                    if let Some(sample) = sequence_for_output.get(sequence_position) {
                        for channel in frame {
                            *channel = *sample;
                        }
                    }
                }
            }
        }),
    )?;
    input_stream.start()?;
    std::thread::sleep(Duration::from_millis(100));
    output_stream.start()?;
    std::thread::sleep(Duration::from_secs(options.duration_seconds));
    output_stream.stop()?;
    input_stream.stop()?;
    let captured = capture.to_vec();
    let base = capture_base.load(Ordering::Acquire);
    if base == usize::MAX || captured.is_empty() {
        bail!("no real input samples were captured; no measured latency is available");
    }
    if let Some(path) = options.save_capture {
        write_wav_f32(path, options.sample_rate, std::slice::from_ref(&captured))?;
    }
    let total_output = output_cursor.load(Ordering::Relaxed);
    let mut emissions = Vec::new();
    let mut emission = first_emission;
    while emission + sequence.len() < total_output {
        emissions.push(base.saturating_add(emission));
        emission = emission.saturating_add(interval);
    }
    let report = estimate_repeated_latency(
        &sequence,
        &captured,
        &emissions,
        options.sample_rate as usize,
        0.75,
        options.sample_rate,
    )?;
    println!("measurement_source=physical_captured_loopback");
    println!("measured_round_trip_samples={:.3}", report.median_samples);
    println!("measured_round_trip_ms={:.3}", report.median_milliseconds());
    println!("minimum_samples={}", report.minimum_samples);
    println!("maximum_samples={}", report.maximum_samples);
    println!("jitter_samples={:.3}", report.jitter_samples);
    println!("confidence={:.6}", report.confidence);
    println!("valid_measurements={}", report.valid_measurements);
    Ok(())
}

fn print_live_status(
    elapsed: Duration,
    snapshot: &aurora_realtime_engine::AdaptiveDuplexSnapshot,
    input: &CallbackMetrics,
    output: &CallbackMetrics,
    state: DuplexStreamState,
) {
    let input = input.snapshot();
    let output = output.snapshot();
    println!(
        "status elapsed_s={} state={state:?} input_callbacks={} output_callbacks={} captured_frames={} rendered_frames={} fill={}/{}/{} underflows={} overflows={} ratio={:.9} correction_ppm={:.3} saturation={} input_avg_ms={:.3} input_max_ms={:.3} input_p95_ms={:.3} output_avg_ms={:.3} output_max_ms={:.3} output_p95_ms={:.3} health={:?} adaptive_fault={:?} engine_fault_code={}",
        elapsed.as_secs(), input.callbacks, output.callbacks, input.frames, output.frames,
        snapshot.duplex.fill_frames, snapshot.duplex.min_fill_frames,
        snapshot.duplex.max_fill_frames, snapshot.duplex.underflow_count,
        snapshot.duplex.overflow_count, snapshot.current_ratio, snapshot.correction_ppm,
        snapshot.controller_saturation_count, input.average_ms, input.maximum_ms, input.p95_ms,
        output.average_ms, output.maximum_ms, output.p95_ms, snapshot.duplex.health, snapshot.fault,
        output.fault
    );
}

fn live_scene(channels: usize, block_size: usize) -> Result<RenderScene> {
    let layout = match channels {
        2 => StandardLayout::Stereo,
        6 => StandardLayout::FiveOne,
        8 => StandardLayout::SevenOne,
        _ => StandardLayout::Custom,
    };
    let roles = if layout == StandardLayout::Custom {
        (0..channels)
            .map(|index| ChannelRole::Custom(format!("duplex-{index}")))
            .collect::<Vec<_>>()
    } else {
        layout.canonical_roles().to_vec()
    };
    let speakers = roles
        .into_iter()
        .enumerate()
        .map(|(index, channel_role)| {
            let angle = index as f32 / channels as f32 * std::f32::consts::TAU;
            Speaker {
                id: format!("duplex-{index}"),
                label: format!("Duplex {index}"),
                channel_role,
                position: Vector3::new(angle.cos(), angle.sin(), 0.0),
                orientation: Vector3::ZERO,
                gain_db: 0.0,
                delay_samples: 0.0,
                enabled: true,
            }
        })
        .collect();
    Ok(RenderScene {
        layout,
        listener: Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 1.2,
        },
        speakers,
        object: SceneObject {
            id: "live-duplex".to_owned(),
            gain_db: 0.0,
            spread: 0.0,
        },
        trajectory: Trajectory::Circle {
            center: Vector3::ZERO,
            radius: 0.0,
            z: 0.0,
            start_degrees: 0.0,
            revolutions_per_second: 0.0,
        },
        block_size,
    })
}

fn merge_faults(input: AudioStreamFault, output: AudioStreamFault) -> AudioStreamFault {
    if input != AudioStreamFault::None {
        input
    } else {
        output
    }
}

struct CallbackMetrics {
    callbacks: AtomicU64,
    frames: AtomicU64,
    total_nanos: AtomicU64,
    maximum_nanos: AtomicU64,
    histogram: [AtomicU64; CALLBACK_HISTOGRAM_BUCKETS],
    fault: AtomicU64,
}

impl CallbackMetrics {
    fn new() -> Self {
        Self {
            callbacks: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            total_nanos: AtomicU64::new(0),
            maximum_nanos: AtomicU64::new(0),
            histogram: std::array::from_fn(|_| AtomicU64::new(0)),
            fault: AtomicU64::new(0),
        }
    }

    fn set_fault(&self, fault: u64) {
        self.fault.store(fault, Ordering::Release);
    }

    fn record(&self, frames: usize, duration: Duration) {
        let nanos = duration.as_nanos().min(u64::MAX as u128) as u64;
        self.callbacks.fetch_add(1, Ordering::Relaxed);
        self.frames.fetch_add(frames as u64, Ordering::Relaxed);
        self.total_nanos.fetch_add(nanos, Ordering::Relaxed);
        self.maximum_nanos.fetch_max(nanos, Ordering::Relaxed);
        let bucket = if nanos == 0 {
            0
        } else {
            (u64::BITS - nanos.leading_zeros()) as usize
        }
        .min(CALLBACK_HISTOGRAM_BUCKETS - 1);
        self.histogram[bucket].fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> CallbackTiming {
        let callbacks = self.callbacks.load(Ordering::Relaxed);
        let total = self.total_nanos.load(Ordering::Relaxed);
        let threshold = callbacks.saturating_mul(95).div_ceil(100);
        let mut cumulative = 0;
        let mut p95_nanos = 0;
        for (index, bucket) in self.histogram.iter().enumerate() {
            cumulative += bucket.load(Ordering::Relaxed);
            if cumulative >= threshold {
                p95_nanos = if index == 0 {
                    0
                } else {
                    1_u64 << index.min(63)
                };
                break;
            }
        }
        CallbackTiming {
            callbacks,
            frames: self.frames.load(Ordering::Relaxed),
            average_ms: if callbacks == 0 {
                0.0
            } else {
                total as f64 / callbacks as f64 / 1_000_000.0
            },
            maximum_ms: self.maximum_nanos.load(Ordering::Relaxed) as f64 / 1_000_000.0,
            p95_ms: p95_nanos as f64 / 1_000_000.0,
            fault: self.fault.load(Ordering::Acquire),
        }
    }
}

struct CallbackTiming {
    callbacks: u64,
    frames: u64,
    average_ms: f64,
    maximum_ms: f64,
    p95_ms: f64,
    fault: u64,
}

struct CaptureBuffer {
    samples: Box<[UnsafeCell<f32>]>,
    length: AtomicUsize,
}

// Safety: only the input callback writes and the control thread reads after both
// streams stop. The release/acquire length publication orders initialized data.
unsafe impl Sync for CaptureBuffer {}

impl CaptureBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            samples: (0..capacity)
                .map(|_| UnsafeCell::new(0.0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            length: AtomicUsize::new(0),
        }
    }

    fn push_first_channel(&self, input: &[f32], channels: usize) {
        if channels == 0 || input.len() % channels != 0 {
            return;
        }
        let start = self.length.load(Ordering::Relaxed);
        let accepted = (input.len() / channels).min(self.samples.len().saturating_sub(start));
        for (offset, frame) in input.chunks_exact(channels).take(accepted).enumerate() {
            unsafe { *self.samples[start + offset].get() = frame[0] };
        }
        self.length.store(start + accepted, Ordering::Release);
    }

    fn len(&self) -> usize {
        self.length.load(Ordering::Acquire)
    }

    fn to_vec(&self) -> Vec<f32> {
        (0..self.len())
            .map(|index| unsafe { *self.samples[index].get() })
            .collect()
    }
}

#[cfg(windows)]
fn current_working_set_bytes() -> Option<u64> {
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
    }
    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut core::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }
    let mut counters = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    let success = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<ProcessMemoryCounters>() as u32,
        )
    };
    (success != 0).then_some(counters.working_set_size as u64)
}

#[cfg(not(windows))]
fn current_working_set_bytes() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_buffer_is_bounded_and_preserves_first_channel() {
        let capture = CaptureBuffer::new(3);
        capture.push_first_channel(&[1.0, 10.0, 2.0, 20.0, 3.0, 30.0, 4.0, 40.0], 2);
        assert_eq!(capture.to_vec(), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    #[ignore = "requires AURORA_REALTIME_INPUT_DEVICE and AURORA_REALTIME_OUTPUT_DEVICE"]
    fn integration_live_duplex() {
        let Some(options) = hardware_options(5) else {
            return;
        };
        let summary = run_duplex(options).unwrap();
        assert_eq!(summary.overflows, 0);
        assert!(summary.output_callbacks > 0);
    }

    #[test]
    #[ignore = "requires a physical loopback cable and explicit device selectors"]
    fn integration_physical_loopback_latency() {
        let (Some(input_device), Some(output_device)) = (
            std::env::var("AURORA_REALTIME_INPUT_DEVICE").ok(),
            std::env::var("AURORA_REALTIME_OUTPUT_DEVICE").ok(),
        ) else {
            return;
        };
        measure_physical_latency(LatencyOptions {
            input_device,
            output_device,
            sample_rate: 48_000,
            block_size: 256,
            channels: 2,
            duration_seconds: 10,
            save_capture: None,
        })
        .unwrap();
    }

    #[test]
    #[ignore = "operator must disconnect one selected device during the test"]
    fn integration_device_disconnect() {
        let Some(options) = hardware_options(30) else {
            return;
        };
        assert!(run_duplex(options).is_err());
    }

    #[test]
    #[ignore = "requires selected hardware and runs for one hour"]
    fn integration_one_hour_soak() {
        let Some(options) = hardware_options(60 * 60) else {
            return;
        };
        let summary = run_duplex(options).unwrap();
        assert!(summary.actual_duration_seconds >= 3_599.0);
        assert_eq!(summary.overflows, 0);
    }

    fn hardware_options(duration_seconds: u64) -> Option<DuplexOptions> {
        Some(DuplexOptions {
            input_device: std::env::var("AURORA_REALTIME_INPUT_DEVICE").ok()?,
            output_device: std::env::var("AURORA_REALTIME_OUTPUT_DEVICE").ok()?,
            requested_rate: 48_000,
            input_rate: 48_000,
            output_rate: 48_000,
            block_size: 256,
            channels: 2,
            duration_seconds,
            restart_attempts: 0,
            print_status: false,
        })
    }
}
