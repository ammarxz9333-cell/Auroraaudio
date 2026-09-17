//! External validation helper compiled in an isolated crate against exact-pinned `sofar`.
//!
//! This is not Aurora runtime code. It turns six deterministic MIT KEMAR directions into
//! bounded 48 kHz FIR coefficients plus `sofar::render::Renderer` impulse-response PCM so
//! Aurora's prepared FIR primitive can be checked against the same extracted filters.

use sofar::reader::{Filter, OpenOptions, Sofar};
use sofar::render::Renderer;
use std::{error::Error, fs};

const SAMPLE_RATE: f32 = 48_000.0;
const TAPS: usize = 1024;
const FRAMES: usize = 2048;
const IMPULSE_FRAME: usize = 256;
const IMPULSE_GAIN: f32 = 0.25;

fn padded_response(sofa: &Sofar, direction: [f32; 3]) -> Result<Filter, Box<dyn Error>> {
    let mut raw = Filter::new(sofa.filter_len());
    sofa.filter(direction[0], direction[1], direction[2], &mut raw);

    let mut padded = Filter::new(TAPS);
    for (source, delay, target) in [
        (&raw.left, raw.ldelay, &mut padded.left),
        (&raw.right, raw.rdelay, &mut padded.right),
    ] {
        if !delay.is_finite() || delay < 0.0 {
            return Err("invalid SOFA delay".into());
        }
        let delayed_samples = delay * SAMPLE_RATE;
        let offset = delayed_samples.floor() as usize;
        let fraction = delayed_samples.fract();
        if offset + source.len() + 1 > TAPS {
            return Err("SOFA response exceeds prepared 1024-tap bound".into());
        }
        for (index, value) in source.iter().enumerate() {
            if !value.is_finite() {
                return Err("nonfinite SOFA coefficient".into());
            }
            target[offset + index] += value * (1.0 - fraction);
            target[offset + index + 1] += value * fraction;
        }
    }
    if padded
        .left
        .iter()
        .chain(padded.right.iter())
        .any(|value| !value.is_finite())
    {
        return Err("nonfinite padded SOFA response".into());
    }
    Ok(padded)
}

fn render_expected(filter: &Filter) -> Result<Vec<f32>, Box<dyn Error>> {
    let mut renderer = Renderer::builder(TAPS).with_partition_len(256).build()?;
    renderer.set_filter(filter)?;
    let mut input = vec![0.0; FRAMES];
    input[IMPULSE_FRAME] = IMPULSE_GAIN;
    let mut left = vec![0.0; FRAMES];
    let mut right = vec![0.0; FRAMES];
    renderer.process_block(&input, &mut left, &mut right)?;
    let mut interleaved = Vec::with_capacity(FRAMES * 2);
    for (left, right) in left.into_iter().zip(right) {
        interleaved.push(left);
        interleaved.push(right);
    }
    if interleaved.iter().any(|value| !value.is_finite()) {
        return Err("nonfinite sofar convolution output".into());
    }
    Ok(interleaved)
}

fn case(sofa: &Sofar, name: &str, direction: [f32; 3]) -> Result<String, Box<dyn Error>> {
    let filter = padded_response(sofa, direction)?;
    let expected = render_expected(&filter)?;
    let mut coefficients = Vec::with_capacity(TAPS * 2);
    coefficients.extend_from_slice(&filter.left);
    coefficients.extend_from_slice(&filter.right);
    Ok(format!(
        "{{\"name\":{name:?},\"direction\":{direction:?},\"coefficients\":{coefficients:?},\"expected\":{expected:?}}}"
    ))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: sofar_direct_filter_export <sofa> <output-json>".into());
    }

    let sofa = OpenOptions::new().sample_rate(SAMPLE_RATE).open(&args[1])?;
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    let directions = [
        ("front", [1.0, 0.0, 0.0]),
        ("back", [-1.0, 0.0, 0.0]),
        ("left", [0.0, 1.0, 0.0]),
        ("right", [0.0, -1.0, 0.0]),
        ("up", [diagonal, 0.0, diagonal]),
        ("down", [diagonal, 0.0, -diagonal]),
    ];
    let mut cases = Vec::new();
    for (name, direction) in directions {
        cases.push(case(&sofa, name, direction)?);
    }

    fs::write(
        &args[2],
        format!(
            "{{\"schema_version\":1,\"sample_rate\":48000,\"taps\":{TAPS},\"frames\":{FRAMES},\"impulse_frame\":{IMPULSE_FRAME},\"impulse_gain\":{IMPULSE_GAIN},\"cases\":[{}]}}\n",
            cases.join(",")
        ),
    )?;
    Ok(())
}
