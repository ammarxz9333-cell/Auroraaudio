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
    #[arg(long, value_enum, default_value_t = RendererSelection::All)]
    renderer: RendererSelection,
    #[arg(long, default_value = "output/evaluation")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 2.0)]
    duration_seconds: f64,
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,
    #[arg(long, default_value_t = 256)]
    block_size: usize,
    #[arg(long, default_value_t = 0.15)]
    max_gain_step: f32,
    #[arg(long, default_value_t = 64.0)]
    max_delay_step_samples: f32,
    #[arg(long, default_value_t = 0.05)]
    normalization_tolerance: f32,
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
struct Position {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Debug, Clone, Serialize)]
struct GainFrame {
    block_index: usize,
    time_seconds: f64,
    source: Position,
    gains: Vec<f32>,
    power_sum_squares: f32,
}

#[derive(Debug, Clone, Serialize)]
struct DelayFrame {
    block_index: usize,
    time_seconds: f64,
    source: Position,
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
struct Metadata {
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
struct Summary {
    renderer: String,
    passed: bool,
    artifacts: Vec<String>,
    discontinuities: DiscontinuityReport,
    performance: PerformanceReport,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate(&cli)?;
    let config = EvaluationConfig {
        sample_rate: cli.sample_rate,
        block_size: cli.block_size,
        duration_seconds: cli.duration_seconds,
        max_gain_step: cli.max_gain_step,
        max_delay_step_samples: cli.max_delay_step_samples,
        normalization_tolerance: cli.normalization_tolerance,
        max_p95_us: cli.max_p95_us,
    };
    let command = reproducible_command();
    let selections = match cli.renderer {
        RendererSelection::All => vec![
            RendererSelection::GeometricBinaural,
            RendererSelection::InverseDistance,
        ],
        selection => vec![selection],
    };

    let mut failed = Vec::new();
    for selection in selections {
        let name = selection_name(selection);
        let output = cli.output_dir.join(name);
        match evaluate(selection, &config, &output, &command) {
            Ok(summary) => {
                println!(
                    "renderer={} passed={} p95_us={:.3} max_gain_step={:.6} max_delay_step_samples={:.6} output={}",
                    summary.renderer,
                    summary.passed,
                    summary.performance.p95_us,
                    summary.discontinuities.observed_max_gain_step,
                    summary.discontinuities.observed_max_delay_step_samples,
                    output.display()
                );
                if !summary.passed {
                    failed.push(summary.renderer);
                }
            }
            Err(error) => {
                eprintln!("renderer={name} evaluation_error={error:#}");
                failed.push(name.to_owned());
            }
        }
    }
    if !failed.is_empty() {
        bail!("renderer evaluation failed for: {}", failed.join(", "));
    }
    Ok(())
}

fn validate(cli: &Cli) -> Result<()> {
    if cli.sample_rate == 0 || cli.block_size == 0 {
        bail!("sample rate and block size must be greater than zero");
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

fn evaluate(
    selection: RendererSelection,
    config: &EvaluationConfig,
    output_dir: &Path,
    command: &str,
) -> Result<Summary> {
    fs::create_dir_all(output_dir)
        .with_context(|| format!("create {}", output_dir.display()))?;
    let (renderer_name, mode_name, mode, speakers) = renderer_definition(selection);
    let roles = speakers
        .iter()
        .map(|speaker| speaker.channel_role.clone())
        .collect::<Vec<_>>();
    let role_names = roles
        .iter()
        .map(|role| role.as_str().to_owned())
        .collect::<Vec<_>>();

    let mut renderer = BasicRenderer::new(mode);
    renderer
        .configure(speakers, config.sample_rate, config.block_size, 1)
        .context("configure renderer")?;
    let mut scratch = RendererScratch::new(
        renderer
            .required_scratch_size()
            .context("query scratch size")?,
    );
    let output_channels = renderer.output_channel_count();
    let total_frames = (config.duration_seconds * f64::from(config.sample_rate)).round() as usize;
    let blocks = (total_frames + config.block_size - 1) / config.block_size;
    let mut pcm = (0..output_channels)
        .map(|_| Vec::with_capacity(total_frames))
        .collect::<Vec<_>>();
    let mut gain_frames = Vec::with_capacity(blocks);
    let mut delay_frames = Vec::with_capacity(blocks);
    let mut timings = Vec::with_capacity(blocks);
    let mut previous_gains: Option<Vec<f32>> = None;
    let mut previous_delays: Option<Vec<f32>> = None;
    let mut discontinuities = DiscontinuityReport {
        configured_max_gain_step: config.max_gain_step,
        configured_max_delay_step_samples: config.max_delay_step_samples,
        observed_max_gain_step: 0.0,
        observed_max_delay_step_samples: 0.0,
        gain_step_violations: 0,
        delay_step_violations: 0,
        normalization_failures: 0,
        non_finite_failures: 0,
    };
    let listener = Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 1.2,
    };

    for block_index in 0..blocks {
        let start_frame = block_index * config.block_size;
        let frames = (total_frames - start_frame).min(config.block_size);
        let progress = if blocks > 1 {
            block_index as f32 / (blocks - 1) as f32
        } else {
            0.0
        };
        let position = source_position(progress);
        let mut rendered = vec![SpeakerGain::default(); output_channels];
        let started = Instant::now();
        renderer
            .render_gains(
                &listener,
                &[RenderObject {
                    position,
                    gain: 1.0,
                }],
                &mut rendered,
                &mut scratch,
            )
            .context("render block")?;
        timings.push(started.elapsed().as_nanos());

        let gains = rendered.iter().map(|entry| entry.gain).collect::<Vec<_>>();
        let delays = rendered
            .iter()
            .map(|entry| entry.delay_samples)
            .collect::<Vec<_>>();
        if gains.iter().any(|value| !value.is_finite())
            || delays.iter().any(|value| !value.is_finite())
        {
            discontinuities.non_finite_failures += 1;
        }
        let power = gains.iter().map(|gain| gain * gain).sum::<f32>();
        if !power.is_finite() || (power - 1.0).abs() > config.normalization_tolerance {
            discontinuities.normalization_failures += 1;
        }
        compare_steps(
            previous_gains.as_deref(),
            &gains,
            config.max_gain_step,
            &mut discontinuities.observed_max_gain_step,
            &mut discontinuities.gain_step_violations,
        );
        compare_steps(
            previous_delays.as_deref(),
            &delays,
            config.max_delay_step_samples,
            &mut discontinuities.observed_max_delay_step_samples,
            &mut discontinuities.delay_step_violations,
        );

        let source = Position {
            x: position.x,
            y: position.y,
            z: position.z,
        };
        let time_seconds = start_frame as f64 / f64::from(config.sample_rate);
        gain_frames.push(GainFrame {
            block_index,
            time_seconds,
            source: source.clone(),
            gains: gains.clone(),
            power_sum_squares: power,
        });
        delay_frames.push(DelayFrame {
            block_index,
            time_seconds,
            source,
            delay_samples: delays.clone(),
        });

        for local_frame in 0..frames {
            let frame = start_frame + local_frame;
            let phase = TAU * 440.0 * frame as f32 / config.sample_rate as f32;
            let sample = 0.1 * phase.sin();
            for (channel, gain) in pcm.iter_mut().zip(&gains) {
                channel.push(sample * *gain);
            }
        }
        previous_gains = Some(gains);
        previous_delays = Some(delays);
    }

    let performance = performance(&timings, config.max_p95_us);
    let metadata = Metadata {
        renderer: renderer_name.to_owned(),
        renderer_mode: mode_name.to_owned(),
        sample_rate: config.sample_rate,
        block_size: config.block_size,
        output_channels,
        output_roles: role_names.clone(),
        renderer_latency_frames: renderer.latency_frames(),
        configuration_hash_fnv1a64: configuration_hash(config, renderer_name, &role_names)?,
        commit_sha: resolve_commit_sha(),
        scene: "deterministic full-circle 3D source with vertical excursion",
        wav_semantics: "gain-routing reference; delay trajectory is evidence but is not applied to PCM",
    };

    let wav_path = output_dir.join("rendered-reference.wav");
    write_wav_f32_with_channel_roles(&wav_path, config.sample_rate, &pcm, &roles)
        .with_context(|| format!("write {}", wav_path.display()))?;
    write_json(output_dir.join("gain-trajectory.json"), &gain_frames)?;
    write_json(output_dir.join("delay-trajectory.json"), &delay_frames)?;
    write_json(output_dir.join("discontinuities.json"), &discontinuities)?;
    write_json(output_dir.join("performance.json"), &performance)?;
    write_json(output_dir.join("metadata.json"), &metadata)?;
    fs::write(output_dir.join("command.txt"), format!("{command}\n"))
        .context("write command artifact")?;

    let passed = discontinuities.gain_step_violations == 0
        && discontinuities.delay_step_violations == 0
        && discontinuities.normalization_failures == 0
        && discontinuities.non_finite_failures == 0
        && performance.p95_us <= config.max_p95_us;
    let summary = Summary {
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

fn compare_steps(
    previous: Option<&[f32]>,
    current: &[f32],
    threshold: f32,
    observed_max: &mut f32,
    violations: &mut usize,
) {
    if let Some(previous) = previous {
        for (before, after) in previous.iter().zip(current) {
            let step = (after - before).abs();
            *observed_max = observed_max.max(step);
            if step > threshold {
                *violations += 1;
            }
        }
    }
}

fn renderer_definition(
    selection: RendererSelection,
) -> (&'static str, &'static str, BasicRendererMode, Vec<Speaker>) {
    match selection {
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
    }
}

fn selection_name(selection: RendererSelection) -> &'static str {
    match selection {
        RendererSelection::All => "all",
        RendererSelection::GeometricBinaural => "geometric-binaural",
        RendererSelection::InverseDistance => "inverse-distance",
    }
}

fn source_position(progress: f32) -> Vector3 {
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
        speaker("front-left", "Front Left", ChannelRole::FrontLeft, Vector3::new(-1.2, 1.7, 1.2)),
        speaker("front-right", "Front Right", ChannelRole::FrontRight, Vector3::new(1.2, 1.7, 1.2)),
        speaker("front-center", "Front Center", ChannelRole::FrontCenter, Vector3::new(0.0, 1.8, 1.2)),
        speaker("lfe", "LFE", ChannelRole::LowFrequencyEffects, Vector3::new(0.0, 1.2, 0.2)),
        speaker("surround-left", "Surround Left", ChannelRole::SurroundLeft, Vector3::new(-1.7, -0.4, 1.2)),
        speaker("surround-right", "Surround Right", ChannelRole::SurroundRight, Vector3::new(1.7, -0.4, 1.2)),
        speaker("top-front-left", "Top Front Left", ChannelRole::TopFrontLeft, Vector3::new(-0.8, 0.9, 2.5)),
        speaker("top-front-right", "Top Front Right", ChannelRole::TopFrontRight, Vector3::new(0.8, 0.9, 2.5)),
    ]
}

fn speaker(id: &str, label: &str, role: ChannelRole, position: Vector3) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: label.to_owned(),
        channel_role: role,
        position,
        orientation: Vector3::new(0.0, -1.0, 0.0),
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn performance(times_ns: &[u128], max_p95_us: f64) -> PerformanceReport {
    let peak_memory_bytes = peak_memory_bytes();
    PerformanceReport {
        measured_blocks: times_ns.len(),
        p50_us: percentile_us(times_ns, 0.50),
        p95_us: percentile_us(times_ns, 0.95),
        p99_us: percentile_us(times_ns, 0.99),
        max_us: times_ns.iter().copied().max().map(ns_to_us).unwrap_or(0.0),
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
    let index = ((sorted.len() - 1) as f64 * quantile.clamp(0.0, 1.0)).round() as usize;
    ns_to_us(sorted[index])
}

fn ns_to_us(value: u128) -> f64 {
    value as f64 / 1_000.0
}

#[cfg(target_os = "linux")]
fn peak_memory_bytes() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let kib = status
        .lines()
        .find(|line| line.starts_with("VmHWM:"))?
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

fn configuration_hash(config: &EvaluationConfig, renderer: &str, roles: &[String]) -> Result<String> {
    let mut bytes = serde_json::to_vec(config).context("serialize evaluation config")?;
    bytes.extend_from_slice(renderer.as_bytes());
    for role in roles {
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
            let value = value.trim();
            if !value.is_empty() {
                return value.to_owned();
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
        .map(|argument| {
            if argument
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-._/:=\\".contains(character))
            {
                argument
            } else {
                format!("\"{}\"", argument.replace('\\', "\\\\").replace('"', "\\\""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_json(path: PathBuf, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).context("serialize evaluation artifact")?;
    fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_is_deterministic() {
        let values = [1_000_u128, 2_000, 3_000, 4_000, 5_000];
        assert_eq!(percentile_us(&values, 0.50), 3.0);
        assert_eq!(percentile_us(&values, 0.95), 5.0);
    }

    #[test]
    fn source_path_contains_front_rear_and_height() {
        assert!(source_position(0.0).y > 0.0);
        assert!(source_position(0.5).y < 0.0);
        assert!(source_position(0.125).z > 1.2);
    }

    #[test]
    fn hash_changes_with_renderer_identity() {
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
        assert_ne!(
            configuration_hash(&config, "a", &roles).unwrap(),
            configuration_hash(&config, "b", &roles).unwrap()
        );
    }
}
