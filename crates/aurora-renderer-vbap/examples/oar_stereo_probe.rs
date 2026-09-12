use std::error::Error;

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::VbapRenderer;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_SIZE: usize = 256;

fn iamf_polar_to_aurora(azimuth_degrees: f32, distance: f32) -> Vector3 {
    // OAR/IAMF layouts use positive azimuth to the listener's left and
    // negative azimuth to the right. Aurora uses Cartesian coordinates, so a
    // forward-facing listener maps polar azimuth to x=-sin(a), y=cos(a).
    let radians = azimuth_degrees.to_radians();
    Vector3::new(-distance * radians.sin(), distance * radians.cos(), 0.0)
}

fn speaker(id: &str, label: &str, role: ChannelRole, azimuth_degrees: f32) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: label.to_owned(),
        channel_role: role,
        position: iamf_polar_to_aurora(azimuth_degrees, 1.0),
        orientation: Vector3::new(0.0, -1.0, 0.0),
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn linear_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn probe(
    renderer: &mut VbapRenderer,
    listener: &Listener,
    scratch: &mut RendererScratch,
    azimuth_degrees: f32,
    gain_db: f32,
) -> Result<[f32; 2], Box<dyn Error>> {
    renderer.reset();
    let object = RenderObject {
        position: iamf_polar_to_aurora(azimuth_degrees, 1.0),
        gain: linear_gain(gain_db),
    };
    let mut output = [SpeakerGain::default(); 2];
    renderer.render_gains(listener, &[object], &mut output, scratch)?;
    Ok([output[0].gain, output[1].gain])
}

fn main() -> Result<(), Box<dyn Error>> {
    let listener = Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 1.2,
    };
    let layout = vec![
        speaker("left", "Left", ChannelRole::FrontLeft, 30.0),
        speaker("right", "Right", ChannelRole::FrontRight, -30.0),
    ];

    let mut renderer = VbapRenderer::new().with_smoothing(1.0);
    renderer.configure(layout, SAMPLE_RATE, BLOCK_SIZE, 1)?;
    let mut scratch = RendererScratch::new(renderer.required_scratch_size()?);

    println!("AURORA-OAR-DIFF-PROBE schema=1 layout=stereo samples_per_channel={BLOCK_SIZE} sample_rate={SAMPLE_RATE}");
    for &(azimuth, gain_db) in &[(30.0_f32, 0.0_f32), (0.0, 0.0), (-30.0, 0.0), (0.0, -6.0)] {
        let [left, right] = probe(&mut renderer, &listener, &mut scratch, azimuth, gain_db)?;
        println!(
            "AURORA-PROBE azimuth={azimuth:.3} gain_db={gain_db:.3} left={left:.9} right={right:.9}"
        );
    }
    Ok(())
}
