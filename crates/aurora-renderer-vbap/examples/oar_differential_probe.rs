use std::{env, fs::File, io::{BufWriter, Write}, path::PathBuf};

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::VbapRenderer;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_SIZE: usize = 256;
const AZIMUTHS_DEGREES: [f32; 7] = [-60.0, -30.0, -15.0, 0.0, 15.0, 30.0, 60.0];

fn speaker(id: &str, role: ChannelRole, x: f32, y: f32) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: id.to_owned(),
        channel_role: role,
        position: Vector3::new(x, y, 0.0),
        orientation: Vector3::new(0.0, -1.0, 0.0),
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: oar_differential_probe OUTPUT_TSV")?;

    let half = 0.5_f32;
    let front = (3.0_f32).sqrt() * 0.5;
    let layout = vec![
        speaker("M+030-left", ChannelRole::FrontLeft, -half, front),
        speaker("M-030-right", ChannelRole::FrontRight, half, front),
    ];

    let listener = Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 0.0,
    };

    let mut renderer = VbapRenderer::new();
    renderer.configure(layout, SAMPLE_RATE, BLOCK_SIZE, 1)?;
    let mut scratch = RendererScratch::new(renderer.required_scratch_size()?);

    let file = File::create(&output_path)?;
    let mut output = BufWriter::new(file);
    writeln!(output, "AURORA_DIFF_V1")?;
    writeln!(output, "azimuth_degrees\tleft_power_share\tright_power_share")?;

    for azimuth in AZIMUTHS_DEGREES {
        let radians = azimuth.to_radians();
        let object = RenderObject {
            // OAR polar convention maps +azimuth toward listener-left.
            position: Vector3::new(-radians.sin(), radians.cos(), 0.0),
            gain: 1.0,
        };
        let mut gains = [SpeakerGain::default(); 2];
        renderer.render_gains(&listener, &[object], &mut gains, &mut scratch)?;

        let left = f64::from(gains[0].gain) * f64::from(gains[0].gain);
        let right = f64::from(gains[1].gain) * f64::from(gains[1].gain);
        let total = left + right;
        if !left.is_finite() || !right.is_finite() || !total.is_finite() || total <= 0.0 {
            return Err(format!("non-finite or silent Aurora VBAP result at azimuth {azimuth}").into());
        }

        writeln!(
            output,
            "{azimuth:.1}\t{:.9}\t{:.9}",
            left / total,
            right / total
        )?;
    }
    output.flush()?;

    println!(
        "AURORA-OAR-DIFFERENTIAL-PROBE-PASS cases={} output={}",
        AZIMUTHS_DEGREES.len(),
        output_path.display()
    );
    Ok(())
}
