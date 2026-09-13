use std::f32::consts::PI;
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::VbapRenderer;
use clap::Parser;
use serde::Serialize;

const SAMPLE_RATE: u32 = 48_000;
const FRAMES: usize = 256;
const FREQUENCY_HZ: f32 = 440.0;

#[derive(Debug, Parser)]
#[command(name = "aurora-oar-differential-probe")]
#[command(
    about = "Emit deterministic Aurora stereo semantics for the pinned OAR differential lane"
)]
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
    channel_order: [&'static str; 2],
    coordinate_convention: &'static str,
    cases: Vec<ProbeCase>,
}

#[derive(Debug, Serialize)]
struct ProbeCase {
    name: &'static str,
    semantic_position: &'static str,
    azimuth_degrees: f32,
    gain_db: f32,
    frame_count: usize,
    channel_levels: [f64; 2],
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
    let encoded = serde_json::to_vec_pretty(&report).context("failed to encode probe report")?;
    fs::write(&args.output, encoded)
        .with_context(|| format!("failed to write {}", args.output.display()))?;
    println!("AURORA-OAR-DIFFERENTIAL-PROBE-PASS");
    println!("report={}", args.output.display());
    Ok(())
}

fn run_probe() -> Result<ProbeReport> {
    let specs = [
        CaseSpec {
            name: "left_unity",
            semantic_position: "left",
            azimuth_degrees: -45.0,
            gain_db: 0.0,
        },
        CaseSpec {
            name: "center_unity",
            semantic_position: "center",
            azimuth_degrees: 0.0,
            gain_db: 0.0,
        },
        CaseSpec {
            name: "right_unity",
            semantic_position: "right",
            azimuth_degrees: 45.0,
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
        implementation: "aurora-vbap2d",
        layout: "stereo",
        sample_rate: SAMPLE_RATE,
        frames_per_case: FRAMES,
        channel_order: ["FL", "FR"],
        coordinate_convention: "negative-azimuth-left",
        cases,
    })
}

fn render_case(spec: CaseSpec) -> Result<ProbeCase> {
    let mut renderer = VbapRenderer::new();
    renderer
        .configure(stereo_speakers(), SAMPLE_RATE, FRAMES, 1)
        .context("failed to configure Aurora stereo renderer")?;
    if renderer.output_channel_count() != 2 {
        bail!(
            "Aurora stereo renderer returned {} channels",
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
    let mut gains = [SpeakerGain::default(), SpeakerGain::default()];
    let mut scratch = RendererScratch::new(
        renderer
            .required_scratch_size()
            .context("failed to obtain renderer scratch size")?,
    );
    renderer
        .render_gains(&listener, &[object], &mut gains, &mut scratch)
        .context("Aurora stereo gain render failed")?;

    if gains[0].speaker_index != 0 || gains[1].speaker_index != 1 {
        bail!(
            "unexpected Aurora stereo channel order: [{}, {}]",
            gains[0].speaker_index,
            gains[1].speaker_index
        );
    }

    let finite = gains.iter().all(|gain| {
        gain.gain.is_finite() && gain.distance_meters.is_finite() && gain.delay_samples.is_finite()
    });
    let channel_levels = [rms_for_gain(gains[0].gain), rms_for_gain(gains[1].gain)];
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

fn stereo_speakers() -> Vec<Speaker> {
    vec![
        Speaker {
            id: "FL".to_owned(),
            label: "Front Left".to_owned(),
            channel_role: ChannelRole::FrontLeft,
            position: vector_from_azimuth(-30.0),
            orientation: Vector3::ZERO,
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        },
        Speaker {
            id: "FR".to_owned(),
            label: "Front Right".to_owned(),
            channel_role: ChannelRole::FrontRight,
            position: vector_from_azimuth(30.0),
            orientation: Vector3::ZERO,
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        },
    ]
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
