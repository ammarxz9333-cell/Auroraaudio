//! Isolated validation executable for Aurora's prepared FIR binaural primitive.
//!
//! The CI workflow compiles this file in a temporary crate. JSON/reference tooling therefore
//! does not become a dependency of `aurora-renderer-basic` or any production runtime path.

use aurora_renderer_basic::binaural::{Filters, Input, PreparedBinaural};
use serde_json::{json, Value};
use std::{collections::BTreeSet, error::Error, fs};

fn finite_f32_array(value: &Value, label: &str) -> Result<Vec<f32>, Box<dyn Error>> {
    let values = value.as_array().ok_or_else(|| format!("{label} must be an array"))?;
    values
        .iter()
        .map(|value| {
            let number = value
                .as_f64()
                .ok_or_else(|| format!("{label} contains a non-number"))? as f32;
            if !number.is_finite() {
                return Err(format!("{label} contains a nonfinite value").into());
            }
            Ok(number)
        })
        .collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: aurora_prepared_fir_probe <oracle-json> <evidence-json>".into());
    }

    let contract: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    if contract["schema_version"] != 1
        || contract["sample_rate"] != 48_000
        || contract["taps"] != 1024
        || contract["frames"] != 2048
        || contract["impulse_frame"] != 256
    {
        return Err("unsupported SOFA prepared-FIR reference contract".into());
    }
    let impulse_gain = contract["impulse_gain"]
        .as_f64()
        .ok_or("missing impulse_gain")? as f32;
    if !impulse_gain.is_finite() || impulse_gain.abs() > 1.0 {
        return Err("invalid impulse_gain".into());
    }

    let cases = contract["cases"].as_array().ok_or("missing cases")?;
    if cases.len() != 6 {
        return Err("expected exactly six direct-object SOFA cases".into());
    }
    let expected_names = ["front", "back", "left", "right", "up", "down"];
    let mut names = BTreeSet::new();
    let mut evidence = Vec::new();

    for case in cases {
        let name = case["name"].as_str().ok_or("missing case name")?;
        if !expected_names.contains(&name) || !names.insert(name.to_owned()) {
            return Err(format!("unexpected or duplicate case: {name}").into());
        }
        let direction = finite_f32_array(&case["direction"], "direction")?;
        if direction.len() != 3 {
            return Err(format!("invalid direction for {name}").into());
        }
        let coefficients = finite_f32_array(&case["coefficients"], "coefficients")?;
        let expected = finite_f32_array(&case["expected"], "expected")?;
        if coefficients.len() != 2048 || expected.len() != 4096 {
            return Err(format!("invalid bounded data shape for {name}").into());
        }

        let filters = Filters::prepare(Input::Objects(1), 48_000, 1, 1024, coefficients)
            .map_err(|error| format!("filter preparation failed for {name}: {error:?}"))?;
        let mut renderer = PreparedBinaural::new(filters, 256)
            .map_err(|error| format!("renderer preparation failed for {name}: {error:?}"))?;
        let mut output = vec![0.0; 4096];
        let mut input = [0.0_f32; 256];
        for (block_index, stereo) in output.chunks_exact_mut(512).enumerate() {
            input.fill(0.0);
            if block_index == 1 {
                input[0] = impulse_gain;
            }
            renderer
                .process(&input, stereo)
                .map_err(|error| format!("FIR process failed for {name}: {error:?}"))?;
        }

        let max_error = output
            .iter()
            .zip(&expected)
            .map(|(actual, expected)| (actual - expected).abs())
            .fold(0.0_f32, f32::max);
        let left_energy: f64 = output
            .iter()
            .step_by(2)
            .map(|sample| f64::from(*sample).powi(2))
            .sum();
        let right_energy: f64 = output
            .iter()
            .skip(1)
            .step_by(2)
            .map(|sample| f64::from(*sample).powi(2))
            .sum();
        if !max_error.is_finite() || max_error > 1.0e-5 || left_energy + right_energy <= 1.0e-12 {
            return Err(format!(
                "prepared FIR mismatch for {name}: max_error={max_error} total_energy={}",
                left_energy + right_energy
            )
            .into());
        }
        evidence.push(json!({
            "name": name,
            "direction": direction,
            "max_absolute_pcm_error": max_error,
            "left_energy": left_energy,
            "right_energy": right_energy,
            "pass": true
        }));
    }

    if names.len() != expected_names.len() {
        return Err("direct-object case set incomplete".into());
    }
    fs::write(
        &args[2],
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "verdict": "pass",
            "cases": evidence,
            "truth_boundary": "Exact-pinned sofar extracts MIT KEMAR FIRs and produces the convolution oracle; Aurora reproduces that PCM with its prepared FIR primitive. This proves software transfer-function execution only, not independent HRTF semantic correctness, personalization, head tracking, perception, physical latency or certification."
        }))?,
    )?;
    Ok(())
}
