use std::error::Error;
use std::fs;
use std::path::PathBuf;

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_SIZE: usize = 256;
const EAR_HEIGHT: f32 = 1.2;
const SOURCE_RADIUS: f32 = 2.0;

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
        CaseSpec { role: "FL", expected_side: "left", azimuth_degrees: -30.0, elevation_degrees: 0.0 },
        CaseSpec { role: "FR", expected_side: "right", azimuth_degrees: 30.0, elevation_degrees: 0.0 },
        CaseSpec { role: "FC", expected_side: "center", azimuth_degrees: 0.0, elevation_degrees: 0.0 },
        CaseSpec { role: "LFE", expected_side: "lfe", azimuth_degrees: 0.0, elevation_degrees: 0.0 },
        CaseSpec { role: "SL", expected_side: "left", azimuth_degrees: -90.0, elevation_degrees: 0.0 },
        CaseSpec { role: "SR", expected_side: "right", azimuth_degrees: 90.0, elevation_degrees: 0.0 },
        CaseSpec { role: "SBL", expected_side: "left", azimuth_degrees: -135.0, elevation_degrees: 0.0 },
        CaseSpec { role: "SBR", expected_side: "right", azimuth_degrees: 135.0, elevation_degrees: 0.0 },
        CaseSpec { role: "TFL", expected_side: "left", azimuth_degrees: -45.0, elevation_degrees: 45.0 },
        CaseSpec { role: "TFR", expected_side: "right", azimuth_degrees: 45.0, elevation_degrees: 45.0 },
        CaseSpec { role: "TRL", expected_side: "left", azimuth_degrees: -135.0, elevation_degrees: 45.0 },
        CaseSpec { role: "TRR", expected_side: "right", azimuth_degrees: 135.0, elevation_degrees: 45.0 },
    ]
}

fn main() -> Result<(), Box<dyn Error>> {
    let output_path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: binaural_7_1_4_probe <output.json>")?;

    let mut text = String::from(
        "{\n  \"schema_version\": 1,\n  \"renderer\": \"aurora-renderer-basic/geometric-binaural\",\n  \"sample_rate_hz\": 48000,\n  \"block_size\": 256,\n  \"channel_order\": [\"FL\", \"FR\", \"FC\", \"LFE\", \"SL\", \"SR\", \"SBL\", \"SBR\", \"TFL\", \"TFR\", \"TRL\", \"TRR\"],\n  \"truth_boundary\": \"Aurora geometric binaural probe only: geometric ITD/ILD and relative per-ear distance weighting. No HRTF/HRIR, pinna, elevation, front-back, perceptual, physical-latency, or certification claim.\",\n  \"cases\": [\n",
    );

    for (channel_index, spec) in specs().into_iter().enumerate() {
        let mut renderer = BasicRenderer::new(BasicRendererMode::GeometricBinaural);
        renderer.configure(
            vec![
                output_speaker("left-ear", ChannelRole::FrontLeft, -0.0875),
                output_speaker("right-ear", ChannelRole::FrontRight, 0.0875),
            ],
            SAMPLE_RATE,
            BLOCK_SIZE,
            1,
        )?;
        let mut scratch = RendererScratch::new(renderer.required_scratch_size()?);
        let mut gains = vec![SpeakerGain::default(); 2];
        let position = source_position(spec.azimuth_degrees, spec.elevation_degrees);
        renderer.render_gains(
            &listener(),
            &[RenderObject { position, gain: 1.0 }],
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

        let comma = if channel_index + 1 == specs().len() { "" } else { "," };
        text.push_str(&format!(
            "    {{\"channel_index\": {channel_index}, \"role\": \"{}\", \"expected_side\": \"{}\", \"azimuth_degrees\": {}, \"elevation_degrees\": {}, \"source_position\": [{}, {}, {}], \"left_gain\": {}, \"right_gain\": {}, \"left_delay_samples\": {}, \"right_delay_samples\": {}, \"left_right_power_bias_db\": {}, \"finite\": {finite}}}{comma}\n",
            spec.role,
            spec.expected_side,
            spec.azimuth_degrees,
            spec.elevation_degrees,
            position.x,
            position.y,
            position.z,
            gains[0].gain,
            gains[1].gain,
            gains[0].delay_samples,
            gains[1].delay_samples,
            bias_db,
        ));
    }
    text.push_str("  ]\n}\n");

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, text)?;
    Ok(())
}
