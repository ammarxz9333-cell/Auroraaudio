//! Isolated validation executable for Aurora's prepared FIR binaural primitive.
//!
//! The CI workflow compiles this file in a temporary crate. JSON/reference tooling therefore
//! does not become a dependency of `aurora-renderer-basic` or any production runtime path.

use aurora_renderer_basic::binaural::{Filters, Input, PreparedBinaural};
use aurora_renderer_basic::binaural::hrtf::{DirectionalHrtf, SofaMeasurement};
use aurora_core::Vector3;
use aurora_renderer_api::{HeadPosePolicy, HeadPoseSample, HeadPoseState, UnitQuaternion};
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

    let mut measurements = Vec::new();
    for case in cases {
        let d = finite_f32_array(&case["direction"], "direction")?;
        if d.len() != 3 { return Err("invalid SOFA direction".into()); }
        measurements.push(SofaMeasurement {
            direction: Vector3::new(d[0], d[1], d[2]),
            coefficients: finite_f32_array(&case["coefficients"], "coefficients")?,
        });
    }
    let bank = DirectionalHrtf::prepare(48_000, 1024, 0.01, measurements)
        .map_err(|e| format!("HRTF bank: {e:?}"))?;
    let mut poses = HeadPoseState::new(HeadPosePolicy::new(100, 10).unwrap());
    poses.commit(HeadPoseSample { sequence: 1, media_frame: 100, orientation: UnitQuaternion::IDENTITY }).unwrap();
    let half = std::f32::consts::FRAC_PI_4;
    poses.commit(HeadPoseSample { sequence: 2, media_frame: 200, orientation: UnitQuaternion::try_new(half.cos(), 0.0, 0.0, half.sin()).unwrap() }).unwrap();

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
        let mut tracked_max_error = 0.0_f32;
        for (frame, world) in [
            (100, Vector3::new(-direction[1], direction[0], direction[2])),
            // Analytic +90 degree head-to-world yaw, independent of transform implementation.
            (200, Vector3::new(-direction[0], -direction[1], direction[2])),
        ] {
            let filters = bank.prepare_objects(&poses, frame, &[world], 1)
                .map_err(|e| format!("head-pose preparation {name}: {e:?}"))?;
            let mut tracked = PreparedBinaural::new(filters, 256)
                .map_err(|e| format!("tracked renderer: {e:?}"))?;
            let mut pcm = vec![0.0; 4096];
            for (block, stereo) in pcm.chunks_exact_mut(512).enumerate() {
                input.fill(0.0);
                if block == 1 { input[0] = impulse_gain; }
                tracked.process(&input, stereo).map_err(|e| format!("tracked PCM: {e:?}"))?;
            }
            for (actual, reference) in pcm.iter().zip(&expected) {
                if !actual.is_finite() { return Err("nonfinite tracked PCM".into()); }
                tracked_max_error = tracked_max_error.max((actual - reference).abs());
            }
        }
        if tracked_max_error > 1e-5 { return Err(format!("tracked SOFA mismatch {name}: {tracked_max_error}").into()); }
        evidence.push(json!({
            "head_pose_cases": 2,
            "head_pose_max_absolute_pcm_error": tracked_max_error,
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
            "truth_boundary": "Exact-pinned sofar extracts MIT KEMAR FIRs and produces the convolution oracle; Aurora reproduces that PCM with its prepared FIR primitive. This proves software transfer-function execution and canonical direction selection at identity and analytic 90-degree yaw. It does not prove continuous tracker scheduling, personalization, perception, physical tracking, physical latency or certification."
        }))?,
    )?;
    Ok(())
}
