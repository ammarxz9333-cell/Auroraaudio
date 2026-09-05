use std::f32::consts::TAU;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::Vbap3dRenderer;
use clap::Parser;
use serde::{Deserialize, Serialize};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Parser)]
#[command(name = "aurora-evaluate-vbap3d")]
#[command(about = "Generate deterministic offline 3D VBAP evidence for the canonical 5.1.2 scene")]
struct Cli {
    #[arg(long, default_value = "fixtures/scenes/5_1_2_upfiring.json")]
    scene: PathBuf,
    #[arg(long, default_value = "output/evaluation/vbap3d-5.1.2")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 2.0)]
    duration_seconds: f64,
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,
    #[arg(long)]
    block_size: Option<usize>,
    #[arg(long, default_value_t = 0.15)]
    max_gain_step: f32,
    #[arg(long, default_value_t = 0.02)]
    normalization_tolerance: f32,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    listener: Listener,
    speakers: Vec<Speaker>,
    block_size: usize,
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
    validated_triplets: usize,
    listener_inside_hull: bool,
    scene_path: String,
    commit_sha: String,
    scene_semantics: &'static str,
    trim_semantics: &'static str,
    wav_semantics: &'static str,
    evidence_boundary: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct Summary {
    passed: bool,
    validated_triplets: usize,
    listener_inside_hull: bool,
    observed_max_gain_step: f32,
    configured_max_gain_step: f32,
    gain_step_violations: usize,
    normalization_failures: usize,
    non_finite_failures: usize,
    artifacts: Vec<String>,
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
    let summary = evaluate(&cli, &reproducible_command())?;
    println!(
        "renderer=aurora-vbap3d passed={} triplets={} closed_hull={} max_gain_step={:.6} gain_step_violations={} normalization_failures={} non_finite_failures={}",
        summary.passed,
        summary.validated_triplets,
        summary.listener_inside_hull,
        summary.observed_max_gain_step,
        summary.gain_step_violations,
        summary.normalization_failures,
        summary.non_finite_failures,
    );
    if !summary.passed {
        bail!("3D VBAP evaluation failed one or more software evidence gates");
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

fn evaluate(cli: &Cli, command: &str) -> Result<Summary> {
    let fixture_bytes = fs::read(&cli.scene)
        .with_context(|| format!("read scene {}", cli.scene.display()))?;
    let fixture: Fixture = serde_json::from_slice(&fixture_bytes)
        .with_context(|| format!("parse scene {}", cli.scene.display()))?;
    if fixture.speakers.is_empty() {
        bail!("evaluation scene must contain at least one loudspeaker");
    }
    let block_size = cli.block_size.unwrap_or(fixture.block_size);
    if block_size == 0 {
        bail!("fixture block size must be greater than zero");
    }

    fs::create_dir_all(&cli.output_dir)
        .with_context(|| format!("create {}", cli.output_dir.display()))?;

    // Preserve canonical positions and channel roles while neutralizing fixture
    // installation trims so unit-power evidence measures the renderer itself.
    let mut speakers = fixture.speakers;
    for speaker in &mut speakers {
        speaker.gain_db = 0.0;
    }
    validate_wav_roles(&speakers)?;

    let roles = speakers
        .iter()
        .map(|speaker| speaker.channel_role.clone())
        .collect::<Vec<_>>();
    let speaker_ids = speakers
        .iter()
        .map(|speaker| speaker.id.clone())
        .collect::<Vec<_>>();
    let output_roles = roles
        .iter()
        .map(|role| role.as_str().to_owned())
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

    let output_channels = renderer.output_channel_count();
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
        let start_frame = block_index * block_size;
        let frames = (total_frames - start_frame).min(block_size);
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

        let source = Position {
            x: position.x,
            y: position.y,
            z: position.z,
        };
        let time_seconds = start_frame as f64 / f64::from(cli.sample_rate);
        gain_frames.push(GainFrame {
            block_index,
            time_seconds,
            source: source.clone(),
            gains: gains.clone(),
            spatial_power_sum_squares: spatial_power,
        });
        delay_frames.push(DelayFrame {
            block_index,
            time_seconds,
            source,
            delay_samples: delays,
        });

        for local_frame in 0..frames {
            let frame = start_frame + local_frame;
            let phase = TAU * 440.0 * frame as f32 / cli.sample_rate as f32;
            let sample = 0.1 * phase.sin();
            for (channel, gain) in pcm.iter_mut().zip(gains.iter()) {
                channel.push(sample * *gain);
            }
        }
        previous_gains = Some(gains);
    }

    write_float32_extensible_wav(
        &cli.output_dir.join("rendered-reference.wav"),
        cli.sample_rate,
        &roles,
        &pcm,
    )?;
    write_json_artifact(
        cli.output_dir.join("gain-trajectory.json"),
        "vbap3d-gain-trajectory",
        &gain_frames,
    )?;
    write_json_artifact(
        cli.output_dir.join("delay-trajectory.json"),
        "vbap3d-delay-trajectory",
        &delay_frames,
    )?;

    let metadata = Metadata {
        renderer: "aurora-vbap3d",
        sample_rate: cli.sample_rate,
        block_size,
        output_channels,
        speaker_ids,
        output_roles,
        validated_triplets,
        listener_inside_hull,
        scene_path: cli.scene.display().to_string(),
        commit_sha: std::env::var("GITHUB_SHA").unwrap_or_else(|_| "unknown".to_owned()),
        scene_semantics: "canonical Aurora 5.1.2 fixture; listener and speaker geometry loaded from JSON",
        trim_semantics: "speaker gain_db normalized to 0 dB for renderer-only unit-power evidence",
        wav_semantics: "IEEE-float WAVE_FORMAT_EXTENSIBLE; 440 Hz mono reference routed by block gains; propagation delays are not applied to PCM",
        evidence_boundary: "software-only; no Atmos/JOC, HRTF, physical hardware, acoustic, or product-readiness claim",
    };
    write_json_artifact(
        cli.output_dir.join("metadata.json"),
        "vbap3d-metadata",
        &metadata,
    )?;
    fs::write(cli.output_dir.join("command.txt"), format!("{command}\n"))
        .context("write reproducible 3D VBAP command")?;

    let passed = listener_inside_hull
        && gain_step_violations == 0
        && normalization_failures == 0
        && non_finite_failures == 0;
    let summary = Summary {
        passed,
        validated_triplets,
        listener_inside_hull,
        observed_max_gain_step,
        configured_max_gain_step: cli.max_gain_step,
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
    write_json_artifact(
        cli.output_dir.join("summary.json"),
        "vbap3d-summary",
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

fn validate_wav_roles(speakers: &[Speaker]) -> Result<()> {
    let mut previous_bit = 0_u32;
    for speaker in speakers {
        let bit = speaker
            .channel_role
            .wav_channel_mask_bit()
            .with_context(|| format!("WAV evidence requires a standard channel role: {}", speaker.id))?;
        if bit <= previous_bit {
            bail!(
                "fixture channel order is not WAVE_FORMAT_EXTENSIBLE mask order at {}",
                speaker.id
            );
        }
        previous_bit = bit;
    }
    Ok(())
}

fn write_float32_extensible_wav(
    path: &Path,
    sample_rate: u32,
    roles: &[ChannelRole],
    pcm: &[Vec<f32>],
) -> Result<()> {
    if pcm.is_empty() || pcm.len() != roles.len() {
        bail!("WAV channel and role counts must match and be non-zero");
    }
    let frames = pcm[0].len();
    if pcm.iter().any(|channel| channel.len() != frames) {
        bail!("all WAV channels must contain the same number of frames");
    }
    let channels = u16::try_from(pcm.len()).context("too many WAV channels")?;
    let block_align = channels
        .checked_mul(4)
        .context("WAV block alignment overflow")?;
    let byte_rate = sample_rate
        .checked_mul(u32::from(block_align))
        .context("WAV byte rate overflow")?;
    let data_bytes = frames
        .checked_mul(usize::from(block_align))
        .context("WAV data size overflow")?;
    let data_bytes_u32 = u32::try_from(data_bytes).context("WAV data exceeds RIFF limit")?;
    let riff_size = 60_u32
        .checked_add(data_bytes_u32)
        .context("WAV RIFF size overflow")?;
    let channel_mask = roles.iter().try_fold(0_u32, |mask, role| {
        role.wav_channel_mask_bit()
            .map(|bit| mask | bit)
            .context("WAV evidence requires standard channel roles")
    })?;

    let file = File::create(path).with_context(|| format!("create {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    writer.write_all(b"RIFF")?;
    writer.write_all(&riff_size.to_le_bytes())?;
    writer.write_all(b"WAVE")?;
    writer.write_all(b"fmt ")?;
    writer.write_all(&40_u32.to_le_bytes())?;
    writer.write_all(&0xfffe_u16.to_le_bytes())?;
    writer.write_all(&channels.to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&32_u16.to_le_bytes())?;
    writer.write_all(&22_u16.to_le_bytes())?;
    writer.write_all(&32_u16.to_le_bytes())?;
    writer.write_all(&channel_mask.to_le_bytes())?;
    writer.write_all(&3_u32.to_le_bytes())?;
    writer.write_all(&0_u16.to_le_bytes())?;
    writer.write_all(&0x0010_u16.to_le_bytes())?;
    writer.write_all(&[0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71])?;
    writer.write_all(b"data")?;
    writer.write_all(&data_bytes_u32.to_le_bytes())?;
    for frame in 0..frames {
        for channel in pcm {
            writer.write_all(&channel[frame].to_le_bytes())?;
        }
    }
    writer.flush()?;
    Ok(())
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
    let bytes = serde_json::to_vec_pretty(&envelope).context("serialize 3D VBAP artifact")?;
    fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))
}

fn reproducible_command() -> String {
    std::env::args()
        .map(|argument| {
            if argument.chars().all(|character| {
                character.is_ascii_alphanumeric() || "-._/:=\\".contains(character)
            }) {
                argument
            } else {
                format!(
                    "\"{}\"",
                    argument.replace('\\', "\\\\").replace('"', "\\\"")
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trajectory_exercises_height_above_and_below_listener() {
        let listener = Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 1.2,
        };
        let high = source_position(0.125, &listener);
        let low = source_position(0.375, &listener);
        assert!(high.z > listener.ear_height);
        assert!(low.z < listener.ear_height);
    }

    #[test]
    fn canonical_roles_are_strictly_mask_ordered() {
        let roles = [
            ChannelRole::FrontLeft,
            ChannelRole::FrontRight,
            ChannelRole::FrontCenter,
            ChannelRole::LowFrequencyEffects,
            ChannelRole::SurroundLeft,
            ChannelRole::SurroundRight,
            ChannelRole::TopFrontLeft,
            ChannelRole::TopFrontRight,
        ];
        let bits = roles
            .iter()
            .map(|role| role.wav_channel_mask_bit().unwrap())
            .collect::<Vec<_>>();
        assert!(bits.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
