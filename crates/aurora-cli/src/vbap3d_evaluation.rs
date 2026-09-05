//! Deterministic offline evidence generation for Aurora's experimental 3D VBAP renderer.
//!
//! This module is intentionally software-only. It validates the renderer against
//! the canonical 5.1.2 scene, emits machine-readable gain/delay trajectories and
//! a multichannel reference WAV, and does not claim physical or product readiness.

use std::f32::consts::TAU;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use aurora_audio_io::write_wav_f32_with_channel_roles;
use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::Vbap3dRenderer;
use aurora_scene::load_render_scene;
use serde::Serialize;

/// Stable schema version for 3D VBAP evidence artifacts.
pub const VBAP3D_EVALUATION_SCHEMA_VERSION: u32 = 1;

/// Deterministic 3D VBAP evaluation configuration.
#[derive(Debug, Clone, Serialize)]
pub struct Vbap3dEvaluationOptions {
    /// Canonical scene used to define the listener and loudspeaker geometry.
    pub scene_path: PathBuf,
    /// Directory receiving all evidence artifacts.
    pub output_dir: PathBuf,
    /// Synthetic source duration in seconds.
    pub duration_seconds: f64,
    /// Processing sample rate.
    pub sample_rate: u32,
    /// Renderer block size.
    pub block_size: usize,
    /// Maximum accepted per-channel gain step between adjacent blocks.
    pub max_gain_step: f32,
    /// Maximum accepted deviation of the spatial gain-power sum from unity.
    pub normalization_tolerance: f32,
}

impl Default for Vbap3dEvaluationOptions {
    fn default() -> Self {
        Self {
            scene_path: PathBuf::from("fixtures/scenes/5_1_2_upfiring.json"),
            output_dir: PathBuf::from("output/evaluation/vbap3d-5.1.2"),
            duration_seconds: 2.0,
            sample_rate: 48_000,
            block_size: 256,
            max_gain_step: 0.15,
            normalization_tolerance: 0.02,
        }
    }
}

/// Final evaluation summary written to `summary.json`.
#[derive(Debug, Clone, Serialize)]
pub struct Vbap3dEvaluationSummary {
    /// Whether every configured numerical/continuity gate passed.
    pub passed: bool,
    /// Number of validated loudspeaker triplets prepared for the listener.
    pub validated_triplets: usize,
    /// Whether the listener is inside the prepared loudspeaker hull.
    pub listener_inside_hull: bool,
    /// Largest observed per-channel gain change between adjacent blocks.
    pub observed_max_gain_step: f32,
    /// Number of gain-step threshold violations.
    pub gain_step_violations: usize,
    /// Number of blocks outside the configured unit-power tolerance.
    pub normalization_failures: usize,
    /// Number of blocks containing a non-finite gain or delay.
    pub non_finite_failures: usize,
    /// Relative artifact names emitted by this run.
    pub artifacts: Vec<String>,
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

#[derive(Debug, Serialize)]
struct Metadata {
    renderer: &'static str,
    sample_rate: u32,
    block_size: usize,
    output_channels: usize,
    speaker_ids: Vec<String>,
    output_roles: Vec<String>,
    scene_path: String,
    scene_semantics: &'static str,
    trim_semantics: &'static str,
    wav_semantics: &'static str,
    evidence_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct ArtifactEnvelope<'a, T: Serialize> {
    schema_version: u32,
    artifact: &'static str,
    payload: &'a T,
}

/// Validate an evaluation configuration before creating output files.
pub fn validate_vbap3d_options(options: &Vbap3dEvaluationOptions) -> Result<()> {
    if options.sample_rate == 0 || options.block_size == 0 {
        bail!("sample rate and block size must be greater than zero");
    }
    if !options.duration_seconds.is_finite() || options.duration_seconds <= 0.0 {
        bail!("duration must be finite and greater than zero");
    }
    if !options.max_gain_step.is_finite() || options.max_gain_step < 0.0 {
        bail!("max gain step must be finite and non-negative");
    }
    if !options.normalization_tolerance.is_finite() || options.normalization_tolerance < 0.0 {
        bail!("normalization tolerance must be finite and non-negative");
    }
    Ok(())
}

/// Run the deterministic 3D VBAP offline evaluation and emit all evidence artifacts.
pub fn evaluate_vbap3d(
    options: &Vbap3dEvaluationOptions,
    reproducible_command: &str,
) -> Result<Vbap3dEvaluationSummary> {
    validate_vbap3d_options(options)?;
    let scene = load_render_scene(&options.scene_path)
        .with_context(|| format!("load scene {}", options.scene_path.display()))?;
    if scene.speakers.is_empty() {
        bail!("evaluation scene must contain at least one loudspeaker");
    }

    fs::create_dir_all(&options.output_dir)
        .with_context(|| format!("create {}", options.output_dir.display()))?;

    // Renderer-energy evidence must not be distorted by installation trims. The
    // geometry and channel roles remain identical to the canonical fixture.
    let mut speakers = scene.speakers.clone();
    for speaker in &mut speakers {
        speaker.gain_db = 0.0;
    }

    let roles = speakers
        .iter()
        .map(|speaker| speaker.channel_role.clone())
        .collect::<Vec<_>>();
    let role_names = roles
        .iter()
        .map(|role| role.as_str().to_owned())
        .collect::<Vec<_>>();
    let speaker_ids = speakers
        .iter()
        .map(|speaker| speaker.id.clone())
        .collect::<Vec<_>>();

    let mut renderer = Vbap3dRenderer::new();
    renderer
        .configure(
            speakers.clone(),
            options.sample_rate,
            options.block_size,
            1,
        )
        .context("configure 3D VBAP renderer")?;
    renderer
        .prepare_listener(&scene.listener)
        .context("prepare 3D VBAP listener-relative hull")?;
    let validated_triplets = renderer.validated_triplets().len();
    let listener_inside_hull = renderer.listener_inside_hull();
    if validated_triplets == 0 {
        bail!("3D VBAP produced no validated loudspeaker triplets");
    }

    let output_channels = renderer.output_channel_count();
    let mut scratch = RendererScratch::new(
        renderer
            .required_scratch_size()
            .context("query 3D VBAP scratch size")?,
    );
    let total_frames =
        (options.duration_seconds * f64::from(options.sample_rate)).round() as usize;
    let blocks = total_frames.div_ceil(options.block_size);

    let mut pcm = (0..output_channels)
        .map(|_| Vec::with_capacity(total_frames))
        .collect::<Vec<_>>();
    let mut gain_frames = Vec::with_capacity(blocks);
    let mut delay_frames = Vec::with_capacity(blocks);
    let mut previous_gains: Option<Vec<f32>> = None;
    let mut observed_max_gain_step = 0.0_f32;
    let mut gain_step_violations = 0_usize;
    let mut normalization_failures = 0_usize;
    let mut non_finite_failures = 0_usize;

    for block_index in 0..blocks {
        let start_frame = block_index * options.block_size;
        let frames = (total_frames - start_frame).min(options.block_size);
        let progress = if blocks > 1 {
            block_index as f32 / (blocks - 1) as f32
        } else {
            0.0
        };
        let position = evaluation_source_position(progress, &scene.listener);
        let mut rendered = vec![SpeakerGain::default(); output_channels];
        renderer
            .render_gains(
                &scene.listener,
                &[RenderObject {
                    position,
                    gain: 1.0,
                }],
                &mut rendered,
                &mut scratch,
            )
            .context("render 3D VBAP evaluation block")?;

        let gains = rendered.iter().map(|entry| entry.gain).collect::<Vec<_>>();
        let delays = rendered
            .iter()
            .map(|entry| entry.delay_samples)
            .collect::<Vec<_>>();
        if gains.iter().any(|value| !value.is_finite())
            || delays.iter().any(|value| !value.is_finite())
        {
            non_finite_failures += 1;
        }

        let spatial_power = speakers
            .iter()
            .zip(gains.iter())
            .filter(|(speaker, _)| {
                !matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects)
            })
            .map(|(_, gain)| gain * gain)
            .sum::<f32>();
        if !spatial_power.is_finite()
            || (spatial_power - 1.0).abs() > options.normalization_tolerance
        {
            normalization_failures += 1;
        }

        if let Some(previous) = previous_gains.as_deref() {
            for (before, after) in previous.iter().zip(gains.iter()) {
                let step = (after - before).abs();
                observed_max_gain_step = observed_max_gain_step.max(step);
                if step > options.max_gain_step {
                    gain_step_violations += 1;
                }
            }
        }

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
            power_sum_squares: spatial_power,
        });
        delay_frames.push(DelayFrame {
            block_index,
            time_seconds,
            source,
            delay_samples: delays,
        });

        for local_frame in 0..frames {
            let frame = start_frame + local_frame;
            let phase = TAU * 440.0 * frame as f32 / options.sample_rate as f32;
            let sample = 0.1 * phase.sin();
            for (channel, gain) in pcm.iter_mut().zip(gains.iter()) {
                channel.push(sample * *gain);
            }
        }
        previous_gains = Some(gains);
    }

    let wav_path = options.output_dir.join("rendered-reference.wav");
    write_wav_f32_with_channel_roles(&wav_path, options.sample_rate, &pcm, &roles)
        .with_context(|| format!("write {}", wav_path.display()))?;
    write_artifact(
        options.output_dir.join("gain-trajectory.json"),
        "vbap3d-gain-trajectory",
        &gain_frames,
    )?;
    write_artifact(
        options.output_dir.join("delay-trajectory.json"),
        "vbap3d-delay-trajectory",
        &delay_frames,
    )?;

    let metadata = Metadata {
        renderer: "aurora-vbap3d",
        sample_rate: options.sample_rate,
        block_size: options.block_size,
        output_channels,
        speaker_ids,
        output_roles: role_names,
        scene_path: options.scene_path.display().to_string(),
        scene_semantics: "canonical Aurora 5.1.2 fixture; listener and speaker geometry loaded verbatim",
        trim_semantics: "speaker gain_db normalized to 0 dB for renderer-only unit-power evidence",
        wav_semantics: "440 Hz mono reference routed by block gains; reported propagation delays are not applied to PCM",
        evidence_boundary: "software-only; no Atmos/JOC, HRTF, physical hardware, acoustic, or product-readiness claim",
    };
    write_artifact(
        options.output_dir.join("metadata.json"),
        "vbap3d-metadata",
        &metadata,
    )?;
    fs::write(
        options.output_dir.join("command.txt"),
        format!("{reproducible_command}\n"),
    )
    .context("write reproducible 3D VBAP command")?;

    let passed = listener_inside_hull
        && gain_step_violations == 0
        && normalization_failures == 0
        && non_finite_failures == 0;
    let summary = Vbap3dEvaluationSummary {
        passed,
        validated_triplets,
        listener_inside_hull,
        observed_max_gain_step,
        gain_step_violations,
        normalization_failures,
        non_finite_failures,
        artifacts: vec![
            "rendered-reference.wav".to_owned(),
            "gain-trajectory.json".to_owned(),
            "delay-trajectory.json".to_owned(),
            "metadata.json".to_owned(),
            "command.txt".to_owned(),
            "summary.json".to_owned(),
        ],
    };
    write_artifact(
        options.output_dir.join("summary.json"),
        "vbap3d-summary",
        &summary,
    )?;
    Ok(summary)
}

fn evaluation_source_position(progress: f32, listener: &Listener) -> Vector3 {
    let angle = TAU * progress;
    let listener_z = listener.position.z + listener.ear_height;
    Vector3::new(
        listener.position.x + 1.1 * angle.cos(),
        listener.position.y + 1.1 * angle.sin(),
        listener_z + 0.65 * (2.0 * angle).sin(),
    )
}

fn write_artifact<T: Serialize>(path: PathBuf, artifact: &'static str, payload: &T) -> Result<()> {
    let envelope = ArtifactEnvelope {
        schema_version: VBAP3D_EVALUATION_SCHEMA_VERSION,
        artifact,
        payload,
    };
    let bytes = serde_json::to_vec_pretty(&envelope).context("serialize 3D VBAP artifact")?;
    fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_are_valid() {
        validate_vbap3d_options(&Vbap3dEvaluationOptions::default()).unwrap();
    }

    #[test]
    fn source_trajectory_exercises_positive_and_negative_elevation() {
        let listener = Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 1.2,
        };
        let high = evaluation_source_position(0.125, &listener);
        let low = evaluation_source_position(0.375, &listener);
        assert!(high.z > listener.ear_height);
        assert!(low.z < listener.ear_height);
    }

    #[test]
    fn invalid_zero_block_size_is_rejected() {
        let mut options = Vbap3dEvaluationOptions::default();
        options.block_size = 0;
        assert!(validate_vbap3d_options(&options).is_err());
    }
}
