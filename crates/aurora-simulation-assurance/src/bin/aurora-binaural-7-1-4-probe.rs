use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};
use clap::Parser;
use serde::Serialize;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_SIZE: usize = 256;
const EAR_HEIGHT: f32 = 1.2;
const SOURCE_RADIUS: f32 = 2.0;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    output: PathBuf,
}

#[derive(Debug, Serialize)]
struct ProbeOutput {
    schema_version: u32,
    renderer: &'static str,
    sample_rate_hz: u32,
    block_size: usize,
    channel_order: Vec<&'static str>,
    truth_boundary: &'static str,
    cases: Vec<ProbeCase>,
}

#[derive(Debug, Serialize)]
struct ProbeCase {
    channel_index: usize,
    role: &'static str,
    expected_side: &'static str,
    azimuth_degrees: f32,
    elevation_degrees: f32,
    source_position: [f32; 3],
    left_gain: f32,
    right_gain: f32,
    left_delay_samples: f32,
    right_delay_samples: f32,
    left_right_power_bias_db: f32,
    finite: bool,
}

#[derive(Clone, Copy)]
struct CaseSpec {
    role: &'static str,
    expected_side: &'static str,
    azimuth_degrees: f32,
    elevation_degrees: f32,
}

fn output_speaker(id: &str, role: ChannelRole, x: f32) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: id.to_owned(),
        channel_role: role,
        position: Vector3::new(x, 0.0, EAR_HEIGHT),
        orientation: Vector3::ZERO,
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn listener() -> Listener {
    Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: EAR_HEIGHT,
    }
}

fn source_position(azimuth_degrees: f32, elevation_degrees: f32) -> Vector3 {
    let azimuth = azimuth_degrees.to_radians();
    let elevation = elevation_degrees.to_radians();
    let horizontal_radius = SOURCE_RADIUS * elevation.cos();
    Vector3::new(
        horizontal_radius * azimuth.sin(),
        horizontal_radius * azimuth.cos(),
        EAR_HEIGHT + SOURCE_RADIUS * elevation.sin(),
    )
}

fn specs() -> [CaseSpec; 12] {
    [
        CaseSpec {
            role: "FL",
            expected_side: "left",
            azimuth_degrees: -30.0,
            elevation_degrees: 0.0,
        },
        CaseSpec {
            role: "FR",
            expected_side: "right",
            azimuth_degrees: 30.0,
            elevation_degrees: 0.0,
        },
        CaseSpec {
            role: "FC",
            expected_side: "center",
            azimuth_degrees: 0.0,
            elevation_degrees: 0.0,
        },
        CaseSpec {
            role: "LFE",
            expected_side: "lfe",
            azimuth_degrees: 0.0,
            elevation_degrees: 0.0,
        },
        CaseSpec {
            role: "SL",
            expected_side: "left",
            azimuth_degrees: -90.0,
            elevation_degrees: 0.0,
        },
        CaseSpec {
            role: "SR",
            expected_side: "right",
            azimuth_degrees: 90.0,
            elevation_degrees: 0.0,
        },
        CaseSpec {
            role: "SBL",
            expected_side: "left",
            azimuth_degrees: -135.0,
            elevation_degrees: 0.0,
        },
        CaseSpec {
            role: "SBR",
            expected_side: "right",
            azimuth_degrees: 135.0,
            elevation_degrees: 0.0,
        },
        CaseSpec {
            role: "TFL",
            expected_side: "left",
            azimuth_degrees: -45.0,
            elevation_degrees: 45.0,
        },
        CaseSpec {
            role: "TFR",
            expected_side: "right",
            azimuth_degrees: 45.0,
            elevation_degrees: 45.0,
        },
        CaseSpec {
            role: "TRL",
            expected_side: "left",
            azimuth_degrees: -135.0,
            elevation_degrees: 45.0,
        },
        CaseSpec {
            role: "TRR",
            expected_side: "right",
            azimuth_degrees: 135.0,
            elevation_degrees: 45.0,
        },
    ]
}

fn render_case(spec: CaseSpec, channel_index: usize) -> Result<ProbeCase> {
    let mut renderer = BasicRenderer::new(BasicRendererMode::GeometricBinaural);
    renderer
        .configure(
            vec![
                output_speaker("left-ear", ChannelRole::FrontLeft, -0.0875),
                output_speaker("right-ear", ChannelRole::FrontRight, 0.0875),
            ],
            SAMPLE_RATE,
            BLOCK_SIZE,
            1,
        )
        .context("configure geometric binaural renderer")?;

    let mut scratch = RendererScratch::new(renderer.required_scratch_size()?);
    let mut gains = vec![SpeakerGain::default(); 2];
    let position = source_position(spec.azimuth_degrees, spec.elevation_degrees);
    renderer.render_gains(
        &listener(),
        &[RenderObject {
            position,
            gain: 1.0,
        }],
        &mut gains,
        &mut scratch,
    )?;

    let left_power = gains[0].gain * gains[0].gain;
    let right_power = gains[1].gain * gains[1].gain;
    let bias_db = 10.0 * ((left_power + 1.0e-12) / (right_power + 1.0e-12)).log10();
    let finite = gains.iter().all(|gain| {
        gain.gain.is_finite()
            && gain.distance_meters.is_finite()
            && gain.delay_samples.is_finite()
    }) && bias_db.is_finite();

    Ok(ProbeCase {
        channel_index,
        role: spec.role,
        expected_side: spec.expected_side,
        azimuth_degrees: spec.azimuth_degrees,
        elevation_degrees: spec.elevation_degrees,
        source_position: [position.x, position.y, position.z],
        left_gain: gains[0].gain,
        right_gain: gains[1].gain,
        left_delay_samples: gains[0].delay_samples,
        right_delay_samples: gains[1].delay_samples,
        left_right_power_bias_db: bias_db,
        finite,
    })
}

fn main() -> Result<()> {
    let args = Args::parse();
    let specs = specs();
    let mut cases = Vec::with_capacity(specs.len());
    for (channel_index, spec) in specs.into_iter().enumerate() {
        cases.push(render_case(spec, channel_index)?);
    }

    let output = ProbeOutput {
        schema_version: 1,
        renderer: "aurora-renderer-basic/geometric-binaural",
        sample_rate_hz: SAMPLE_RATE,
        block_size: BLOCK_SIZE,
        channel_order: vec![
            "FL", "FR", "FC", "LFE", "SL", "SR", "SBL", "SBR", "TFL", "TFR",
            "TRL", "TRR",
        ],
        truth_boundary: "Aurora geometric binaural probe only: geometric ITD/ILD and relative per-ear distance weighting. No HRTF/HRIR, pinna, elevation, front-back, perceptual, physical-latency, or certification claim.",
        cases,
    };

    let text = serde_json::to_string_pretty(&output)? + "\n";
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, text)?;
    Ok(())
}
