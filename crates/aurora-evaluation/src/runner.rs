use std::mem::size_of;
use std::time::Instant;

use aurora_core::Listener;
use aurora_dsp_basic::DelayProcessor;
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_scene::Trajectory;
use serde::Serialize;

use crate::{
    AllocationObservation, AudioMetrics, DiscontinuityMetrics, EvaluationBundle, EvaluationConfig,
    EvaluationError, EvaluationReport, EvidenceStatus, GainTrajectoryPoint, LatencyEvidence,
    MemoryMetrics, PerformanceMetrics, ProbeResult, Provenance, RendererMetadata,
    ValidationFinding, ValidationSummary, EVALUATION_SCHEMA_VERSION, MAX_BLOCKS, MAX_CHANNELS,
    MAX_FRAMES, MAX_HOOKS, MAX_PROBES, MAX_STRING_BYTES,
};

#[derive(Serialize)]
struct HashConfiguration<'a> {
    renderer_id: &'a str,
    scenario_id: &'a str,
    sample_rate: u32,
    block_size: usize,
    max_delay_samples: f32,
    apply_delays: bool,
    thresholds: crate::EvaluationThresholds,
    probes: &'a [crate::ProbeDefinition],
}

/// Runs one configured renderer against deterministic probes and a trajectory.
///
/// All block-loop storage is allocated before timing starts. Host timing uses
/// [`Instant`] and is explicitly reported as `host_api_observation`. Host timing
/// is advisory and excluded from the required deterministic aggregate. The
/// supplied renderer remains responsible for its accepted allocation-free
/// steady-state contract.
pub fn evaluate_renderer<R: Renderer + ?Sized>(
    renderer: &mut R,
    listener: &Listener,
    trajectory: &Trajectory,
    input_mono: &[f32],
    config: &EvaluationConfig,
) -> Result<EvaluationBundle, EvaluationError> {
    validate_config(config, input_mono.len())?;
    let channel_count = renderer.output_channel_count();
    if channel_count == 0 || channel_count > MAX_CHANNELS {
        return Err(EvaluationError::LimitExceeded {
            field: "output_channels",
            actual: channel_count,
            maximum: MAX_CHANNELS,
        });
    }

    let block_count = input_mono.len().div_ceil(config.block_size);
    if block_count > MAX_BLOCKS {
        return Err(EvaluationError::LimitExceeded {
            field: "blocks",
            actual: block_count,
            maximum: MAX_BLOCKS,
        });
    }

    let trajectory_capacity = block_count
        .checked_mul(channel_count)
        .ok_or(EvaluationError::CapacityOverflow)?;
    let mut scratch = RendererScratch::new(renderer.required_scratch_size()?);
    let mut gains = vec![SpeakerGain::default(); channel_count];
    let mut delays = vec![0.0_f32; channel_count];
    let mut block_input = vec![vec![0.0_f32; config.block_size]; channel_count];
    let mut block_output = vec![vec![0.0_f32; config.block_size]; channel_count];
    let mut audio_channels = vec![vec![0.0_f32; input_mono.len()]; channel_count];
    let mut trajectory_points = Vec::with_capacity(trajectory_capacity);
    let mut timing_samples = Vec::with_capacity(block_count);
    let mut probe_results = Vec::with_capacity(config.probes.len());
    let mut delay_processor = DelayProcessor::new(channel_count, config.max_delay_samples);

    for probe in &config.probes {
        renderer.reset();
        renderer.render_gains(
            listener,
            &[RenderObject {
                position: probe.position,
                gain: 1.0,
            }],
            &mut gains,
            &mut scratch,
        )?;
        probe_results.push(ProbeResult {
            id: probe.id.clone(),
            position: probe.position,
            gains: gains.iter().map(|value| value.gain).collect(),
            delays_samples: gains.iter().map(|value| value.delay_samples).collect(),
        });
    }

    renderer.reset();
    let mut maximum_gain_power_error = 0.0_f32;
    let mut maximum_gain_delta = 0.0_f32;
    let mut maximum_delay_delta = 0.0_f32;
    let mut previous_gains = vec![0.0_f32; channel_count];
    let mut previous_delays = vec![0.0_f32; channel_count];
    let mut has_previous_block = false;

    for (block_index, block_start) in (0..input_mono.len()).step_by(config.block_size).enumerate() {
        let block_end = (block_start + config.block_size).min(input_mono.len());
        let frame_count = block_end - block_start;
        let midpoint = block_start + frame_count / 2;
        let position = trajectory.position_at_time(midpoint as f64 / f64::from(config.sample_rate));
        let object = RenderObject {
            position,
            gain: 1.0,
        };

        let started = Instant::now();
        renderer.render_gains(
            listener,
            std::slice::from_ref(&object),
            &mut gains,
            &mut scratch,
        )?;
        timing_samples.push(duration_ns_u64(started.elapsed().as_nanos()));

        let gain_power = gains
            .iter()
            .map(|value| value.gain * value.gain)
            .sum::<f32>();
        maximum_gain_power_error = maximum_gain_power_error.max((gain_power - 1.0).abs());

        for (channel_index, gain) in gains.iter().enumerate() {
            if has_previous_block {
                maximum_gain_delta =
                    maximum_gain_delta.max((gain.gain - previous_gains[channel_index]).abs());
                maximum_delay_delta = maximum_delay_delta
                    .max((gain.delay_samples - previous_delays[channel_index]).abs());
            }
            previous_gains[channel_index] = gain.gain;
            previous_delays[channel_index] = gain.delay_samples;
            delays[channel_index] = if config.apply_delays {
                gain.delay_samples
            } else {
                0.0
            };
            trajectory_points.push(GainTrajectoryPoint {
                block_index,
                frame_index: midpoint,
                channel_index,
                gain: gain.gain,
                delay_samples: gain.delay_samples,
            });

            block_input[channel_index][..frame_count]
                .iter_mut()
                .zip(&input_mono[block_start..block_end])
                .for_each(|(target, input)| *target = *input * gain.gain);
            block_input[channel_index][frame_count..].fill(0.0);
            block_output[channel_index].fill(0.0);
        }
        has_previous_block = true;

        delay_processor.set_delays_slice(&delays)?;
        delay_processor.process_block_into(&block_input, &mut block_output, frame_count)?;
        for channel_index in 0..channel_count {
            audio_channels[channel_index][block_start..block_end]
                .copy_from_slice(&block_output[channel_index][..frame_count]);
        }
    }

    timing_samples.sort_unstable();
    let performance = performance_metrics(&timing_samples, config.thresholds.max_renderer_p99_ns);
    let audio = audio_metrics(&audio_channels);
    let maximum_audio_sample_delta = audio_channels
        .iter()
        .flat_map(|channel| channel.windows(2))
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0_f32, f32::max);
    let discontinuity = DiscontinuityMetrics {
        maximum_gain_delta,
        maximum_delay_delta_samples: maximum_delay_delta,
        maximum_audio_sample_delta,
    };

    let memory = memory_metrics(
        input_mono.len(),
        channel_count,
        trajectory_points.capacity(),
        timing_samples.capacity(),
        config.block_size,
    )?;
    let allocation_status = match config.steady_state_allocations {
        Some(0) => EvidenceStatus::Pass,
        Some(_) => EvidenceStatus::Fail,
        None => EvidenceStatus::NotObserved,
    };
    let allocations = AllocationObservation {
        status: allocation_status,
        steady_state_allocations: config.steady_state_allocations,
        truth_source: config
            .steady_state_allocations
            .map(|_| "unit_test".to_owned()),
    };
    let maximum_delay = trajectory_points
        .iter()
        .map(|point| point.delay_samples)
        .fold(0.0_f32, f32::max);
    let latency = LatencyEvidence {
        renderer_latency_frames: renderer.latency_frames(),
        configured_delay_latency_frames: maximum_delay.ceil() as usize,
        classification: "reported_and_configured_not_physical_measurement".to_owned(),
    };
    let validation = validation_summary(
        maximum_gain_power_error,
        &audio,
        &discontinuity,
        &allocations,
        &config.hooks,
        config,
    );

    let hash_input = serde_json::to_vec(&HashConfiguration {
        renderer_id: &config.renderer_id,
        scenario_id: &config.scenario_id,
        sample_rate: config.sample_rate,
        block_size: config.block_size,
        max_delay_samples: config.max_delay_samples,
        apply_delays: config.apply_delays,
        thresholds: config.thresholds,
        probes: &config.probes,
    })?;
    let report = EvaluationReport {
        schema_version: EVALUATION_SCHEMA_VERSION,
        renderer: RendererMetadata {
            renderer_id: config.renderer_id.clone(),
            output_channels: channel_count,
            configuration_hash_fnv1a64: fnv1a64_hex(&hash_input),
        },
        provenance: Provenance {
            scenario_id: config.scenario_id.clone(),
            commit_sha: config.commit_sha.clone(),
            command: config.command.clone(),
            correctness_truth_source: "unit_test".to_owned(),
            performance_truth_source: "host_api_observation".to_owned(),
        },
        audio,
        latency,
        performance,
        memory,
        allocations,
        discontinuity,
        probes: probe_results,
        trajectory: trajectory_points,
        hooks: config.hooks.clone(),
        validation,
    };

    Ok(EvaluationBundle {
        report,
        audio_channels,
    })
}

fn validate_config(config: &EvaluationConfig, frames: usize) -> Result<(), EvaluationError> {
    if config.sample_rate == 0 || config.block_size == 0 {
        return Err(EvaluationError::InvalidConfiguration(
            "sample rate and block size must be greater than zero",
        ));
    }
    if !config.max_delay_samples.is_finite() || config.max_delay_samples < 0.0 {
        return Err(EvaluationError::InvalidConfiguration(
            "maximum delay must be finite and non-negative",
        ));
    }
    if frames == 0 || frames > MAX_FRAMES {
        return Err(EvaluationError::LimitExceeded {
            field: "frames",
            actual: frames,
            maximum: MAX_FRAMES,
        });
    }
    if config.probes.len() > MAX_PROBES {
        return Err(EvaluationError::LimitExceeded {
            field: "probes",
            actual: config.probes.len(),
            maximum: MAX_PROBES,
        });
    }
    if config.hooks.len() > MAX_HOOKS {
        return Err(EvaluationError::LimitExceeded {
            field: "hooks",
            actual: config.hooks.len(),
            maximum: MAX_HOOKS,
        });
    }
    for value in [
        config.renderer_id.as_str(),
        config.scenario_id.as_str(),
        config.commit_sha.as_str(),
        config.command.as_str(),
    ] {
        if value.is_empty() || value.len() > MAX_STRING_BYTES {
            return Err(EvaluationError::InvalidConfiguration(
                "renderer, commit, and command strings must be non-empty and bounded",
            ));
        }
    }
    for probe in &config.probes {
        if probe.id.is_empty() || probe.id.len() > MAX_STRING_BYTES {
            return Err(EvaluationError::InvalidConfiguration(
                "probe identifiers must be non-empty and bounded",
            ));
        }
        if !probe.position.x.is_finite()
            || !probe.position.y.is_finite()
            || !probe.position.z.is_finite()
        {
            return Err(EvaluationError::InvalidConfiguration(
                "probe positions must contain only finite coordinates",
            ));
        }
    }
    for hook in &config.hooks {
        if hook.id.is_empty()
            || hook.id.len() > MAX_STRING_BYTES - "hook:".len()
            || hook.truth_source.is_empty()
            || hook.truth_source.len() > MAX_STRING_BYTES
        {
            return Err(EvaluationError::InvalidConfiguration(
                "hook identifiers and truth sources must be non-empty and bounded",
            ));
        }
    }
    let thresholds = config.thresholds;
    if !thresholds.max_gain_power_error.is_finite()
        || thresholds.max_gain_power_error < 0.0
        || !thresholds.max_gain_discontinuity.is_finite()
        || thresholds.max_gain_discontinuity < 0.0
        || !thresholds.max_delay_discontinuity_samples.is_finite()
        || thresholds.max_delay_discontinuity_samples < 0.0
        || !thresholds.max_audio_discontinuity.is_finite()
        || thresholds.max_audio_discontinuity < 0.0
        || thresholds.max_renderer_p99_ns == 0
    {
        return Err(EvaluationError::InvalidConfiguration(
            "evaluation thresholds must be finite, non-negative, and have a non-zero p99 limit",
        ));
    }
    Ok(())
}

fn performance_metrics(samples: &[u64], advisory_limit_ns: u64) -> PerformanceMetrics {
    let renderer_p99_ns = percentile(samples, 99);
    PerformanceMetrics {
        samples: samples.len(),
        renderer_p50_ns: percentile(samples, 50),
        renderer_p95_ns: percentile(samples, 95),
        renderer_p99_ns,
        renderer_max_ns: samples.last().copied().unwrap_or(0),
        advisory_p99_limit_ns: advisory_limit_ns,
        advisory_threshold_met: renderer_p99_ns <= advisory_limit_ns,
        truth_source: "host_api_observation".to_owned(),
    }
}

fn percentile(samples: &[u64], percentile: usize) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let rank = samples.len().saturating_mul(percentile).saturating_add(99) / 100;
    samples[rank.saturating_sub(1).min(samples.len() - 1)]
}

fn audio_metrics(channels: &[Vec<f32>]) -> AudioMetrics {
    let peak_per_channel = channels
        .iter()
        .map(|channel| {
            channel
                .iter()
                .map(|sample| sample.abs())
                .fold(0.0, f32::max)
        })
        .collect::<Vec<_>>();
    let mut hash = FNV1A64_OFFSET;
    for byte in channels
        .iter()
        .flatten()
        .flat_map(|sample| sample.to_le_bytes())
    {
        hash = fnv1a64_update(hash, byte);
    }
    AudioMetrics {
        frames: channels.first().map_or(0, Vec::len),
        channels: channels.len(),
        peak_per_channel,
        contains_non_finite: channels.iter().flatten().any(|sample| !sample.is_finite()),
        clipping_detected: channels.iter().flatten().any(|sample| sample.abs() > 1.0),
        audio_checksum_fnv1a64: format!("{hash:016x}"),
    }
}

fn memory_metrics(
    frames: usize,
    channels: usize,
    trajectory_capacity: usize,
    timing_capacity: usize,
    block_size: usize,
) -> Result<MemoryMetrics, EvaluationError> {
    let output_audio_bytes = checked_bytes(frames, channels, size_of::<f32>())?;
    let trajectory_bytes = checked_bytes(trajectory_capacity, 1, size_of::<GainTrajectoryPoint>())?;
    let timing_bytes = checked_bytes(timing_capacity, 1, size_of::<u64>())?;
    let processing_bytes = checked_bytes(block_size, channels.saturating_mul(2), size_of::<f32>())?;
    let bounded = output_audio_bytes
        .checked_add(trajectory_bytes)
        .and_then(|value| value.checked_add(timing_bytes))
        .and_then(|value| value.checked_add(processing_bytes))
        .ok_or(EvaluationError::CapacityOverflow)?;
    Ok(MemoryMetrics {
        bounded_working_set_bytes: bounded,
        output_audio_bytes,
        trajectory_bytes,
        timing_bytes,
        peak_process_memory_bytes: None,
        coverage: "partial_primary_payloads".to_owned(),
        truth_source: "deterministic_capacity_accounting".to_owned(),
    })
}

fn checked_bytes(a: usize, b: usize, width: usize) -> Result<u64, EvaluationError> {
    let bytes = a
        .checked_mul(b)
        .and_then(|value| value.checked_mul(width))
        .ok_or(EvaluationError::CapacityOverflow)?;
    u64::try_from(bytes).map_err(|_| EvaluationError::CapacityOverflow)
}

fn validation_summary(
    maximum_gain_power_error: f32,
    audio: &AudioMetrics,
    discontinuity: &DiscontinuityMetrics,
    allocations: &AllocationObservation,
    hooks: &[crate::HookEvidence],
    config: &EvaluationConfig,
) -> ValidationSummary {
    let mut findings = vec![
        finding_bool("finite_output", true, !audio.contains_non_finite),
        finding_bool("no_clipping", true, !audio.clipping_detected),
        finding_limit(
            "gain_power_normalization",
            true,
            maximum_gain_power_error as f64,
            config.thresholds.max_gain_power_error as f64,
        ),
        finding_limit(
            "gain_discontinuity",
            true,
            discontinuity.maximum_gain_delta as f64,
            config.thresholds.max_gain_discontinuity as f64,
        ),
        finding_limit(
            "delay_discontinuity",
            true,
            discontinuity.maximum_delay_delta_samples as f64,
            config.thresholds.max_delay_discontinuity_samples as f64,
        ),
        finding_limit(
            "audio_sample_delta_proxy",
            false,
            discontinuity.maximum_audio_sample_delta as f64,
            config.thresholds.max_audio_discontinuity as f64,
        ),
        ValidationFinding {
            id: "steady_state_allocations".to_owned(),
            status: allocations.status,
            required: true,
            observed: allocations
                .steady_state_allocations
                .map(|value| value as f64),
            limit: Some(0.0),
        },
    ];
    findings.extend(hooks.iter().map(|hook| ValidationFinding {
        id: format!("hook:{}", hook.id),
        status: hook.status,
        required: hook.required,
        observed: None,
        limit: None,
    }));
    let status = if findings
        .iter()
        .any(|finding| finding.required && finding.status == EvidenceStatus::Fail)
    {
        EvidenceStatus::Fail
    } else if findings
        .iter()
        .any(|finding| finding.required && finding.status == EvidenceStatus::NotObserved)
    {
        EvidenceStatus::NotObserved
    } else {
        EvidenceStatus::Pass
    };
    ValidationSummary { status, findings }
}

fn finding_bool(id: &str, required: bool, passed: bool) -> ValidationFinding {
    ValidationFinding {
        id: id.to_owned(),
        status: if passed {
            EvidenceStatus::Pass
        } else {
            EvidenceStatus::Fail
        },
        required,
        observed: None,
        limit: None,
    }
}

fn finding_limit(id: &str, required: bool, observed: f64, limit: f64) -> ValidationFinding {
    ValidationFinding {
        id: id.to_owned(),
        status: if observed.is_finite() && observed <= limit {
            EvidenceStatus::Pass
        } else {
            EvidenceStatus::Fail
        },
        required,
        observed: Some(observed),
        limit: Some(limit),
    }
}

fn duration_ns_u64(value: u128) -> u64 {
    value.min(u128::from(u64::MAX)) as u64
}

/// Returns a stable lowercase hexadecimal FNV-1a 64-bit regression fingerprint.
///
/// This is not a cryptographic integrity digest.
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash = FNV1A64_OFFSET;
    for byte in bytes {
        hash = fnv1a64_update(hash, *byte);
    }
    format!("{hash:016x}")
}

const FNV1A64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A64_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a64_update(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte)).wrapping_mul(FNV1A64_PRIME)
}
