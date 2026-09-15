use std::f32::consts::TAU;
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::Vbap3dRenderer;
use clap::Parser;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Parser)]
#[command(name = "aurora-validate-immersive-layout")]
#[command(about = "Generate deterministic geometry-only 3D VBAP evidence for standard or custom immersive layouts")]
struct Cli {
    #[arg(long)]
    scene: PathBuf,
    #[arg(long)]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 1.0)]
    duration_seconds: f64,
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,
    #[arg(long)]
    block_size: Option<usize>,
    #[arg(long, default_value_t = 0.15)]
    max_gain_step: f32,
    #[arg(long, default_value_t = 0.001)]
    normalization_tolerance: f32,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    #[serde(default = "default_layout_name")]
    layout: String,
    listener: Listener,
    speakers: Vec<Speaker>,
    block_size: usize,
}

fn default_layout_name() -> String {
    "custom".to_owned()
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
    spatial_power_sum_squares: f32,
}

#[derive(Debug, Serialize)]
struct Summary {
    passed: bool,
    layout: String,
    output_channels: usize,
    spatial_channels: usize,
    lfe_channels: usize,
    top_channels: usize,
    custom_role_channels: usize,
    validated_triplets: usize,
    listener_inside_hull: bool,
    observed_max_gain_step: f32,
    configured_max_gain_step: f32,
    gain_step_violations: usize,
    normalization_failures: usize,
    non_finite_failures: usize,
    lfe_nonzero_frames: usize,
    frame_count: usize,
    speaker_ids: Vec<String>,
    output_roles: Vec<String>,
    truth_boundary: &'static str,
}

#[derive(Debug, Serialize)]
struct ArtifactEnvelope<'a, T: Serialize> {
    schema_version: u32,
    artifact: &'static str,
    payload: &'a T,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate_cli(&cli)?;
    let summary = evaluate(&cli)?;
    println!(
        "layout={} passed={} outputs={} spatial={} triplets={} closed_hull={} max_gain_step={:.6} normalization_failures={} lfe_nonzero_frames={}",
        summary.layout,
        summary.passed,
        summary.output_channels,
        summary.spatial_channels,
        summary.validated_triplets,
        summary.listener_inside_hull,
        summary.observed_max_gain_step,
        summary.normalization_failures,
        summary.lfe_nonzero_frames,
    );
    if !summary.passed {
        bail!("immersive layout validation failed one or more software evidence gates");
    }
    Ok(())
}

fn validate_cli(cli: &Cli) -> Result<()> {
    if cli.sample_rate == 0 {
        bail!("sample rate must be greater than zero");
    }
    if cli.block_size == Some(0) {
        bail!("block size must be greater than zero");
    }
    if !cli.duration_seconds.is_finite() || cli.duration_seconds <= 0.0 {
        bail!("duration must be finite and greater than zero");
    }
    if !cli.max_gain_step.is_finite() || cli.max_gain_step < 0.0 {
        bail!("max gain step must be finite and non-negative");
    }
    if !cli.normalization_tolerance.is_finite() || cli.normalization_tolerance < 0.0 {
        bail!("normalization tolerance must be finite and non-negative");
    }
    Ok(())
}

fn evaluate(cli: &Cli) -> Result<Summary> {
    let bytes = fs::read(&cli.scene)
        .with_context(|| format!("read scene {}", cli.scene.display()))?;
    let fixture: Fixture = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse scene {}", cli.scene.display()))?;
    if fixture.speakers.is_empty() {
        bail!("scene must contain at least one loudspeaker");
    }
    let block_size = cli.block_size.unwrap_or(fixture.block_size);
    if block_size == 0 {
        bail!("fixture block size must be greater than zero");
    }

    fs::create_dir_all(&cli.output_dir)
        .with_context(|| format!("create {}", cli.output_dir.display()))?;

    let mut speakers = fixture.speakers;
    for speaker in &mut speakers {
        speaker.gain_db = 0.0;
    }

    let output_channels = speakers.iter().filter(|speaker| speaker.enabled).count();
    let lfe_channels = speakers
        .iter()
        .filter(|speaker| speaker.enabled && matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects))
        .count();
    let top_channels = speakers
        .iter()
        .filter(|speaker| speaker.enabled && speaker.channel_role.as_str().starts_with("top-"))
        .count();
    let custom_role_channels = speakers
        .iter()
        .filter(|speaker| speaker.enabled && matches!(speaker.channel_role, ChannelRole::Custom(_)))
        .count();
    let spatial_channels = output_channels.saturating_sub(lfe_channels);
    if lfe_channels != 1 {
        bail!("immersive reference scenes require exactly one LFE channel");
    }
    if spatial_channels < 4 {
        bail!("3D VBAP requires at least four spatial channels");
    }

    let speaker_ids = speakers
        .iter()
        .filter(|speaker| speaker.enabled)
        .map(|speaker| speaker.id.clone())
        .collect::<Vec<_>>();
    let output_roles = speakers
        .iter()
        .filter(|speaker| speaker.enabled)
        .map(|speaker| speaker.channel_role.as_str().to_owned())
        .collect::<Vec<_>>();

    let mut renderer = Vbap3dRenderer::new();
    renderer
        .configure(speakers.clone(), cli.sample_rate, block_size, 1)
        .context("configure 3D VBAP renderer")?;
    renderer
        .prepare_listener(&fixture.listener)
        .context("prepare listener-relative 3D VBAP hull")?;
    let validated_triplets = renderer.validated_triplets().len();
    let listener_inside_hull = renderer.listener_inside_hull();
    if validated_triplets == 0 {
        bail!("3D VBAP produced no validated loudspeaker triplets");
    }

    let mut scratch = RendererScratch::new(
        renderer
            .required_scratch_size()
            .context("query 3D VBAP scratch size")?,
    );
    let total_frames = (cli.duration_seconds * f64::from(cli.sample_rate)).round() as usize;
    if total_frames == 0 {
        bail!("duration rounds to zero frames");
    }
    let blocks = total_frames.div_ceil(block_size);

    let enabled_speakers = speakers
        .iter()
        .filter(|speaker| speaker.enabled)
        .collect::<Vec<_>>();
    let lfe_indices = enabled_speakers
        .iter()
        .enumerate()
        .filter_map(|(index, speaker)| {
            matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects).then_some(index)
        })
        .collect::<Vec<_>>();

    let mut frames = Vec::with_capacity(blocks);
    let mut previous_gains: Option<Vec<f32>> = None;
    let mut observed_max_gain_step = 0.0_f32;
    let mut gain_step_violations = 0_usize;
    let mut normalization_failures = 0_usize;
    let mut non_finite_failures = 0_usize;
    let mut lfe_nonzero_frames = 0_usize;

    for block_index in 0..blocks {
        let start_frame = block_index * block_size;
        let progress = if blocks > 1 {
            block_index as f32 / (blocks - 1) as f32
        } else {
            0.0
        };
        let position = source_position(progress, &fixture.listener);
        let mut rendered = vec![SpeakerGain::default(); output_channels];
        renderer
            .render_gains(
                &fixture.listener,
                &[RenderObject {
                    position,
                    gain: 1.0,
                }],
                &mut rendered,
                &mut scratch,
            )
            .context("render immersive layout validation block")?;

        let gains = rendered.iter().map(|entry| entry.gain).collect::<Vec<_>>();
        if rendered.iter().any(|entry| {
            !entry.gain.is_finite()
                || !entry.distance_meters.is_finite()
                || !entry.delay_samples.is_finite()
        }) {
            non_finite_failures += 1;
        }
        if lfe_indices.iter().any(|index| gains[*index].abs() > 1.0e-7) {
            lfe_nonzero_frames += 1;
        }

        let spatial_power = enabled_speakers
            .iter()
            .zip(gains.iter())
            .filter(|(speaker, _)| {
                !matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects)
            })
            .map(|(_, gain)| gain * gain)
            .sum::<f32>();
        if !spatial_power.is_finite()
            || (spatial_power - 1.0).abs() > cli.normalization_tolerance
        {
            normalization_failures += 1;
        }

        if let Some(previous) = previous_gains.as_deref() {
            for (before, after) in previous.iter().zip(gains.iter()) {
                let step = (after - before).abs();
                observed_max_gain_step = observed_max_gain_step.max(step);
                if step > cli.max_gain_step {
                    gain_step_violations += 1;
                }
            }
        }

        frames.push(GainFrame {
            block_index,
            time_seconds: start_frame as f64 / f64::from(cli.sample_rate),
            source: Position {
                x: position.x,
                y: position.y,
                z: position.z,
            },
            gains: gains.clone(),
            spatial_power_sum_squares: spatial_power,
        });
        previous_gains = Some(gains);
    }

    write_json_artifact(
        cli.output_dir.join("gain-trajectory.json"),
        "immersive-layout-gain-trajectory",
        &frames,
    )?;

    let passed = listener_inside_hull
        && gain_step_violations == 0
        && normalization_failures == 0
        && non_finite_failures == 0
        && lfe_nonzero_frames == 0;
    let summary = Summary {
        passed,
        layout: fixture.layout,
        output_channels,
        spatial_channels,
        lfe_channels,
        top_channels,
        custom_role_channels,
        validated_triplets,
        listener_inside_hull,
        observed_max_gain_step,
        configured_max_gain_step: cli.max_gain_step,
        gain_step_violations,
        normalization_failures,
        non_finite_failures,
        lfe_nonzero_frames,
        frame_count: frames.len(),
        speaker_ids,
        output_roles,
        truth_boundary: "software-only geometry and gain validation; no WAV channel-mask standardization, codec, physical acoustic, proprietary-renderer equivalence, or certification claim",
    };
    write_json_artifact(
        cli.output_dir.join("summary.json"),
        "immersive-layout-summary",
        &summary,
    )?;
    Ok(summary)
}

fn source_position(progress: f32, listener: &Listener) -> Vector3 {
    let angle = TAU * progress;
    let listener_z = listener.position.z + listener.ear_height;
    Vector3::new(
        listener.position.x + 1.1 * angle.cos(),
        listener.position.y + 1.1 * angle.sin(),
        listener_z + 0.65 * (2.0 * angle).sin(),
    )
}

fn write_json_artifact<T: Serialize>(
    path: PathBuf,
    artifact: &'static str,
    payload: &T,
) -> Result<()> {
    let envelope = ArtifactEnvelope {
        schema_version: SCHEMA_VERSION,
        artifact,
        payload,
    };
    let bytes = serde_json::to_vec_pretty(&envelope).context("serialize immersive layout artifact")?;
    fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))
}
