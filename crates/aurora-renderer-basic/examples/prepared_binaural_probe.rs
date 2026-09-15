//! Executes externally prepared SOFA filters in Aurora and compares real stereo PCM.
use aurora_renderer_basic::binaural::{Filters, Input, PreparedBinaural};
use serde_json::{json, Value};
use std::{error::Error, fs};

fn samples(value: &Value) -> Result<Vec<f32>, Box<dyn Error>> {
    value
        .as_array()
        .ok_or("expected array")?
        .iter()
        .map(|x| {
            let x = x.as_f64().ok_or("expected number")? as f32;
            if !x.is_finite() {
                return Err("nonfinite coefficient".into());
            }
            Ok(x)
        })
        .collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: prepared_binaural_probe <oracle.json> <evidence.json>".into());
    }
    let contract: Value = serde_json::from_slice(&fs::read(&args[1])?)?;
    if contract["schema_version"] != 1
        || contract["sample_rate"] != 48_000
        || contract["taps"] != 1024
        || contract["frames"] != 2048
    {
        return Err("unsupported prepared reference contract".into());
    }
    let cases = contract["cases"].as_array().ok_or("missing cases")?;
    if cases.len() != 19 {
        return Err("incomplete reference cases".into());
    }
    let mut evidence = Vec::new();
    for case in cases {
        let order = case["hoa_order"].as_u64().ok_or("missing order")?;
        if order > 3 {
            return Err("unsupported HOA order".into());
        }
        let input = if order == 0 {
            Input::Objects(1)
        } else {
            Input::AmbisonicsAcnSn3d(order as u8)
        };
        let channels = input.channels().map_err(|e| format!("{e:?}"))?;
        let weights = samples(&case["weights"])?;
        if weights.len() != channels {
            return Err("invalid input weights".into());
        }
        let filters = Filters::prepare(input, 48_000, 1, 1024, samples(&case["coefficients"])?)
            .map_err(|e| format!("{e:?}"))?;
        let mut renderer = PreparedBinaural::new(filters, 256).map_err(|e| format!("{e:?}"))?;
        let mut output = vec![0.0; 4096];
        let mut block = vec![0.0; 256 * channels];
        for (index, out) in output.chunks_exact_mut(512).enumerate() {
            block.fill(0.0);
            if index == 1 {
                for (i, w) in weights.iter().enumerate() {
                    block[i] = 0.25 * w;
                }
            }
            renderer
                .process(&block, out)
                .map_err(|e| format!("{e:?}"))?;
        }
        let expected = samples(&case["expected"])?;
        if expected.len() != output.len() {
            return Err("oracle frame mismatch".into());
        }
        let max_error = output
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        let left: f64 = output
            .iter()
            .step_by(2)
            .map(|x| f64::from(*x).powi(2))
            .sum();
        let right: f64 = output
            .iter()
            .skip(1)
            .step_by(2)
            .map(|x| f64::from(*x).powi(2))
            .sum();
        if max_error > 0.00001 || left + right < 1e-12 {
            return Err(format!(
                "PCM mismatch or silent output for {}: {max_error}",
                case["name"]
            )
            .into());
        }
        evidence.push(json!({"name":case["name"],"hoa_order":order,"head_direction":case["head_direction"],"weights":weights,
            "max_absolute_pcm_error":max_error,"left_energy":left,"right_energy":right,
            "bias_db":10.0*((left+1e-20)/(right+1e-20)).log10(),"pcm":output,"pass":true}));
    }
    fs::write(
        &args[2],
        serde_json::to_vec(&json!({"schema_version":1,"cases":evidence,
        "truth_boundary":"Prepared SOFA FIR object/ACN-SN3D HOA software PCM reference. No perceptual, head tracker, physical latency or certification claim.","verdict":"pass"}))?,
    )?;
    Ok(())
}
