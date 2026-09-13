use std::{env, fs, path::PathBuf};

use aurora_realtime_engine::{
    AsynchronousResampler, DriftController, DriftControllerConfig, RubatoAsrc,
};
use serde_json::{json, Value};

const SAMPLE_RATE: u32 = 48_000;
const WINDOW_OUTPUT_FRAMES: u64 = 480_000;
const TARGET_FILL: usize = 2_048;
const CHANNELS: usize = 2;
const BLOCK_SIZE: usize = 256;
const MAX_STEP_PPM: f64 = 2.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("aurora-resilience-transitions.json"));

    let jitter = clock_jitter_case()?;
    let step = clock_step_case()?;
    let report = json!({
        "schema_version": 1,
        "verdict": "pass",
        "source": "aurora-realtime-audio-sim-real-rust-transition-components",
        "clock_jitter_median_filter": jitter,
        "clock_step_bounded_slew": step,
        "truth_boundary": "hardware-independent frame-count and ASRC-control evidence using Aurora's real estimator/controller/RubatoAsrc path; not physical oscillator or device measurement"
    });

    if let Some(parent) = report_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
    println!(
        "AURORA-RESILIENCE-TRANSITIONS-PASS report={} jitter=median3 step=slew_limited",
        report_path.display()
    );
    Ok(())
}

fn controller() -> Result<DriftController, Box<dyn std::error::Error>> {
    Ok(DriftController::new(DriftControllerConfig {
        input_rate: SAMPLE_RATE,
        output_rate: SAMPLE_RATE,
        target_fill_frames: TARGET_FILL,
        ..DriftControllerConfig::default()
    })?)
}

fn input_frames_for_ppm(ppm: f64) -> u64 {
    ((WINDOW_OUTPUT_FRAMES as f64) * (1.0 + ppm / 1_000_000.0)).round() as u64
}

fn clock_jitter_case() -> Result<Value, Box<dyn std::error::Error>> {
    let jitter_ppm = [210.0, 330.0, 250.0];
    let mut controller = controller()?;
    let mut final_estimate = None;
    for ppm in jitter_ppm {
        final_estimate = controller.observe_clock_frames(
            input_frames_for_ppm(ppm),
            WINDOW_OUTPUT_FRAMES,
            false,
        )?;
    }
    let filtered = final_estimate.ok_or("jitter estimator never became trusted")?;
    if (filtered - 250.0).abs() > 5.0 {
        return Err(format!("median jitter filter missed target: {filtered}").into());
    }
    if (controller.feedforward_correction_ppm() + 250.0).abs() > 5.0 {
        return Err("jitter-filtered feed-forward has wrong sign or magnitude".into());
    }

    let first = controller.update(TARGET_FILL, 0, BLOCK_SIZE)?;
    if first.correction_ppm.abs() > MAX_STEP_PPM + f64::EPSILON {
        return Err("jitter feed-forward bypassed the configured slew bound".into());
    }

    Ok(json!({
        "window_input_ppm": jitter_ppm,
        "trusted_filtered_ppm": filtered,
        "feedforward_correction_ppm": controller.feedforward_correction_ppm(),
        "first_applied_correction_ppm": first.correction_ppm,
        "maximum_allowed_step_ppm": MAX_STEP_PPM,
        "passed": true
    }))
}

fn clock_step_case() -> Result<Value, Box<dyn std::error::Error>> {
    let mut controller = controller()?;
    for _ in 0..3 {
        controller.observe_clock_frames(
            input_frames_for_ppm(250.0),
            WINDOW_OUTPUT_FRAMES,
            false,
        )?;
    }

    let mut asrc = RubatoAsrc::default();
    asrc.configure(SAMPLE_RATE, SAMPLE_RATE, CHANNELS, BLOCK_SIZE)?;
    let mut previous = 0.0_f64;
    let mut maximum_observed_step = 0.0_f64;
    for _ in 0..130 {
        let report = controller.update(TARGET_FILL, 0, BLOCK_SIZE)?;
        maximum_observed_step = maximum_observed_step.max((report.correction_ppm - previous).abs());
        if maximum_observed_step > MAX_STEP_PPM + 1.0e-9 {
            return Err("initial clock acquisition exceeded the configured slew bound".into());
        }
        asrc.set_ratio(report.ratio)?;
        previous = report.correction_ppm;
    }
    if (previous + 250.0).abs() > 5.0 {
        return Err("initial +250 ppm epoch did not converge to -250 ppm correction".into());
    }

    let mut step_estimates = Vec::new();
    for _ in 0..3 {
        if let Some(estimate) = controller.observe_clock_frames(
            input_frames_for_ppm(-250.0),
            WINDOW_OUTPUT_FRAMES,
            false,
        )? {
            step_estimates.push(estimate);
        }
    }
    let stepped_feedforward = controller.feedforward_correction_ppm();
    if (stepped_feedforward - 250.0).abs() > 5.0 {
        return Err("continuous clock step was not re-estimated to the new sign".into());
    }

    let correction_before_step_slew = previous;
    let mut converged = false;
    let mut updates_to_converge = 0_u32;
    for update in 1_u32..=300 {
        let report = controller.update(TARGET_FILL, 0, BLOCK_SIZE)?;
        let delta = (report.correction_ppm - previous).abs();
        maximum_observed_step = maximum_observed_step.max(delta);
        if delta > MAX_STEP_PPM + 1.0e-9 {
            return Err(format!("clock step produced an unsafe correction jump: {delta}").into());
        }
        asrc.set_ratio(report.ratio)?;
        previous = report.correction_ppm;
        if (previous - 250.0).abs() <= 5.0 {
            converged = true;
            updates_to_converge = update;
            break;
        }
    }
    if !converged {
        return Err("clock step did not converge within the bounded slew horizon".into());
    }

    Ok(json!({
        "initial_input_clock_ppm": 250.0,
        "stepped_input_clock_ppm": -250.0,
        "correction_before_step_slew_ppm": correction_before_step_slew,
        "stepped_feedforward_correction_ppm": stepped_feedforward,
        "window_estimates_after_step_ppm": step_estimates,
        "final_correction_ppm": previous,
        "updates_to_converge": updates_to_converge,
        "maximum_observed_step_ppm": maximum_observed_step,
        "maximum_allowed_step_ppm": MAX_STEP_PPM,
        "asrc_ratio_path_exercised": true,
        "passed": true
    }))
}
