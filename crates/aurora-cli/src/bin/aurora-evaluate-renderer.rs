use std::cmp::Ordering;
use std::f32::consts::TAU;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use aurora_audio_io::write_wav_f32_with_channel_roles;
use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};
use clap::{Parser, ValueEnum};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "aurora-evaluate-renderer")]
#[command(about = "Deterministic Aurora renderer evaluation and artifact runner")]
struct Cli {
    /// Renderer to evaluate. `all` evaluates both accepted built-in baselines.
    #[arg(long, value_enum, default_value_t = RendererSelection::All)]
    renderer: RendererSelection,

    /// Root directory for generated evidence artifacts.
    #[arg(long, default_value = "output/evaluation")]
    output_dir: PathBuf,

    /// Synthetic test duration in seconds.
    #[arg(long, default_value_t = 2.0)]
    duration_seconds: f64,

    /// Processing sample rate.
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,

    /// Renderer block size.
    #[arg(long, default_value_t = 256)]
    block_size: usize,

    /// Fail when the largest inter-block gain step exceeds this value.
    #[arg(long, default_value_t = 0.15)]
    max_gain_step: f32,

    /// Fail when the largest inter-block delay step exceeds this many samples.
    #[arg(long, default_value_t = 64.0)]
    max_delay_step_samples: f32,

    /// Allowed deviation of sum-of-squares gain power from 1.0.
    #[arg(long, default_value_t = 0.05)]
    normalization_tolerance: f32,

    /// Fail when renderer p95 processing time exceeds this many microseconds.
    #[arg(long, default_value_t = 1_000.0)]
    max_p95_us: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum RendererSelection {
    All,
    GeometricBinaural,
    InverseDistance,
}

#[derive(Debug, Clone, Serialize)]
struct EvaluationConfig {
    sample_rate: u32,
    block_size: usize,
    duration_seconds: f64,
    max_gain_step: f32,
    max_delay_step_samples: f32,
    normalization_tolerance: f32,
    max_p95_us: f64,
}

#[derive(Debug, Clone, Serialize)]
struct SourcePosition {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Debug, Clone, Serialize)]
struct GainTrajectoryFrame {
    block_index: usize,
    time_seconds: f64,
    source_position: SourcePosition,
    gains: Vec<f32>,
    power_sum_squares: f32,
}

#[derive(Debug, Clone, Serialize)]
struct DelayTrajectoryFrame {
    block_index: usize,
    time_seconds: f64,
    source_position: SourcePosition,
    delay_samples: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
struct DiscontinuityReport {
    configured_max_gain_step: f32,
    configured_max_delay_step_samples: f32,
    observed_max_gain_step: f32,
    observed_max_delay_step_samples: f32,
    gain_step_violations: usize,
    delay_step_violations: usize,
    normalization_failures: usize,
    non_finite_failures: usize,
}

#[derive(Debug, Clone, Serialize)]
struct PerformanceReport {
    measured_blocks: usize,
    p50_us: f64,
    p95_us: f64,
    p99_us: f64,
    max_us: f64,
    configured_max_p95_us: f64,
    peak_memory_bytes: Option<u64>,
    peak_memory_source: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct RendererMetadata {
    renderer: String,
    renderer_mode: String,
    sample_rate: u32,
    block_size: usize,
    output_channels: usize,
    output_roles: Vec<String>,
    renderer_latency_frames: usize,
    configuration_hash_fnv1a64: String,
    commit_sha: String,
    scene: &'static str,
    wav_semantics: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct EvaluationSummary {
    renderer: String,
    passed: bool,
    artifacts: Vec<String>,
    discontinuities: DiscontinuityReport,
    performance: PerformanceReport,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate_cli(&cli)?;

    let config = EvaluationConfig {
        sample_rate: cli.sample_rate,
        block_size: cli.block_size,
        duration_seconds: cli.duration_seconds,
        max_gain_step: cli.max_gain_step,
        max_delay_step_samples: cli.max_delay_step_samples,
        normalization_tolerance: cli.normalization_tolerance,
        max_p95_us: cli.max_p95_us,
    };
    let reproducible_command = reproducible_command();

    let selections: &[RendererSelection] = match cli.renderer {
        RendererSelection::All => &[
            RendererSelection::GeometricBinaural,
            RendererSelection::InverseDistance,
        ],
        RendererSelection::GeometricBinaural => &[RendererSelection::GeometricBinaural],
        RendererSelection::InverseDistance => &[RendererSelection::InverseDistance],
    };

    let mut failures = Vec::new();
    for selection in selections {
        let subdir = match selection {
            RendererSelection::All => unreachable!(),
            RendererSelection::GeometricBinaural => "geometric-binaural",
            RendererSelection::InverseDistance => "inverse-distance",
        };
        let output_dir = cli.output_dir.join(subdir);
        match evaluate_selection(*selection, &config, &output_dir, &reproducible_command) {
            Ok(summary) => {
                println!(
                    "renderer={} passed={} p95_us={:.3} max_gain_step={:.6} max_delay_step_samples={:.6} output={}",
                    summary.renderer,
                    summary.passed,
                    summary.performance.p95_us,
                    summary.discontinuities.observed_max_gain_step,
                    summary.discontinuities.observed_max_delay_step_samples,
                    output_dir.display()
                );
                if !summary.passed {
                    failures.push(summary.renderer);
                }
            }
            Err(error) => {
                failures.push(subdir.to_owned());
                eprintln!("renderer={subdir} evaluation_error={error:#}");
            }
        }
    }

    if !failures.is_empty() {
        bail!("renderer evaluation failed for: {}", failures.join(", "));
    }
    Ok(())
}

fn validate_cli(cli: &Cli) -> Result<()> {
    if cli.sample_rate == 0 {
        bail!("sample rate must be greater than zero");
    }
    if cli.block_size == 0 {
        bail!("block size must be greater than zero");
    }
    if !cli.duration_seconds.is_finite() || cli.duration_seconds <= 0.0 {
        bail!("duration must be finite and greater than zero");
    }
    if !cli.max_gain_step.is_finite() || cli.max_gain_step < 0.0 {
        bail!("max gain step must be finite and non-negative");
    }
    if !cli.max_delay_step_samples.is_finite() || cli.max_delay_step_samples < 0.0 {
        bail!("max delay step must be finite and non-negative");
    }
    if !cli.normalization_tolerance.is_finite() || cli.normalization_tolerance < 0.0 {
        bail!("normalization tolerance must be finite and non-negative");
    }
    if !cli.max_p95_us.is_finite() || cli.max_p95_us <= 0.0 {
        bail!("max p95 must be finite and greater than zero");
    }
    Ok(())
}

fn evaluate_selection(
    selection: RendererSelection,
    config: &EvaluationConfig,
    output_dir: &Path,
    reproducible_command: &str,
) -> Result<EvaluationSummary> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("create evaluation directory {}", output_dir.display()))?;

    let (renderer_name, renderer_mode_name, mode, speakers) = match selection {
        RendererSelection::All => unreachable!(),
        RendererSelection::GeometricBinaural => (
            "basic-geometric-binaural",
            "geometric-binaural",
            BasicRendererMode::GeometricBinaural,
            stereo_speakers(),
        ),
        RendererSelection::InverseDistance => (
            "basic-inverse-distance",
            "inverse-distance",
            BasicRendererMode::InverseDistance,
            five_one_two_speakers(),
        ),
    };

    let roles = speakers
        .iter()
        .map(|speaker| speaker.channel_role.clone())
        .collect::<Vec<_>>();
    let role_names = roles
        .iter()
        .map(|role| role.as_str().to_owned())
        .collect::<Vec<_>>();

    let config_hash = configuration_hash(config, renderer_name, &role_names)?;
    let commit_sha = resolve_commit_sha();
    let mut renderer = BasicRenderer::new(mode);
    renderer
        .configure(speakers, config.sample_rate, config.block_size, 1)
        .context("configure renderer")?;
    let scratch_size = renderer
        .required_scratch_size()
        .context("query renderer scratch size")?;
    let mut scratch = RendererScratch::new(scratch_size);
    let output_channels = renderer.output_channel_count();
    let renderer_latency_frames = renderer.latency_frames();

    let total_frames = (config.duration_seconds * f64::from(config.sample_rate)).round() as usize;
    let block_count = total_frames.div_ceil(config.block_size);
    let mut pcm = (0..output_channels)
        .map(|_| Vec::with_capacity(total_frames))
        .collect::<Vec<_>>();
    let mut gain_trajectory = Vec::with_capacity(block_count);
    let mut delay_trajectory = Vec::with_capacity(block_count);
    let mut render_times_ns = Vec::with_capacity(block_count);

    let mut previous_gains: Option<Vec<f32>> = None;
    let mut previous_delays: Option<Vec<f32>> = None;
    let mut observed_max_gain_step = 0.0_f32;
    let mut observed_max_delay_step = 0.0_f32;
    let mut gain_step_violations = 0_usize;
    let mut delay_step_violations = 0_usize;
    let mut normalization_failures = 0_usize;
    let mut non_finite_failures = 0_usize;

    let listener = Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 1.2,
    };

    for block_index in 0..block_count {
        let start_frame = block_index * config.block_size;
        let frames_this_block = (total_frames - start_frame).min(config.block_size);
        let time_seconds = start_frame as f64 / f64::from(config.sample_rate);
        let progress = if block_count > 1 {
            block_index as f32 / (block_count - 1) as f32
        } else {
            0.0
        };
        let position = deterministic_source_position(progress);
        let object = RenderObject {
            position,
            gain: 1.0,
        };
        let mut gains = vec![SpeakerGain::default(); output_channels];

        let started = Instant::now();
        renderer
            .render_gains(&listener, &[object], &mut gains, &mut scratch)
            .context("render gain block")?;
        render_times_ns.push(started.elapsed().as_nanos());

        let gain_values = gains.iter().map(|value| value.gain).collect::<Vec<_>>();
        let delay_values = gains
            .iter()
            .map(|value| value.delay_samples)
            .collect::<Vec<_>>();
        if gain_values.iter().any(|value| !value.is_finite())
            || delay_values.iter().any(|value| !value.is_finite())
        {
            non_finite_failures += 1;
        }

        let power_sum_squares = gain_values.iter().map(|value| value * value).sum::<f32>();
        if !power_sum_squares.is_finite()
            || (power_sum_squares - 1.0).abs() > config.normalization_tolerance
        {
            normalization_failures += 1;
        }

        if let Some(previous) = &previous_gains {
            for (before, after) in previous.iter().zip(&gain_values) {
                let step = (after - before).abs();
                observed_max_gain_step = observed_max_gain_step.max(step);
                if step > config.max_gain_step {
                    gain_step_violations += 1;
                }
            }
        }
        if let Some(previous) = &previous_delays {
            for (before, after) in previous.iter().zip(&delay_values) {
                let step = (after - before).abs();
                observed_max_delay_step = observed_max_delay_step.max(step);
                if step > config.max_delay_step_samples {
                    delay_step_violations += 1;
                }
            }
        }

        let source_position = SourcePosition {
            x: position.x,
            y: position.y,
            z: position.z,
        };
        gain_trajectory.push(GainTrajectoryFrame {
            block_index,
            time_seconds,
            source_position: source_position.clone(),
            gains: gain_values.clone(),
            power_sum_squares,
        });
        delay_trajectory.push(DelayTrajectoryFrame {
            block_index,
            time_seconds,
            source_position,
            delay_samples: delay_values.clone(),
        });

        for local_frame in 0..frames_this_block {
            let global_frame = start_frame + local_frame;
            let phase = TAU * 440.0 * global_frame as f32 / config.sample_rate as f32;
            let input_sample = 0.1 * phase.sin();
            for (channel, gain) in pcm.iter_mut().zip(&gain_values) {
                channel.push(input_sample * *gain);
            }
        }

        previous_gains = Some(gain_values);
        previous_delays = Some(delay_values);
    }

    let discontinuities = DiscontinuityReport {
        configured_max_gain_step: config.max_gain_step,
        configured_max_delay_step_samples: config.max_delay_step_samples,
        observed_max_gain_step,
        observed_max_delay_step_samples: observed_max_delay_step,
        gain_step_violations,
        delay_step_violations,
        normalization_failures,
        non_finite_failures,
    };
    let performance = performance_report(&render_times_ns, config.max_p95_us);
    let metadata = RendererMetadata {
        renderer: renderer_name.to_owned(),
        renderer_mode: renderer_mode_name.to_owned(),
        sample_rate: config.sample_rate,
        block_size: config.block_size,
        output_channels,
        output_roles: role_names,
        renderer_latency_frames,
        configuration_hash_fnv1a64: config_hash,
        commit_sha,
        scene: "deterministic 3D circular source with vertical excursion",
        wav_semantics: "reference gain-routing artifact; delay trajectory is reported separately and is not applied to PCM",
    };

    let wav_path = output_dir.join("rendered-reference.wav");
    write_wav_f32_with_channel_roles(&wav_path, config.sample_rate, &pcm, &roles)
        .with_context(|| format!("write {}", wav_path.display()))?;
    write_json(output_dir.join("gain-trajectory.json"), &gain_trajectory)?;
    write_json(output_dir.join("delay-trajectory.json"), &delay_trajectory)?;
    write_json(output_dir.join("discontinuities.json"), &discontinuities)?;
    write_json(output_dir.join("performance.json"), &performance)?;
    write_json(output_dir.join("metadata.json"), &metadata)?;
    fs::write(output_dir.join("command.txt"), format!("{reproducible_command}\n"))
        .context("write reproducible command")?;

    let passed = discontinuities.gain_step_violations == 0
        && discontinuities.delay_step_violations == 0
        && discontinuities.normalization_failures == 0
        && discontinuities.non_finite_failures == 0
        && performance.p95_us <= config.max_p95_us;
    let summary = EvaluationSummary {
        renderer: renderer_name.to_owned(),
        passed,
        artifacts: vec![
            "rendered-reference.wav".to_owned(),
            "gain-trajectory.json".to_owned(),
            "delay-trajectory.json".to_owned(),
            "discontinuities.json".to_owned(),
            "performance.json".to_owned(),
            "metadata.json".to_owned(),
            "command.txt".to_owned(),
            "summary.json".to_owned(),
        ],
        discontinuities,
        performance,
    };
    write_json(output_dir.join("summary.json"), &summary)?;
    Ok(summary)
}

fn deterministic_source_position(progress: f32) -> Vector3 {
    let angle = TAU * progress;
    Vector3::new(
        1.35 * angle.sin(),
        1.35 * angle.cos(),
        1.2 + 0.75 * (2.0 * angle).sin().max(0.0),
    )
}

fn stereo_speakers() -> Vec<Speaker> {
    vec![
        speaker(
            "front-left",
            "Front Left",
            ChannelRole::FrontLeft,
            Vector3::new(-0.9, 1.4, 1.2),
        ),
        speaker(
            "front-right",
            "Front Right",
            ChannelRole::FrontRight,
            Vector3::new(0.9, 1.4, 1.2),
        ),
    ]
}

fn five_one_two_speakers() -> Vec<Speaker> {
    vec![
        speaker(
            "front-left",
            "Front Left",
            ChannelRole::FrontLeft,
            Vector3::new(-1.2, 1.7, 1.2),
        ),
        speaker(
            "front-right",
            "Front Right",
            ChannelRole::FrontRight,
            Vector3::new(1.2, 1.7, 1.2),
        ),
        speaker(
            "front-center",
            "Front Center",
            ChannelRole::FrontCenter,
            Vector3::new(0.0, 1.8, 1.2),
        ),
        speaker(
            "lfe",
            "LFE",
            ChannelRole::LowFrequencyEffects,
            Vector3::new(0.0, 1.2, 0.2),
        ),
        speaker(
            "surround-left",
            "Surround Left",
            ChannelRole::SurroundLeft,
            Vector3::new(-1.7, -0.4, 1.2),
        ),
        speaker(
            "surround-right",
            "Surround Right",
            ChannelRole::SurroundRight,
            Vector3::new(1.7, -0.4, 1.2),
        ),
        speaker(
            "top-front-left",
            "Top Front Left",
            ChannelRole::TopFrontLeft,
            Vector3::new(-0.8, 0.9, 2.5),
        ),
        speaker(
            "top-front-right",
            "Top Front Right",
            ChannelRole::TopFrontRight,
            Vector3::new(0.8, 0.9, 2.5),
        ),
    ]
}

fn speaker(id: &str, label: &str, channel_role: ChannelRole, position: Vector3) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: label.to_owned(),
        channel_role,
        position,
        orientation: Vector3::new(0.0, -1.0, 0.0),
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn performance_report(render_times_ns: &[u128], max_p95_us: f64) -> PerformanceReport {
    let p50_us = percentile_us(render_times_ns, 0.50);
    let p95_us = percentile_us(render_times_ns, 0.95);
    let p99_us = percentile_us(render_times_ns, 0.99);
    let max_us = render_times_ns
        .iter()
        .copied()
        .max()
        .map(ns_to_us)
        .unwrap_or(0.0);
    let peak_memory_bytes = peak_memory_bytes();
    PerformanceReport {
        measured_blocks: render_times_ns.len(),
        p50_us,
        p95_us,
        p99_us,
        max_us,
        configured_max_p95_us: max_p95_us,
        peak_memory_bytes,
        peak_memory_source: if cfg!(target_os = "linux") {
            "/proc/self/status VmHWM"
        } else {
            "unavailable on this platform"
        },
    }
}

fn percentile_us(values: &[u128], quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let clamped = quantile.clamp(0.0, 1.0);
    let index = ((sorted.len() - 1) as f64 * clamped).round() as usize;
    ns_to_us(sorted[index])
}

fn ns_to_us(value: u128) -> f64 {
    value as f64 / 1_000.0
}

#[cfg(target_os = "linux")]
fn peak_memory_bytes() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    let kib = line
        .split_whitespace()
        .nth(1)?
        .parse::<u64>()
        .ok()?;
    kib.checked_mul(1024)
}

#[cfg(not(target_os = "linux"))]
fn peak_memory_bytes() -> Option<u64> {
    None
}

fn configuration_hash(
    config: &EvaluationConfig,
    renderer_name: &str,
    role_names: &[String],
) -> Result<String> {
    let mut bytes = serde_json::to_vec(config).context("serialize evaluation configuration")?;
    bytes.extend_from_slice(renderer_name.as_bytes());
    for role in role_names {
        bytes.push(0);
        bytes.extend_from_slice(role.as_bytes());
    }
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("{hash:016x}"))
}

fn resolve_commit_sha() -> String {
    for variable in ["AURORA_COMMIT_SHA", "GITHUB_SHA"] {
        if let Ok(value) = std::env::var(variable) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_owned();
            }
        }
    }
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn reproducible_command() -> String {
    std::env::args()
        .map(|argument| shell_quote(&argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-._/:=\\".contains(character))
    {
        value.to_owned()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).context("serialize evaluation artifact")?;
    fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_is_stable_for_known_values() {
        let values = [1_000_u128, 2_000, 3_000, 4_000, 5_000];
        assert_eq!(percentile_us(&values, 0.50), 3.0);
        assert_eq!(percentile_us(&values, 0.95), 5.0);
    }

    #[test]
    fn configuration_hash_changes_with_renderer_identity() {
        let config = EvaluationConfig {
            sample_rate: 48_000,
            block_size: 256,
            duration_seconds: 1.0,
            max_gain_step: 0.15,
            max_delay_step_samples: 64.0,
            normalization_tolerance: 0.05,
            max_p95_us: 1_000.0,
        };
        let roles = vec!["front-left".to_owned(), "front-right".to_owned()];
        let first = configuration_hash(&config, "a", &roles).unwrap();
        let second = configuration_hash(&config, "b", &roles).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn source_trajectory_covers_rear_and_height() {
        let front = deterministic_source_position(0.0);
        let rear = deterministic_source_position(0.5);
        let elevated = deterministic_source_position(0.125);
        assert!(front.y > 0.0);
        assert!(rear.y < 0.0);
        assert!(elevated.z > 1.2);
    }

    #[test]
    fn floating_point_sort_fallback_is_not_required() {
        let mut values = [3.0_f64, 1.0, 2.0];
        values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
        assert_eq!(values, [1.0, 2.0, 3.0]);
    }
}
