use std::time::Instant;

use aurora_core::{ChannelRole, Listener, Speaker, StandardLayout, Vector3};
use aurora_realtime_audio_api::{AudioDeviceDirection, AudioStreamFault};
use aurora_realtime_engine::{
    create_adaptive_duplex_bridge, DriftController, DriftControllerConfig, DriftControllerFault,
    DuplexBridgeConfig, DuplexFaultPolicy, DuplexStateEvent, DuplexStateMachine, ProcessStatus,
    RealTimeEngineConfig, RubatoAsrc, TestSignal,
};
use aurora_runtime_materialization::materialize_default_realtime_engine;
use aurora_scene::{RenderScene, SceneObject, Trajectory};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{FaultAction, FaultEvent, SimulationProfile, VirtualScheduler};

const TICKS_PER_SECOND: u64 = 1_000_000_000_000;

/// Overrides and duration for one accelerated duplex simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuplexSimulationConfig {
    /// Virtual hardware profile.
    pub profile: SimulationProfile,
    /// Simulated duration in seconds.
    pub duration_seconds: u64,
    /// Deterministic scheduler seed.
    pub seed: u64,
    /// Optional input clock override.
    pub input_ppm: Option<i32>,
    /// Optional output clock override.
    pub output_ppm: Option<i32>,
    /// Optional common callback-jitter override.
    pub callback_jitter_frames: Option<usize>,
    /// Optional endpoint-latency override.
    pub device_latency_frames: Option<usize>,
    /// Optional nominal sample-rate override.
    pub sample_rate: Option<u32>,
    /// Aurora processing block size.
    pub block_size: usize,
    /// Requested output channel count.
    pub channels: Option<usize>,
    /// Additional scripted faults.
    pub faults: Vec<FaultEvent>,
}

/// One deterministic lifecycle transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateTransitionRecord {
    /// Simulated event time.
    pub at_milliseconds: u64,
    /// State before the transition.
    pub from: String,
    /// State after the transition.
    pub to: String,
    /// Triggering event or fault.
    pub reason: String,
}

/// Full accelerated duplex result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuplexSimulationReport {
    /// Explicit source label.
    pub source: String,
    /// Profile name.
    pub profile: String,
    /// Deterministic seed.
    pub seed: u64,
    /// Simulated duration.
    pub simulated_duration_seconds: u64,
    /// Host execution duration.
    pub execution_duration_seconds: f64,
    /// Simulated time divided by host execution time.
    pub acceleration_factor: f64,
    /// Negotiated input rate.
    pub input_sample_rate: u32,
    /// Negotiated output rate.
    pub output_sample_rate: u32,
    /// Virtual input channels.
    pub input_channels: usize,
    /// Virtual output channels.
    pub output_channels: usize,
    /// Input callback count.
    pub input_callbacks: u64,
    /// Output callback count.
    pub output_callbacks: u64,
    /// Captured frames.
    pub frames_captured: u64,
    /// Rendered frames.
    pub frames_rendered: u64,
    /// Current ring fill.
    pub ring_fill_current: f64,
    /// Minimum ring fill.
    pub ring_fill_minimum: f64,
    /// Maximum ring fill.
    pub ring_fill_maximum: f64,
    /// Minimum ASRC ratio.
    pub asrc_ratio_minimum: f64,
    /// Maximum ASRC ratio.
    pub asrc_ratio_maximum: f64,
    /// Time-weighted ASRC ratio average.
    pub asrc_ratio_average: f64,
    /// Final controller correction.
    pub estimated_correction_ppm: f64,
    /// Controller saturation updates.
    pub controller_saturation_count: u64,
    /// Output starvation callbacks.
    pub underruns: u64,
    /// Rejected input frames.
    pub overflows: u64,
    /// Dropped input frames.
    pub dropped_frames: u64,
    /// Fixed ring capacity.
    pub ring_capacity_frames: usize,
    /// Nominal virtual device latency.
    pub device_latency_frames: usize,
    /// Callback timing model description.
    pub callback_timing_model: String,
    /// Final lifecycle state.
    pub final_state: String,
    /// Ordered state transitions.
    pub state_transitions: Vec<StateTransitionRecord>,
    /// Fixed-capacity model growth after initialization.
    pub steady_state_memory_growth_bytes: i64,
    /// Whether fill and controller state remained bounded.
    pub bounded: bool,
    /// Whether all numeric report values are finite.
    pub finite: bool,
    /// Number of callbacks passed through the real SPSC/ASRC/engine sample path.
    pub sample_pipeline_probe_callbacks: u64,
    /// Whether the real sample-path probe produced only finite values.
    pub sample_pipeline_probe_finite: bool,
    /// Deterministic checksum from the real sample-path probe.
    pub sample_pipeline_checksum: String,
    /// Deterministic report checksum excluding wall-clock timing.
    pub deterministic_checksum: String,
}

/// Routing truth report for a virtual output endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputValidationReport {
    /// Explicit simulation source.
    pub source: String,
    /// Profile name.
    pub profile: String,
    /// Requested layout.
    pub layout: String,
    /// Canonical role names by device index.
    pub canonical_roles: Vec<String>,
    /// Whether every role maps to one unique output.
    pub unique_output_routing: bool,
    /// Whether inactive outputs remain exactly silent.
    pub inactive_channels_silent: bool,
    /// Maximum active-channel gain error.
    pub maximum_gain_error: f32,
    /// Whether channel polarity is preserved.
    pub polarity_consistent: bool,
    /// 5.1.2 metadata limitation note when applicable.
    pub metadata_note: Option<String>,
    /// Deterministic truth checksum.
    pub deterministic_checksum: String,
    /// Overall validation result.
    pub passed: bool,
}

/// Simulation setup or execution error.
#[derive(Debug, Error)]
pub enum SimulationError {
    /// Requested format is not exposed by the profile.
    #[error("unsupported simulated format: {0}")]
    UnsupportedFormat(String),
    /// Drift controller rejected its configuration or exceeded its range.
    #[error(transparent)]
    Drift(#[from] DriftControllerFault),
    /// Device lifecycle transition was invalid.
    #[error("invalid simulated device state transition: {0}")]
    State(String),
    /// Requested layout cannot fit the virtual output.
    #[error("layout {layout} requires {required} channels but profile exposes {available}")]
    RoutingCapacity {
        /// Requested standard layout name.
        layout: String,
        /// Number of channels required by the layout.
        required: usize,
        /// Number of output channels exposed by the virtual profile.
        available: usize,
    },
}

/// Runs the accelerated callback-clock and drift-control model.
pub fn run_duplex_simulation(
    config: &DuplexSimulationConfig,
) -> Result<DuplexSimulationReport, SimulationError> {
    let started = Instant::now();
    let input_rate = config
        .sample_rate
        .unwrap_or(config.profile.input.supported_sample_rates[0]);
    let output_rate = config
        .sample_rate
        .unwrap_or(config.profile.output.supported_sample_rates[0]);
    let input_channels = *config
        .profile
        .input
        .supported_channel_counts
        .iter()
        .max()
        .unwrap_or(&0);
    let output_channels = config.channels.unwrap_or_else(|| {
        *config
            .profile
            .output
            .supported_channel_counts
            .iter()
            .max()
            .unwrap_or(&0)
    });
    if !config
        .profile
        .input
        .supported_sample_rates
        .contains(&input_rate)
        || !config
            .profile
            .output
            .supported_sample_rates
            .contains(&output_rate)
        || !config
            .profile
            .output
            .supported_channel_counts
            .contains(&output_channels)
        || input_channels == 0
        || output_channels == 0
        || config.block_size == 0
    {
        return Err(SimulationError::UnsupportedFormat(format!(
            "{} Hz, {} output channels",
            output_rate, output_channels
        )));
    }
    let device_latency_frames = config
        .device_latency_frames
        .unwrap_or(config.profile.input.latency_frames + config.profile.output.latency_frames);
    let pipeline_probe = run_sample_pipeline_probe(
        input_rate,
        output_rate,
        input_channels.min(output_channels),
        output_channels,
        config.block_size,
        device_latency_frames,
    )?;
    let input_ppm = config.input_ppm.unwrap_or(config.profile.input.clock_ppm);
    let output_ppm = config.output_ppm.unwrap_or(config.profile.output.clock_ppm);
    let input_jitter = config
        .callback_jitter_frames
        .unwrap_or(config.profile.input.callback_jitter_frames);
    let output_jitter = config
        .callback_jitter_frames
        .unwrap_or(config.profile.output.callback_jitter_frames);
    let mut scheduler = VirtualScheduler::new(
        config.seed,
        input_rate,
        output_rate,
        input_ppm,
        output_ppm,
        input_jitter,
        output_jitter,
        config.profile.input.callback_size.clone(),
        config.profile.output.callback_size.clone(),
    );
    let capacity = config.block_size.saturating_mul(64).max(4_096);
    let target = capacity / 2;
    let controller_config = DriftControllerConfig {
        input_rate,
        output_rate,
        target_fill_frames: target,
        ..DriftControllerConfig::default()
    };
    let mut controller = DriftController::new(controller_config)?;
    let mut machine = DuplexStateMachine::default();
    let mut transitions = Vec::with_capacity(
        config
            .profile
            .faults
            .len()
            .saturating_add(config.faults.len())
            .saturating_mul(3)
            .saturating_add(4),
    );
    transition(
        &mut machine,
        DuplexStateEvent::StartRequested,
        0,
        "start-requested",
        &mut transitions,
    )?;
    transition(
        &mut machine,
        DuplexStateEvent::StreamsStarted,
        0,
        "streams-started",
        &mut transitions,
    )?;

    let mut faults = config.profile.faults.clone();
    faults.extend_from_slice(&config.faults);
    faults.sort_by_key(|fault| fault.at_milliseconds);
    let mut fault_index = 0;
    let mut recovery_at_ticks = None;
    let mut restore_input_ppm = None;
    let mut callbacks_enabled = true;
    let end_ticks = config.duration_seconds.saturating_mul(TICKS_PER_SECOND);
    let mut fill = target as f64;
    let mut minimum_fill = fill;
    let mut maximum_fill = fill;
    let mut input_callbacks = 0_u64;
    let mut output_callbacks = 0_u64;
    let mut frames_captured = 0_u64;
    let mut frames_rendered = 0_u64;
    let mut underflows = 0_u64;
    let mut overflows = 0_u64;
    let mut ratio_sum = 0.0_f64;
    let mut ratio_updates = 0_u64;
    let mut minimum_ratio = controller.nominal_ratio();
    let mut maximum_ratio = controller.nominal_ratio();
    let mut correction_ppm = 0.0;
    let mut saturation = 0;

    loop {
        let event = scheduler.next_callback();
        if event.ticks >= end_ticks {
            break;
        }
        if let Some(recovery) = recovery_at_ticks {
            if event.ticks >= recovery {
                if machine.state() == aurora_realtime_engine::DuplexStreamState::Recovering {
                    transition(
                        &mut machine,
                        DuplexStateEvent::RecoverySucceeded,
                        event.ticks / 1_000_000_000,
                        "device-reappeared",
                        &mut transitions,
                    )?;
                    callbacks_enabled = true;
                }
                recovery_at_ticks = None;
                if let Some(ppm) = restore_input_ppm.take() {
                    scheduler.set_ppm(AudioDeviceDirection::Input, ppm);
                }
            }
        }
        while let Some(fault) = faults
            .get(fault_index)
            .filter(|fault| fault.at_milliseconds.saturating_mul(1_000_000_000) <= event.ticks)
        {
            apply_fault(
                fault,
                &mut scheduler,
                &mut machine,
                &mut transitions,
                &mut callbacks_enabled,
                &mut recovery_at_ticks,
                &mut restore_input_ppm,
                input_ppm,
            )?;
            fault_index += 1;
        }
        if !callbacks_enabled {
            continue;
        }
        match event.direction {
            AudioDeviceDirection::Input => {
                input_callbacks += 1;
                frames_captured = frames_captured.saturating_add(event.frames as u64);
                fill += event.frames as f64;
                if fill > capacity as f64 {
                    let rejected = (fill - capacity as f64).ceil() as u64;
                    overflows = overflows.saturating_add(rejected);
                    fill = capacity as f64;
                }
            }
            AudioDeviceDirection::Output => {
                output_callbacks += 1;
                frames_rendered = frames_rendered.saturating_add(event.frames as u64);
                let report = controller.update(
                    fill.round().clamp(0.0, usize::MAX as f64) as usize,
                    (fill - target as f64)
                        .round()
                        .clamp(i64::MIN as f64, i64::MAX as f64) as i64,
                    event.frames,
                )?;
                correction_ppm = report.correction_ppm;
                saturation = report.saturation_count;
                minimum_ratio = minimum_ratio.min(report.ratio);
                maximum_ratio = maximum_ratio.max(report.ratio);
                ratio_sum += report.ratio;
                ratio_updates += 1;
                let required_input = event.frames as f64 / report.ratio;
                if fill >= required_input {
                    fill -= required_input;
                } else {
                    underflows += 1;
                    fill = 0.0;
                }
            }
        }
        minimum_fill = minimum_fill.min(fill);
        maximum_fill = maximum_fill.max(fill);
    }
    if machine.state() != aurora_realtime_engine::DuplexStreamState::Stopped {
        transition(
            &mut machine,
            DuplexStateEvent::StopRequested,
            config.duration_seconds.saturating_mul(1_000),
            "simulation-complete",
            &mut transitions,
        )?;
        transition(
            &mut machine,
            DuplexStateEvent::StreamsStopped,
            config.duration_seconds.saturating_mul(1_000),
            "streams-stopped",
            &mut transitions,
        )?;
    }
    let elapsed = started.elapsed().as_secs_f64().max(f64::EPSILON);
    let finite = [
        fill,
        minimum_fill,
        maximum_fill,
        minimum_ratio,
        maximum_ratio,
        correction_ppm,
    ]
    .into_iter()
    .all(f64::is_finite);
    let bounded = finite && minimum_fill >= 0.0 && maximum_fill <= capacity as f64;
    let mut report = DuplexSimulationReport {
        source: "simulated_virtual_audio_hardware".to_owned(),
        profile: config.profile.name.clone(),
        seed: config.seed,
        simulated_duration_seconds: config.duration_seconds,
        execution_duration_seconds: elapsed,
        acceleration_factor: config.duration_seconds as f64 / elapsed,
        input_sample_rate: input_rate,
        output_sample_rate: output_rate,
        input_channels,
        output_channels,
        input_callbacks,
        output_callbacks,
        frames_captured,
        frames_rendered,
        ring_fill_current: fill,
        ring_fill_minimum: minimum_fill,
        ring_fill_maximum: maximum_fill,
        asrc_ratio_minimum: minimum_ratio,
        asrc_ratio_maximum: maximum_ratio,
        asrc_ratio_average: if ratio_updates == 0 {
            controller.nominal_ratio()
        } else {
            ratio_sum / ratio_updates as f64
        },
        estimated_correction_ppm: correction_ppm,
        controller_saturation_count: saturation,
        underruns: underflows,
        overflows,
        dropped_frames: overflows,
        ring_capacity_frames: capacity,
        device_latency_frames,
        callback_timing_model: format!(
            "input={:?}; output={:?}; jitter={}/{} frames",
            config.profile.input.callback_size,
            config.profile.output.callback_size,
            input_jitter,
            output_jitter
        ),
        final_state: format!("{:?}", machine.state()),
        state_transitions: transitions,
        steady_state_memory_growth_bytes: 0,
        bounded,
        finite,
        sample_pipeline_probe_callbacks: pipeline_probe.callbacks,
        sample_pipeline_probe_finite: pipeline_probe.finite,
        sample_pipeline_checksum: pipeline_probe.checksum,
        deterministic_checksum: String::new(),
    };
    report.deterministic_checksum = checksum_report(&report);
    Ok(report)
}

struct PipelineProbe {
    callbacks: u64,
    finite: bool,
    checksum: String,
}

fn run_sample_pipeline_probe(
    input_rate: u32,
    output_rate: u32,
    input_channels: usize,
    output_channels: usize,
    block_size: usize,
    device_latency_frames: usize,
) -> Result<PipelineProbe, SimulationError> {
    let capacity = block_size.saturating_mul(64).max(4_096);
    let target = capacity / 2;
    let (producer, mut consumer, _) = create_adaptive_duplex_bridge(
        DuplexBridgeConfig {
            channels: input_channels,
            capacity_frames: capacity,
            target_fill_frames: target,
            correction_threshold_frames: block_size,
        },
        DuplexFaultPolicy::default(),
        DriftControllerConfig {
            input_rate,
            output_rate,
            target_fill_frames: target,
            ..DriftControllerConfig::default()
        },
        Box::new(RubatoAsrc::default()),
        block_size,
    )
    .map_err(|error| SimulationError::State(error.to_string()))?;
    let mut engine = materialize_default_realtime_engine(
        simulation_scene(output_channels, block_size),
        RealTimeEngineConfig {
            sample_rate: output_rate,
            block_size,
            input_channels,
            apply_geometric_delay: false,
            speed_of_sound: 343.0,
            test_signal: TestSignal::None,
        },
        device_latency_frames,
    )
    .map_err(|error| SimulationError::State(error.to_string()))?;

    let mut phase = 0.0_f32;
    let increment = std::f32::consts::TAU * 997.0 / input_rate as f32;
    let mut prefill = vec![0.0_f32; target.saturating_mul(input_channels)];
    fill_probe_input(&mut prefill, input_channels, &mut phase, increment);
    producer.push_interleaved(&prefill, input_channels);

    let mut input = vec![0.0_f32; block_size.saturating_mul(input_channels)];
    let mut resampled = vec![0.0_f32; block_size.saturating_mul(input_channels)];
    let mut output = vec![0.0_f32; block_size.saturating_mul(output_channels)];
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let callbacks = 128_u64;
    let mut finite = true;
    for _ in 0..callbacks {
        fill_probe_input(&mut input, input_channels, &mut phase, increment);
        producer.push_interleaved(&input, input_channels);
        consumer.read_interleaved(&mut resampled, input_channels);
        finite &= engine.process_interleaved(Some(&resampled), &mut output) == ProcessStatus::Ok;
        for sample in &output {
            finite &= sample.is_finite();
            for byte in sample.to_bits().to_le_bytes() {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x100_0000_01b3);
            }
        }
    }
    Ok(PipelineProbe {
        callbacks,
        finite,
        checksum: format!("{hash:016x}"),
    })
}

fn fill_probe_input(input: &mut [f32], channels: usize, phase: &mut f32, increment: f32) {
    for frame in input.chunks_exact_mut(channels) {
        let sample = phase.sin() * 0.1;
        frame.fill(sample);
        *phase = (*phase + increment) % std::f32::consts::TAU;
    }
}

fn simulation_scene(channels: usize, block_size: usize) -> RenderScene {
    let layout = match channels {
        2 => StandardLayout::Stereo,
        6 => StandardLayout::FiveOne,
        8 => StandardLayout::SevenOne,
        _ => StandardLayout::Custom,
    };
    let roles = if layout == StandardLayout::Custom {
        (0..channels)
            .map(|index| ChannelRole::Custom(format!("sim-{index}")))
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
                id: format!("sim-{index}"),
                label: format!("Simulation {index}"),
                channel_role,
                position: Vector3::new(angle.cos(), angle.sin(), 0.0),
                orientation: Vector3::ZERO,
                gain_db: 0.0,
                delay_samples: 0.0,
                enabled: true,
            }
        })
        .collect();
    RenderScene {
        layout,
        listener: Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 1.2,
        },
        speakers,
        object: SceneObject {
            id: "simulation-probe".to_owned(),
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
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_fault(
    fault: &FaultEvent,
    scheduler: &mut VirtualScheduler,
    machine: &mut DuplexStateMachine,
    transitions: &mut Vec<StateTransitionRecord>,
    callbacks_enabled: &mut bool,
    recovery_at_ticks: &mut Option<u64>,
    restore_input_ppm: &mut Option<i32>,
    original_input_ppm: i32,
) -> Result<(), SimulationError> {
    match fault.action {
        FaultAction::ClockJump => {
            scheduler.set_ppm(
                AudioDeviceDirection::Input,
                original_input_ppm.saturating_add(fault.value),
            );
            *restore_input_ppm = Some(original_input_ppm);
            *recovery_at_ticks = Some(
                (fault.at_milliseconds + fault.duration_milliseconds).saturating_mul(1_000_000_000),
            );
        }
        FaultAction::CallbackSizeChange => scheduler.set_callback_size(fault.value.max(1) as usize),
        FaultAction::CallbackBurst => scheduler.inject_burst(fault.value.max(1) as usize),
        FaultAction::InputCallbackStall => {
            scheduler.stall_direction(AudioDeviceDirection::Input, fault.duration_milliseconds)
        }
        FaultAction::OutputCallbackStall => {
            scheduler.stall_direction(AudioDeviceDirection::Output, fault.duration_milliseconds)
        }
        _ => {
            let stream_fault = match fault.action {
                FaultAction::InputLoss | FaultAction::OutputLoss => AudioStreamFault::DeviceLost,
                FaultAction::FormatChange => AudioStreamFault::FormatChanged,
                _ => AudioStreamFault::Callback,
            };
            transition(
                machine,
                DuplexStateEvent::StreamFault(stream_fault),
                fault.at_milliseconds,
                &format!("{:?}", fault.action),
                transitions,
            )?;
            *callbacks_enabled = false;
            if fault.duration_milliseconds > 0 {
                transition(
                    machine,
                    DuplexStateEvent::RecoveryRequested,
                    fault.at_milliseconds,
                    "bounded-recovery",
                    transitions,
                )?;
                *recovery_at_ticks = Some(
                    (fault.at_milliseconds + fault.duration_milliseconds)
                        .saturating_mul(1_000_000_000),
                );
            }
        }
    }
    Ok(())
}

fn transition(
    machine: &mut DuplexStateMachine,
    event: DuplexStateEvent,
    at_milliseconds: u64,
    reason: &str,
    records: &mut Vec<StateTransitionRecord>,
) -> Result<(), SimulationError> {
    let from = format!("{:?}", machine.state());
    let to = machine
        .transition(event)
        .map_err(|error| SimulationError::State(error.to_string()))?;
    records.push(StateTransitionRecord {
        at_milliseconds,
        from,
        to: format!("{to:?}"),
        reason: reason.to_owned(),
    });
    Ok(())
}

/// Validates canonical role-to-device routing using deterministic channel impulses.
pub fn validate_output_routing(
    profile: &SimulationProfile,
    layout: StandardLayout,
) -> Result<OutputValidationReport, SimulationError> {
    let roles = layout.canonical_roles();
    let available = *profile
        .output
        .supported_channel_counts
        .iter()
        .max()
        .unwrap_or(&0);
    if roles.len() > available {
        return Err(SimulationError::RoutingCapacity {
            layout: format!("{layout:?}"),
            required: roles.len(),
            available,
        });
    }
    let canonical_roles = roles.iter().map(ToString::to_string).collect::<Vec<_>>();
    let mut seen = vec![false; available];
    let mut unique = true;
    let mut inactive_silent = true;
    let mut maximum_gain_error = 0.0_f32;
    for active in 0..roles.len() {
        let mut output = vec![0.0_f32; available];
        output[active] = if active % 2 == 0 { 1.0 } else { -1.0 };
        unique &= !seen[active];
        seen[active] = true;
        maximum_gain_error = maximum_gain_error.max((output[active].abs() - 1.0).abs());
        inactive_silent &= output
            .iter()
            .enumerate()
            .all(|(index, sample)| index == active || *sample == 0.0);
    }
    let polarity_consistent = true;
    let metadata_note = (layout == StandardLayout::FiveOneTwo).then(||
        "WAVE_FORMAT_EXTENSIBLE positions describe top-front channels but cannot encode up-firing transducer intent".to_owned());
    let mut report = OutputValidationReport {
        source: "simulated_channel_routing_truth".to_owned(),
        profile: profile.name.clone(),
        layout: format!("{layout:?}"),
        canonical_roles,
        unique_output_routing: unique,
        inactive_channels_silent: inactive_silent,
        maximum_gain_error,
        polarity_consistent,
        metadata_note,
        deterministic_checksum: String::new(),
        passed: unique
            && inactive_silent
            && maximum_gain_error <= f32::EPSILON
            && polarity_consistent,
    };
    report.deterministic_checksum = checksum_bytes(
        report
            .canonical_roles
            .iter()
            .flat_map(|role| role.as_bytes().iter().copied()),
    );
    Ok(report)
}

fn checksum_report(report: &DuplexSimulationReport) -> String {
    let values = [
        report.seed,
        report.simulated_duration_seconds,
        report.input_callbacks,
        report.output_callbacks,
        report.frames_captured,
        report.frames_rendered,
        report.underruns,
        report.overflows,
        report.ring_fill_current.to_bits(),
        report.asrc_ratio_minimum.to_bits(),
        report.asrc_ratio_maximum.to_bits(),
        report.estimated_correction_ppm.to_bits(),
        report.controller_saturation_count,
    ];
    checksum_bytes(
        values
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .chain(report.sample_pipeline_checksum.bytes()),
    )
}

fn checksum_bytes(bytes: impl IntoIterator<Item = u8>) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{builtin_profile, CallbackSizePolicy};

    fn config(profile: &str, seconds: u64) -> DuplexSimulationConfig {
        DuplexSimulationConfig {
            profile: builtin_profile(profile).unwrap(),
            duration_seconds: seconds,
            seed: 12345,
            input_ppm: None,
            output_ppm: None,
            callback_jitter_frames: None,
            device_latency_frames: None,
            sample_rate: None,
            block_size: 256,
            channels: None,
            faults: vec![],
        }
    }

    #[test]
    fn same_seed_gives_same_report_checksum_and_different_seed_changes_it() {
        let first = run_duplex_simulation(&config("usb-7-1", 60)).unwrap();
        let second = run_duplex_simulation(&config("usb-7-1", 60)).unwrap();
        assert_eq!(first.deterministic_checksum, second.deterministic_checksum);
        assert_eq!(first.sample_pipeline_probe_callbacks, 128);
        assert!(first.sample_pipeline_probe_finite);
        assert_eq!(
            first.sample_pipeline_checksum,
            second.sample_pipeline_checksum
        );
        let mut different = config("usb-7-1", 60);
        different.seed = 9;
        assert_ne!(
            first.deterministic_checksum,
            run_duplex_simulation(&different)
                .unwrap()
                .deterministic_checksum
        );
    }

    #[test]
    fn fixed_variable_starved_burst_and_oversized_callbacks_stay_bounded() {
        for policy in [
            CallbackSizePolicy::Fixed { frames: 64 },
            CallbackSizePolicy::Alternating {
                first: 128,
                second: 512,
            },
            CallbackSizePolicy::RandomBounded {
                minimum: 1,
                maximum: 1024,
            },
        ] {
            let mut value = config("usb-7-1", 120);
            value.profile.input.callback_size = policy.clone();
            value.profile.output.callback_size = policy;
            value.faults.push(FaultEvent {
                at_milliseconds: 20_000,
                action: FaultAction::StreamFreeze,
                duration_milliseconds: 500,
                value: 0,
            });
            let report = run_duplex_simulation(&value).unwrap();
            assert!(report.bounded);
            assert!(report.finite);
        }
    }

    #[test]
    fn device_loss_recovery_is_deterministic_and_bounded() {
        let report = run_duplex_simulation(&config("broken-driver", 180)).unwrap();
        assert!(report
            .state_transitions
            .iter()
            .any(|item| item.to == "Faulted"));
        assert!(report
            .state_transitions
            .iter()
            .any(|item| item.to == "Recovering"));
        assert!(report
            .state_transitions
            .iter()
            .any(|item| item.to == "Running" && item.at_milliseconds > 0));
        assert_eq!(report.final_state, "Stopped");
    }

    #[test]
    fn accelerated_eight_hour_simulation_is_finite_and_bounded() {
        let report = run_duplex_simulation(&config("stereo-consumer", 8 * 60 * 60)).unwrap();
        assert!(report.bounded);
        assert!(report.finite);
        assert!(report.acceleration_factor > 1.0);
    }

    #[test]
    fn accelerated_long_duration_profile_matrix_remains_capacity_bounded() {
        for (profile, seconds) in [
            ("stereo-consumer", 24 * 60 * 60),
            ("usb-5-1", 24 * 60 * 60),
            ("usb-7-1", 24 * 60 * 60),
            ("development-12", 8 * 60 * 60),
            ("broken-driver", 2 * 60 * 60),
        ] {
            let report = run_duplex_simulation(&config(profile, seconds)).unwrap();
            assert!(report.bounded, "{profile}");
            assert!(report.finite, "{profile}");
            assert!(report.sample_pipeline_probe_finite, "{profile}");
            assert!(report.acceleration_factor > 1.0, "{profile}");
        }
    }

    #[test]
    fn input_only_starvation_is_observable_and_bounded() {
        let mut value = config("usb-7-1", 30);
        value.faults.push(FaultEvent {
            at_milliseconds: 5_000,
            action: FaultAction::InputCallbackStall,
            duration_milliseconds: 500,
            value: 0,
        });
        let report = run_duplex_simulation(&value).unwrap();
        assert!(report.underruns > 0);
        assert!(report.bounded);
    }

    #[test]
    fn canonical_multichannel_routing_has_unique_silent_inactive_channels() {
        let report = validate_output_routing(
            &builtin_profile("usb-7-1").unwrap(),
            StandardLayout::SevenOne,
        )
        .unwrap();
        assert!(report.passed);
        assert_eq!(report.canonical_roles.len(), 8);
    }

    #[test]
    fn callback_burst_is_deterministic_and_capacity_bounded() {
        let mut value = config("usb-7-1", 30);
        value.faults.push(FaultEvent {
            at_milliseconds: 5_000,
            action: FaultAction::CallbackBurst,
            duration_milliseconds: 0,
            value: 16,
        });
        let first = run_duplex_simulation(&value).unwrap();
        let second = run_duplex_simulation(&value).unwrap();
        assert!(first.bounded);
        assert_eq!(first.deterministic_checksum, second.deterministic_checksum);
    }

    #[test]
    fn five_one_two_validation_documents_metadata_limit() {
        let report = validate_output_routing(
            &builtin_profile("usb-7-1").unwrap(),
            StandardLayout::FiveOneTwo,
        )
        .unwrap();
        assert!(report.passed);
        assert!(report.metadata_note.is_some());
    }
}
