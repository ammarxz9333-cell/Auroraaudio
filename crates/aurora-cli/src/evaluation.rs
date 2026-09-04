//! Deterministic, software-only renderer evaluation.
//!
//! The evaluation path produces versioned machine-readable artifacts and a
//! reference WAV while keeping physical-product and perceptual claims outside
//! this software evidence boundary.

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
use serde::Serialize;

/// Current JSON artifact schema version.
pub const EVALUATION_SCHEMA_VERSION: u32 = 1;

/// Built-in renderer target supported by the unified evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererTarget {
    /// Accepted lightweight stereo geometric baseline.
    GeometricBinaural,
    /// Existing deterministic loudspeaker inverse-distance renderer.
    InverseDistance,
}

impl RendererTarget {
    /// Stable artifact-directory name.
    pub const fn slug(self) -> &'static str {
        match self {
            Self::GeometricBinaural => "geometric-binaural",
            Self::InverseDistance => "inverse-distance",
        }
    }
}

/// Deterministic evaluation configuration and failure thresholds.
#[derive(Debug, Clone, Serialize)]
pub struct EvaluationOptions {
    /// Root output directory. Each renderer receives its own subdirectory.
    pub output_dir: PathBuf,
    /// Synthetic source duration in seconds.
    pub duration_seconds: f64,
    /// Processing sample rate.
    pub sample_rate: u32,
    /// Renderer block size.
    pub block_size: usize,
    /// Maximum accepted gain change between adjacent evaluated blocks.
    pub max_gain_step: f32,
    /// Maximum accepted delay change between adjacent evaluated blocks.
    pub max_delay_step_samples: f32,
    /// Maximum accepted deviation of gain-power sum from unity.
    pub normalization_tolerance: f32,
    /// Maximum accepted p95 `Renderer::render_gains` processing time.
    pub max_p95_us: f64,
}

impl Default for EvaluationOptions {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("output/evaluation"),
            duration_seconds: 2.0,
            sample_rate: 48_000,
            block_size: 256,
            max_gain_step: 0.15,
            max_delay_step_samples: 64.0,
            normalization_tolerance: 0.05,
            max_p95_us: 1_000.0,
        }
    }
}

/// One renderer's evaluation result.
#[derive(Debug, Clone, Serialize)]
pub struct EvaluationSummary {
    /// Stable renderer identity.
    pub renderer: String,
    /// Whether every configured evidence threshold passed.
    pub passed: bool,
    /// Relative artifact filenames emitted for this renderer.
    pub artifacts: Vec<String>,
    /// Discontinuity and numerical-safety evidence.
    pub discontinuities: DiscontinuityReport,
    /// Timing and bounded-memory evidence.
    pub performance: PerformanceReport,
}

/// Result of evaluating one or more renderers.
#[derive(Debug, Clone)]
pub struct EvaluationRun {
    /// Per-renderer summaries in requested order.
    pub summaries: Vec<EvaluationSummary>,
}

impl EvaluationRun {
    /// Returns true when every selected renderer passed.
    pub fn passed(&self) -> bool {
        self.summaries.iter().all(|summary| summary.passed)
    }

    /// Returns stable renderer names that failed at least one threshold.
    pub fn failed_renderers(&self) -> Vec<&str> {
        self.summaries
            .iter()
            .filter(|summary| !summary.passed)
            .map(|summary| summary.renderer.as_str())
            .collect()
    }
}

/// Discontinuity, normalization, and finite-value evidence.
#[derive(Debug, Clone, Serialize)]
pub struct DiscontinuityReport {
    /// Configured maximum inter-block gain step.
    pub configured_max_gain_step: f32,
    /// Configured maximum inter-block delay step.
    pub configured_max_delay_step_samples: f32,
    /// Largest measured gain step.
    pub observed_max_gain_step: f32,
    /// Largest measured delay step.
    pub observed_max_delay_step_samples: f32,
    /// Number of gain-step threshold violations.
    pub gain_step_violations: usize,
    /// Number of delay-step threshold violations.
    pub delay_step_violations: usize,
    /// Number of blocks outside the configured normalization tolerance.
    pub normalization_failures: usize,
    /// Number of blocks containing non-finite gains or delays.
    pub non_finite_failures: usize,
}

/// Renderer processing-time and process peak-memory evidence.
#[derive(Debug, Clone, Serialize)]
pub struct PerformanceReport {
    /// Number of measured calls to `Renderer::render_gains`.
    pub measured_blocks: usize,
    /// Median call cost in microseconds.
    pub p50_us: f64,
    /// 95th-percentile call cost in microseconds.
    pub p95_us: f64,
    /// 99th-percentile call cost in microseconds.
    pub p99_us: f64,
    /// Maximum observed call cost in microseconds.
    pub max_us: f64,
    /// Configured p95 regression threshold.
    pub configured_max_p95_us: f64,
    /// Process high-water resident memory when the host exposes it.
    pub peak_memory_bytes: Option<u64>,
    /// Source used for peak-memory evidence.
    pub peak_memory_source: &'static str,
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
    evidence_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct ArtifactEnvelope<'a, T: Serialize> {
    schema_version: u32,
    artifact: &'static str,
    payload: &'a T,
}

/// Validates options before any artifact directory is created.
pub fn validate_options(options: &EvaluationOptions) -> Result<()> {
    if options.sample_rate == 0 || options.block_size == 0 {
        bail!("sample rate and block size must be greater than zero");
    }
    if !options.duration_seconds.is_finite() || options.duration_seconds <= 0.0 {
        bail!("duration must be finite and greater than zero");
    }
    if !options.max_gain_step.is_finite() || options.max_gain_step < 0.0 {
        bail!("max gain step must be finite and non-negative");
    }
    if !options.max_delay_step_samples.is_finite() || options.max_delay_step_samples < 0.0 {
        bail!("max delay step must be finite and non-negative");
    }
    if !options.normalization_tolerance.is_finite() || options.normalization_tolerance < 0.0 {
        bail!("normalization tolerance must be finite and non-negative");
    }
    if !options.max_p95_us.is_finite() || options.max_p95_us <= 0.0 {
        bail!("max p95 must be finite and greater than zero");
    }
    Ok(())
}

/// Evaluates selected renderers and writes versioned evidence artifacts.
///
/// `reproducible_command` is stored verbatim in `command.txt`. Callers should
/// pass the exact command surface they want future users or CI to reproduce.
pub fn evaluate_renderers(
    targets: &[RendererTarget],
    options: &EvaluationOptions,
    reproducible_command: &str,
) -> Result<EvaluationRun> {
    validate_options(options)?;
    if targets.is_empty() {
        bail!("at least one renderer target is required");
    }

    let summaries = targets
        .iter()
        .copied()
        .map(|target| {
            evaluate_one(
                target,
                options,
                &options.output_dir.join(target.slug()),
                reproducible_command,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(EvaluationRun { summaries })
}

fn evaluate_one(
    target: RendererTarget,
    options: &EvaluationOptions,
    output_dir: &Path,
    command: &str,
) -> Result<EvaluationSummary> {
    fs::create_dir_all(output_dir).with_context(|| format!("create {}", output_dir.display()))?;
    let (renderer_name, mode_name, mode, speakers) = renderer_definition(target);
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
        .configure(speakers, options.sample_rate, options.block_size, 1)
        .context("configure renderer")?;
    let mut scratch = RendererScratch::new(
        renderer
            .required_scratch_size()
            .context("query renderer scratch size")?,
    );
    let output_channels = renderer.output_channel_count();
    let total_frames = (options.duration_seconds * f64::from(options.sample_rate)).round() as usize;
    let blocks = total_frames.div_ceil(options.block_size);

    let mut pcm = (0..output_channels)
        .map(|_| Vec::with_capacity(total_frames))
        .collect::<Vec<_>>();
    let mut gain_frames = Vec::with_capacity(blocks);
    let mut delay_frames = Vec::with_capacity(blocks);
    let mut timings = Vec::with_capacity(blocks);
    let mut previous_gains: Option<Vec<f32>> = None;
    let mut previous_delays: Option<Vec<f32>> = None;
    let mut discontinuities = DiscontinuityReport {
        configured_max_gain_step: options.max_gain_step,
        configured_max_delay_step_samples: options.max_delay_step_samples,
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
        let start_frame = block_index * options.block_size;
        let frames = (total_frames - start_frame).min(options.block_size);
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
            .context("render evaluation block")?;
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
        if !power.is_finite() || (power - 1.0).abs() > options.normalization_tolerance {
            discontinuities.normalization_failures += 1;
        }
        compare_steps(
            previous_gains.as_deref(),
            &gains,
            options.max_gain_step,
            &mut discontinuities.observed_max_gain_step,
            &mut discontinuities.gain_step_violations,
        );
        compare_steps(
            previous_delays.as_deref(),
            &delays,
            options.max_delay_step_samples,
            &mut discontinuities.observed_max_delay_step_samples,
            &mut discontinuities.delay_step_violations,
        );

        let source = Position {
            x: position.x,
            y: position.y,
            z: position.z,
        };
        let time_seconds = start_frame as f64 / f64::from(options.sample_rate);
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
            let phase = TAU * 440.0 * frame as f32 / options.sample_rate as f32;
            let sample = 0.1 * phase.sin();
            for (channel, gain) in pcm.iter_mut().zip(&gains) {
                channel.push(sample * *gain);
            }
        }
        previous_gains = Some(gains);
        previous_delays = Some(delays);
    }

    let performance = performance_report(&timings, options.max_p95_us);
    let metadata = Metadata {
        renderer: renderer_name.to_owned(),
        renderer_mode: mode_name.to_owned(),
        sample_rate: options.sample_rate,
        block_size: options.block_size,
        output_channels,
        output_roles: role_names.clone(),
        renderer_latency_frames: renderer.latency_frames(),
        configuration_hash_fnv1a64: configuration_hash(options, renderer_name, &role_names)?,
        commit_sha: resolve_commit_sha(),
        scene: "deterministic full-circle 3D source with vertical excursion",
        wav_semantics:
            "gain-routing reference; delay trajectory is evidence but is not applied to PCM",
        evidence_boundary:
            "software-only; no Atmos/JOC, HRTF, physical hardware, acoustic, or product-readiness claim",
    };

    let wav_path = output_dir.join("rendered-reference.wav");
    write_wav_f32_with_channel_roles(&wav_path, options.sample_rate, &pcm, &roles)
        .with_context(|| format!("write {}", wav_path.display()))?;
    write_artifact(
        output_dir.join("gain-trajectory.json"),
        "gain-trajectory",
        &gain_frames,
    )?;
    write_artifact(
        output_dir.join("delay-trajectory.json"),
        "delay-trajectory",
        &delay_frames,
    )?;
    write_artifact(
        output_dir.join("discontinuities.json"),
        "discontinuities",
        &discontinuities,
    )?;
    write_artifact(
        output_dir.join("performance.json"),
        "performance",
        &performance,
    )?;
    write_artifact(output_dir.join("metadata.json"), "metadata", &metadata)?;
    fs::write(output_dir.join("command.txt"), format!("{command}\n"))
        .context("write reproducible command")?;

    let passed = discontinuities.gain_step_violations == 0
        && discontinuities.delay_step_violations == 0
        && discontinuities.normalization_failures == 0
        && discontinuities.non_finite_failures == 0
        && performance.p95_us <= options.max_p95_us;
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
    write_artifact(output_dir.join("summary.json"), "summary", &summary)?;
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
    target: RendererTarget,
) -> (&'static str, &'static str, BasicRendererMode, Vec<Speaker>) {
    match target {
        RendererTarget::GeometricBinaural => (
            "basic-geometric-binaural",
            "geometric-binaural",
            BasicRendererMode::GeometricBinaural,
            stereo_speakers(),
        ),
        RendererTarget::InverseDistance => (
            "basic-inverse-distance",
            "inverse-distance",
            BasicRendererMode::InverseDistance,
            five_one_two_speakers(),
        ),
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

fn performance_report(times_ns: &[u128], max_p95_us: f64) -> PerformanceReport {
    PerformanceReport {
        measured_blocks: times_ns.len(),
        p50_us: percentile_us(times_ns, 0.50),
        p95_us: percentile_us(times_ns, 0.95),
        p99_us: percentile_us(times_ns, 0.99),
        max_us: times_ns.iter().copied().max().map(ns_to_us).unwrap_or(0.0),
        configured_max_p95_us: max_p95_us,
        peak_memory_bytes: peak_memory_bytes(),
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

fn configuration_hash(
    options: &EvaluationOptions,
    renderer: &str,
    roles: &[String],
) -> Result<String> {
    let mut bytes = serde_json::to_vec(options).context("serialize evaluation configuration")?;
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

fn write_artifact<T: Serialize>(path: PathBuf, artifact: &'static str, payload: &T) -> Result<()> {
    let envelope = ArtifactEnvelope {
        schema_version: EVALUATION_SCHEMA_VERSION,
        artifact,
        payload,
    };
    let bytes = serde_json::to_vec_pretty(&envelope).context("serialize evaluation artifact")?;
    fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_are_valid() {
        validate_options(&EvaluationOptions::default()).unwrap();
    }

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
    fn renderer_slugs_are_stable() {
        assert_eq!(
            RendererTarget::GeometricBinaural.slug(),
            "geometric-binaural"
        );
        assert_eq!(RendererTarget::InverseDistance.slug(), "inverse-distance");
    }

    #[test]
    fn hash_changes_with_renderer_identity() {
        let options = EvaluationOptions::default();
        let roles = vec!["front-left".to_owned(), "front-right".to_owned()];
        assert_ne!(
            configuration_hash(&options, "a", &roles).unwrap(),
            configuration_hash(&options, "b", &roles).unwrap()
        );
    }
}
