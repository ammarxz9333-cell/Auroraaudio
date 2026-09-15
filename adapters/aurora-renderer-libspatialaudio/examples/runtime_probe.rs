use std::env;

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{
    ObjectPcmBlock, ObjectPcmRenderer, RenderObject, Renderer, RendererScratch, SpeakerGain,
};
use aurora_renderer_libspatialaudio::{LibspatialaudioRenderer, LibspatialaudioRuntimeConfig};
use aurora_renderer_vbap::VbapRenderer;
use aurora_test_alloc::{count_allocations, CountingAllocator};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 256;
const CHANNELS: usize = 12;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let shim = env::var("AURORA_LIBSPATIALAUDIO_SHIM")?;
    let layout = canonical_layout();

    let mut renderer = LibspatialaudioRenderer::load(LibspatialaudioRuntimeConfig::new(shim))?;
    renderer.configure(layout.clone(), SAMPLE_RATE, BLOCK_FRAMES, 1)?;
    assert_eq!(renderer.output_channel_count(), CHANNELS);
    assert_eq!(renderer.latency_frames(), 255);

    let mut vbap = VbapRenderer::new();
    vbap.configure(layout, SAMPLE_RATE, BLOCK_FRAMES, 1)?;
    let mut vbap_scratch = RendererScratch::new(vbap.required_scratch_size()?);
    let mut vbap_gains = vec![SpeakerGain::default(); CHANNELS];

    let input = tone_block();
    let mut output_storage = vec![vec![0.0_f32; BLOCK_FRAMES]; CHANNELS];
    let mut output_refs: Vec<&mut [f32]> =
        output_storage.iter_mut().map(Vec::as_mut_slice).collect();

    // Exact nominal directions must agree between the native PCM renderer and
    // Aurora's independent VBAP implementation. Two native calls cover the
    // 255-frame direct-path delay and one-block metadata interpolation before
    // the steady-state channel-energy comparison.
    for (position, expected_channel) in [
        (Vector3::new(-0.5, 0.866_025_4, 0.0), 0_usize),
        (Vector3::new(0.5, 0.866_025_4, 0.0), 1_usize),
        (Vector3::new(0.0, 1.0, 0.0), 2_usize),
    ] {
        render_twice(
            &mut renderer,
            &listener_origin(),
            position,
            &input,
            &mut output_refs,
        )?;
        let energies = channel_energies(&output_refs);
        let native_dominant = dominant_channel(&energies);

        vbap.render_gains(
            &listener_origin(),
            &[RenderObject {
                position,
                gain: 1.0,
            }],
            &mut vbap_gains,
            &mut vbap_scratch,
        )?;
        let vbap_dominant = vbap_gains
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.gain.total_cmp(&right.gain))
            .map(|(index, _)| index)
            .unwrap_or(0);

        if native_dominant != expected_channel
            || vbap_dominant != expected_channel
            || energies[native_dominant] <= 1.0e-8
        {
            return Err(format!(
                "semantic differential mismatch: position={position:?} expected={expected_channel} native={native_dominant} vbap={vbap_dominant} energies={energies:?} gains={vbap_gains:?}"
            )
            .into());
        }
    }

    // A translated listener facing +X with an object one metre along +X must
    // map to the same local front position as origin/+Y. The final differential
    // case above already leaves the native renderer at local front, so this also
    // exercises unchanged native metadata across a world-pose change.
    let baseline = flatten(&output_refs);
    let moved_listener = Listener {
        position: Vector3::new(10.0, 20.0, 2.0),
        orientation: Vector3::new(1.0, 0.0, 0.0),
        ear_height: 1.2,
    };
    render_twice(
        &mut renderer,
        &moved_listener,
        Vector3::new(11.0, 20.0, 2.0),
        &input,
        &mut output_refs,
    )?;
    let moved = flatten(&output_refs);
    let max_delta = baseline
        .iter()
        .zip(moved.iter())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    if max_delta > 1.0e-5 {
        return Err(format!("listener-relative transform drifted: max_delta={max_delta}").into());
    }

    // The Rust half of the steady-state adapter must allocate nothing after
    // configure. Native C++ allocations are checked separately by the exact-pin
    // operator-new audit in CI.
    let block = [ObjectPcmBlock {
        object: RenderObject {
            position: Vector3::new(11.0, 20.0, 2.0),
            gain: 1.0,
        },
        samples: &input,
    }];
    let mut result = Ok(());
    let rust_allocations = count_allocations(|| {
        result = renderer.render_pcm(&moved_listener, &block, &mut output_refs);
    });
    result?;
    if rust_allocations != 0 {
        return Err(format!("steady-state Rust render allocated {rust_allocations} times").into());
    }

    println!(
        "aurora-libspatialaudio-runtime: PASS outputs={CHANNELS} rate={SAMPLE_RATE} block={BLOCK_FRAMES} latency=255 rust_allocations=0 semantic_differential=fl-fr-fc transform_max_delta={max_delta}"
    );
    Ok(())
}

fn render_twice(
    renderer: &mut LibspatialaudioRenderer,
    listener: &Listener,
    position: Vector3,
    input: &[f32],
    output: &mut [&mut [f32]],
) -> Result<(), aurora_renderer_api::PcmRendererError> {
    let block = [ObjectPcmBlock {
        object: RenderObject {
            position,
            gain: 1.0,
        },
        samples: input,
    }];
    renderer.render_pcm(listener, &block, output)?;
    renderer.render_pcm(listener, &block, output)?;
    Ok(())
}

fn listener_origin() -> Listener {
    Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 1.2,
    }
}

fn tone_block() -> Vec<f32> {
    (0..BLOCK_FRAMES)
        .map(|index| {
            let phase = std::f32::consts::TAU * 440.0 * index as f32 / SAMPLE_RATE as f32;
            0.25 * phase.sin()
        })
        .collect()
}

fn channel_energies(output: &[&mut [f32]]) -> [f64; CHANNELS] {
    std::array::from_fn(|channel| {
        output[channel]
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum()
    })
}

fn dominant_channel(energies: &[f64; CHANNELS]) -> usize {
    energies
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn flatten(output: &[&mut [f32]]) -> Vec<f32> {
    output
        .iter()
        .flat_map(|channel| channel.iter().copied())
        .collect()
}

fn canonical_layout() -> Vec<Speaker> {
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    vec![
        speaker("FL", ChannelRole::FrontLeft, -0.5, 0.866_025_4, 0.0),
        speaker("FR", ChannelRole::FrontRight, 0.5, 0.866_025_4, 0.0),
        speaker("FC", ChannelRole::FrontCenter, 0.0, 1.0, 0.0),
        speaker("LFE", ChannelRole::LowFrequencyEffects, 0.0, 1.0, 0.0),
        speaker("SL", ChannelRole::SurroundLeft, -1.0, 0.0, 0.0),
        speaker("SR", ChannelRole::SurroundRight, 1.0, 0.0, 0.0),
        speaker(
            "SBL",
            ChannelRole::SurroundBackLeft,
            -diagonal,
            -diagonal,
            0.0,
        ),
        speaker(
            "SBR",
            ChannelRole::SurroundBackRight,
            diagonal,
            -diagonal,
            0.0,
        ),
        speaker(
            "TFL",
            ChannelRole::TopFrontLeft,
            -0.5,
            0.866_025_4,
            0.577_350_26,
        ),
        speaker(
            "TFR",
            ChannelRole::TopFrontRight,
            0.5,
            0.866_025_4,
            0.577_350_26,
        ),
        speaker(
            "TRL",
            ChannelRole::TopRearLeft,
            -diagonal,
            -diagonal,
            0.577_350_26,
        ),
        speaker(
            "TRR",
            ChannelRole::TopRearRight,
            diagonal,
            -diagonal,
            0.577_350_26,
        ),
    ]
}

fn speaker(id: &str, channel_role: ChannelRole, x: f32, y: f32, z: f32) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: id.to_owned(),
        channel_role,
        position: Vector3::new(x, y, z),
        orientation: Vector3::ZERO,
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}
