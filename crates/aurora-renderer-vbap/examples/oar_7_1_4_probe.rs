use std::error::Error;

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::Vbap3dRenderer;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_SIZE: usize = 256;
const CHANNELS: usize = 12;

fn iamf_polar_to_aurora(azimuth_degrees: f32, elevation_degrees: f32, distance: f32) -> Vector3 {
    let azimuth = azimuth_degrees.to_radians();
    let elevation = elevation_degrees.to_radians();
    let horizontal = distance * elevation.cos();
    // IAMF/OAR positive azimuth points to the listener's left. Aurora faces +Y.
    Vector3::new(
        -horizontal * azimuth.sin(),
        horizontal * azimuth.cos(),
        distance * elevation.sin(),
    )
}

fn speaker(
    id: &str,
    label: &str,
    role: ChannelRole,
    azimuth_degrees: f32,
    elevation_degrees: f32,
) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: label.to_owned(),
        channel_role: role,
        position: iamf_polar_to_aurora(azimuth_degrees, elevation_degrees, 1.0),
        orientation: Vector3::new(0.0, -1.0, 0.0),
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn layout() -> Vec<Speaker> {
    vec![
        speaker("fl", "Front Left", ChannelRole::FrontLeft, 30.0, 0.0),
        speaker("fr", "Front Right", ChannelRole::FrontRight, -30.0, 0.0),
        speaker("fc", "Front Center", ChannelRole::FrontCenter, 0.0, 0.0),
        // OAR's 7.1.4 reference places LFE1 at +45/-30. Aurora's 3D VBAP excludes
        // LFE from spatial panning by semantic channel role, which is what this probe verifies.
        speaker(
            "lfe",
            "LFE",
            ChannelRole::LowFrequencyEffects,
            45.0,
            -30.0,
        ),
        speaker("sl", "Side Left", ChannelRole::SurroundLeft, 90.0, 0.0),
        speaker("sr", "Side Right", ChannelRole::SurroundRight, -90.0, 0.0),
        speaker(
            "sbl",
            "Rear Left",
            ChannelRole::SurroundBackLeft,
            135.0,
            0.0,
        ),
        speaker(
            "sbr",
            "Rear Right",
            ChannelRole::SurroundBackRight,
            -135.0,
            0.0,
        ),
        speaker(
            "tfl",
            "Top Front Left",
            ChannelRole::TopFrontLeft,
            45.0,
            30.0,
        ),
        speaker(
            "tfr",
            "Top Front Right",
            ChannelRole::TopFrontRight,
            -45.0,
            30.0,
        ),
        speaker(
            "trl",
            "Top Rear Left",
            ChannelRole::TopRearLeft,
            135.0,
            30.0,
        ),
        speaker(
            "trr",
            "Top Rear Right",
            ChannelRole::TopRearRight,
            -135.0,
            30.0,
        ),
    ]
}

fn linear_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn probe(
    renderer: &mut Vbap3dRenderer,
    listener: &Listener,
    scratch: &mut RendererScratch,
    azimuth_degrees: f32,
    elevation_degrees: f32,
    gain_db: f32,
) -> Result<[f32; CHANNELS], Box<dyn Error>> {
    renderer.reset();
    let object = RenderObject {
        position: iamf_polar_to_aurora(azimuth_degrees, elevation_degrees, 1.0),
        gain: linear_gain(gain_db),
    };
    let mut output = [SpeakerGain::default(); CHANNELS];
    renderer.render_gains(listener, &[object], &mut output, scratch)?;
    let mut gains = [0.0_f32; CHANNELS];
    for item in output {
        gains[item.speaker_index] = item.gain;
    }
    Ok(gains)
}

fn main() -> Result<(), Box<dyn Error>> {
    let listener = Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        // The OAR reference coordinates are listener-centric. Zero ear-height
        // makes the acoustic origin identical for this differential fixture.
        ear_height: 0.0,
    };
    let mut renderer = Vbap3dRenderer::new().with_smoothing(1.0);
    renderer.configure(layout(), SAMPLE_RATE, BLOCK_SIZE, 1)?;
    renderer.prepare_listener(&listener)?;
    let mut scratch = RendererScratch::new(renderer.required_scratch_size()?);

    println!(
        "AURORA714-DIFF-PROBE schema=1 layout=7.1.4 channels={CHANNELS} samples_per_channel={BLOCK_SIZE} sample_rate={SAMPLE_RATE} inside_hull={}",
        renderer.listener_inside_hull()
    );

    let probes = [
        ("FL", 30.0_f32, 0.0_f32, 0.0_f32),
        ("FC", 0.0, 0.0, 0.0),
        ("FR", -30.0, 0.0, 0.0),
        ("SL", 90.0, 0.0, 0.0),
        ("SR", -90.0, 0.0, 0.0),
        ("SBL", 135.0, 0.0, 0.0),
        ("SBR", -135.0, 0.0, 0.0),
        ("TFL", 45.0, 30.0, 0.0),
        ("TFR", -45.0, 30.0, 0.0),
        ("TRL", 135.0, 30.0, 0.0),
        ("TRR", -135.0, 30.0, 0.0),
        ("TOPC", 0.0, 30.0, 0.0),
        ("MIDC", 0.0, 15.0, 0.0),
        ("FCM6", 0.0, 0.0, -6.0),
    ];

    for (name, azimuth, elevation, gain_db) in probes {
        let gains = probe(
            &mut renderer,
            &listener,
            &mut scratch,
            azimuth,
            elevation,
            gain_db,
        )?;
        print!(
            "AURORA714-PROBE name={name} azimuth={azimuth:.3} elevation={elevation:.3} gain_db={gain_db:.3}"
        );
        for (index, gain) in gains.iter().enumerate() {
            print!(" ch{index}={gain:.9}");
        }
        println!();
    }
    Ok(())
}
