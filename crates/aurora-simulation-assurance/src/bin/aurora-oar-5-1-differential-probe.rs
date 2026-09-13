use std::f32::consts::PI;
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::ObjectVbapRenderer;
use clap::Parser;
use serde::Serialize;

const SAMPLE_RATE: u32 = 48_000;
const FRAMES: usize = 256;
const FREQUENCY_HZ: f32 = 440.0;
const CHANNELS: usize = 6;

#[derive(Debug, Parser)]
#[command(name = "aurora-oar-5-1-differential-probe")]
#[command(about = "Emit deterministic Aurora 5.1 object semantics for pinned OAR comparison")]
struct Args {
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Serialize)]
struct ProbeReport {
    schema_version: u32,
    implementation: &'static str,
    layout: &'static str,
    sample_rate: u32,
    frames_per_case: usize,
    channel_order: [&'static str; CHANNELS],
    coordinate_convention: &'static str,
    lfe_semantics: &'static str,
    cases: Vec<ProbeCase>,
}

#[derive(Debug, Serialize)]
struct ProbeCase {
    name: &'static str,
    semantic_position: &'static str,
    azimuth_degrees: f32,
    gain_db: f32,
    frame_count: usize,
    channel_levels: [f64; CHANNELS],
    finite: bool,
}

#[derive(Debug, Clone, Copy)]
struct CaseSpec {
    name: &'static str,
    semantic_position: &'static str,
    azimuth_degrees: f32,
    gain_db: f32,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let report = run_probe()?;
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(
        &args.output,
        serde_json::to_vec_pretty(&report).context("failed to encode 5.1 probe report")?,
    )
    .with_context(|| format!("failed to write {}", args.output.display()))?;
    println!("AURORA-OAR-5-1-DIFFERENTIAL-PROBE-PASS");
    println!("report={}", args.output.display());
    Ok(())
}

fn run_probe() -> Result<ProbeReport> {
    let specs = [
        CaseSpec {
            name: "front_left_unity",
            semantic_position: "front_left",
            azimuth_degrees: -30.0,
            gain_db: 0.0,
        },
        CaseSpec {
            name: "center_unity",
            semantic_position: "center",
            azimuth_degrees: 0.0,
            gain_db: 0.0,
        },
        CaseSpec {
            name: "front_right_unity",
            semantic_position: "front_right",
            azimuth_degrees: 30.0,
            gain_db: 0.0,
        },
        CaseSpec {
            name: "surround_left_unity",
            semantic_position: "surround_left",
            azimuth_degrees: -110.0,
            gain_db: 0.0,
        },
        CaseSpec {
            name: "surround_right_unity",
            semantic_position: "surround_right",
            azimuth_degrees: 110.0,
            gain_db: 0.0,
        },
        CaseSpec {
            name: "center_minus_6db",
            semantic_position: "center",
            azimuth_degrees: 0.0,
            gain_db: -6.0,
        },
    ];

    let mut cases = Vec::with_capacity(specs.len());
    for spec in specs {
        cases.push(render_case(spec)?);
    }

    Ok(ProbeReport {
        schema_version: 1,
        implementation: "aurora-object-vbap2d",
        layout: "5.1",
        sample_rate: SAMPLE_RATE,
        frames_per_case: FRAMES,
        channel_order: ["FL", "FR", "FC", "LFE", "SL", "SR"],
        coordinate_convention: "negative-azimuth-left",
        lfe_semantics: "non-directional-zero-for-object-render",
        cases,
    })
}

fn render_case(spec: CaseSpec) -> Result<ProbeCase> {
    let mut renderer = ObjectVbapRenderer::new();
    renderer
        .configure(five_one_speakers(), SAMPLE_RATE, FRAMES, 1)
        .context("failed to configure Aurora object-safe 5.1 renderer")?;
    if renderer.output_channel_count() != CHANNELS {
        bail!(
            "Aurora 5.1 renderer returned {} channels",
            renderer.output_channel_count()
        );
    }

    let listener = Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(1.0, 0.0, 0.0),
        ear_height: 0.0,
    };
    let object = RenderObject {
        position: vector_from_azimuth(spec.azimuth_degrees),
        gain: db_to_linear(spec.gain_db),
    };
    let mut gains = [SpeakerGain::default(); CHANNELS];
    let mut scratch = RendererScratch::new(
        renderer
            .required_scratch_size()
            .context("failed to obtain Aurora 5.1 scratch size")?,
    );
    renderer
        .render_gains(&listener, &[object], &mut gains, &mut scratch)
        .context("Aurora 5.1 object render failed")?;

    for (index, gain) in gains.iter().enumerate() {
        if gain.speaker_index != index {
            bail!(
                "unexpected Aurora 5.1 channel order at {index}: {}",
                gain.speaker_index
            );
        }
    }

    let finite = gains.iter().all(|gain| {
        gain.gain.is_finite() && gain.distance_meters.is_finite() && gain.delay_samples.is_finite()
    });
    let channel_levels = std::array::from_fn(|index| rms_for_gain(gains[index].gain));
    let finite = finite && channel_levels.iter().all(|level| level.is_finite());

    Ok(ProbeCase {
        name: spec.name,
        semantic_position: spec.semantic_position,
        azimuth_degrees: spec.azimuth_degrees,
        gain_db: spec.gain_db,
        frame_count: FRAMES,
        channel_levels,
        finite,
    })
}

fn five_one_speakers() -> Vec<Speaker> {
    vec![
        speaker("FL", "Front Left", ChannelRole::FrontLeft, -30.0),
        speaker("FR", "Front Right", ChannelRole::FrontRight, 30.0),
        speaker("FC", "Front Center", ChannelRole::FrontCenter, 0.0),
        speaker(
            "LFE",
            "Low Frequency Effects",
            ChannelRole::LowFrequencyEffects,
            0.0,
        ),
        speaker("SL", "Surround Left", ChannelRole::SurroundLeft, -110.0),
        speaker("SR", "Surround Right", ChannelRole::SurroundRight, 110.0),
    ]
}

fn speaker(id: &str, label: &str, role: ChannelRole, azimuth_degrees: f32) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: label.to_owned(),
        channel_role: role,
        position: vector_from_azimuth(azimuth_degrees),
        orientation: Vector3::ZERO,
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn vector_from_azimuth(degrees: f32) -> Vector3 {
    let radians = degrees * PI / 180.0;
    Vector3::new(radians.cos(), radians.sin(), 0.0)
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn rms_for_gain(gain: f32) -> f64 {
    let mut sum_squares = 0.0_f64;
    for frame in 0..FRAMES {
        let phase = 2.0 * PI * FREQUENCY_HZ * frame as f32 / SAMPLE_RATE as f32;
        let sample = phase.sin() * gain;
        sum_squares += f64::from(sample) * f64::from(sample);
    }
    (sum_squares / FRAMES as f64).sqrt()
}
