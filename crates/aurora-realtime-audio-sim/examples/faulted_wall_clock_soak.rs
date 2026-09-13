use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use aurora_realtime_audio_api::AudioStreamFault;
use aurora_realtime_engine::{
    AsynchronousResampler, DriftController, DriftControllerConfig, DuplexStateEvent,
    DuplexStateMachine, DuplexStreamState, ProcessStatus, RealTimeEngineConfig, RealTimeFault,
    RubatoAsrc, TestSignal,
};
use aurora_runtime_materialization::materialize_default_realtime_engine;
use aurora_scene::RenderScene;
use serde::Serialize;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_SIZE: usize = 256;
const CHANNELS: usize = 12;
const DEFAULT_WALL_SECONDS: u64 = 15;
const TARGET_FILL: usize = 2_048;
const CLOCK_WINDOW_OUTPUT_FRAMES: u64 = 480_000;
const MAX_RSS_GROWTH_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CORRECTION_STEP_PPM: f64 = 2.000_001;

#[derive(Debug, Serialize)]
struct FaultedWallClockReport {
    schema_version: u32,
    accepted: bool,
    wall_seconds_requested: u64,
    wall_seconds_observed: f64,
    callbacks: u64,
    processing_deadline_misses: u64,
    maximum_processing_microseconds: f64,
    observed_peak: f32,
    rss_start_bytes: Option<u64>,
    rss_max_bytes: Option<u64>,
    rss_final_bytes: Option<u64>,
    clock_initial_trusted_ppm: Option<f64>,
    clock_initial_feedforward_ppm: Option<f64>,
    clock_feedforward_after_discontinuity_ppm: Option<f64>,
    clock_correction_after_discontinuity_ppm: Option<f64>,
    clock_discontinuity_reset: bool,
    clock_reacquired_ppm: Option<f64>,
    clock_reacquired_feedforward_ppm: f64,
    maximum_correction_step_ppm: f64,
    reconnect_backoff_ms: Vec<u64>,
    reconnect_success_attempt: Option<u32>,
    reconnect_stable_reset_attempts: u32,
    reconnect_final_state: String,
    engine_input_underruns: u64,
    engine_output_underruns: u64,
    engine_dropped_blocks: u64,
    engine_fault: String,
    violations: Vec<String>,
    truth_boundary: &'static str,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let wall_seconds = args
        .next()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(DEFAULT_WALL_SECONDS);
    let report_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("output/validation/faulted-wall-clock-soak-v1.json"));
    if args.next().is_some() || wall_seconds < 15 {
        return Err("usage: faulted_wall_clock_soak [wall_seconds>=15] [report_path]".into());
    }

    let scene: RenderScene = serde_json::from_str(include_str!(
        "../../../fixtures/scenes/7_1_4_reference.json"
    ))?;
    let mut engine = materialize_default_realtime_engine(
        scene,
        RealTimeEngineConfig {
            sample_rate: SAMPLE_RATE,
            block_size: BLOCK_SIZE,
            input_channels: 0,
            apply_geometric_delay: false,
            speed_of_sound: 343.0,
            test_signal: TestSignal::RotatingSine,
        },
        0,
    )?;
    if engine.output_roles().len() != CHANNELS {
        return Err("expected 12-channel 7.1.4 engine".into());
    }

    let mut controller = DriftController::new(DriftControllerConfig {
        input_rate: SAMPLE_RATE,
        output_rate: SAMPLE_RATE,
        target_fill_frames: TARGET_FILL,
        ..DriftControllerConfig::default()
    })?;
    let mut asrc = RubatoAsrc::default();
    asrc.configure(SAMPLE_RATE, SAMPLE_RATE, 2, BLOCK_SIZE)?;

    let mut device = DuplexStateMachine::default();
    device.transition(DuplexStateEvent::StartRequested)?;
    device.transition(DuplexStateEvent::StreamsStarted)?;

    let callback_period = Duration::from_secs_f64(BLOCK_SIZE as f64 / f64::from(SAMPLE_RATE));
    let requested_duration = Duration::from_secs(wall_seconds);
    let started = Instant::now();
    let mut next_deadline = started;
    let mut output = vec![0.0_f32; BLOCK_SIZE * CHANNELS];
    let mut callbacks = 0_u64;
    let mut deadline_misses = 0_u64;
    let mut maximum_processing = Duration::ZERO;
    let mut observed_peak = 0.0_f32;
    let rss_start = resident_set_bytes();
    let mut rss_max = rss_start;
    let mut next_memory_sample = started;

    let jitter_windows = [210.0_f64, 330.0, 250.0];
    let mut next_clock_window = 0_usize;
    let mut initial_trusted = None;
    let mut initial_feedforward = None;
    let mut discontinuity_done = false;
    let mut feedforward_after_discontinuity = None;
    let mut correction_after_discontinuity = None;
    let mut reverse_windows = 0_u8;
    let mut reacquired = None;
    let mut previous_correction = 0.0_f64;
    let mut maximum_correction_step = 0.0_f64;
    let mut epoch_reset_pending = false;

    let mut device_fault_injected = false;
    let mut recovery_due: Option<Instant> = None;
    let mut reconnect_backoff = Vec::new();
    let mut reconnect_success_attempt = None;
    let mut running_since: Option<Instant> = None;
    let mut stable_reset_done = false;

    while started.elapsed() < requested_duration {
        let now = Instant::now();
        if now < next_deadline {
            thread::sleep(next_deadline - now);
        }

        let elapsed = started.elapsed();
        if next_clock_window < jitter_windows.len()
            && elapsed >= Duration::from_secs((next_clock_window + 1) as u64)
        {
            let ppm = jitter_windows[next_clock_window];
            initial_trusted = controller.observe_clock_frames(
                input_frames_for_ppm(ppm),
                CLOCK_WINDOW_OUTPUT_FRAMES,
                false,
            )?;
            if initial_trusted.is_some() {
                initial_feedforward = Some(controller.feedforward_correction_ppm());
            }
            next_clock_window += 1;
        }

        if !device_fault_injected && elapsed >= Duration::from_secs(5) {
            device.transition(DuplexStateEvent::StreamFault(AudioStreamFault::DeviceLost))?;
            device_fault_injected = true;
            schedule_next_recovery(&device, &mut recovery_due, &mut reconnect_backoff)?;
        }
        if let Some(due) = recovery_due {
            if Instant::now() >= due {
                device.transition(DuplexStateEvent::RecoveryRequested)?;
                let attempt = device.recovery_attempts();
                if attempt < 3 {
                    device.transition(DuplexStateEvent::RecoveryFailed)?;
                    schedule_next_recovery(&device, &mut recovery_due, &mut reconnect_backoff)?;
                } else {
                    device.transition(DuplexStateEvent::RecoverySucceeded)?;
                    reconnect_success_attempt = Some(attempt);
                    running_since = Some(Instant::now());
                    recovery_due = None;
                }
            }
        }
        if !stable_reset_done
            && running_since
                .is_some_and(|time| Instant::now().duration_since(time) >= Duration::from_secs(1))
        {
            device.transition(DuplexStateEvent::StableRunObserved)?;
            stable_reset_done = true;
        }

        if !discontinuity_done && elapsed >= Duration::from_secs(9) {
            controller.observe_clock_frames(1, 1, true)?;
            feedforward_after_discontinuity = Some(controller.feedforward_correction_ppm());
            discontinuity_done = true;
            epoch_reset_pending = true;
            // A clock-epoch discontinuity deliberately resets the controller while the media path
            // is expected to be muted/recovering. Do not interpret that reset boundary as an
            // in-epoch ASRC slew step.
            previous_correction = 0.0;
        }
        if discontinuity_done
            && reverse_windows < 3
            && elapsed >= Duration::from_secs(10 + u64::from(reverse_windows))
        {
            reacquired = controller.observe_clock_frames(
                input_frames_for_ppm(-250.0),
                CLOCK_WINDOW_OUTPUT_FRAMES,
                false,
            )?;
            reverse_windows += 1;
        }

        let correction = controller.update(TARGET_FILL, 0, BLOCK_SIZE)?;
        if epoch_reset_pending {
            correction_after_discontinuity = Some(correction.correction_ppm);
            epoch_reset_pending = false;
        }
        let correction_step = (correction.correction_ppm - previous_correction).abs();
        maximum_correction_step = maximum_correction_step.max(correction_step);
        previous_correction = correction.correction_ppm;
        asrc.set_ratio(correction.ratio)?;

        let callback_started = Instant::now();
        let status = engine.process_interleaved(None, &mut output);
        let processing = callback_started.elapsed();
        maximum_processing = maximum_processing.max(processing);
        if processing > callback_period {
            deadline_misses = deadline_misses.saturating_add(1);
        }
        if status != ProcessStatus::Ok || output.iter().any(|sample| !sample.is_finite()) {
            return Err(format!("faulted soak callback failed: status={status:?}").into());
        }
        for sample in &output {
            observed_peak = observed_peak.max(sample.abs());
        }
        callbacks = callbacks.saturating_add(1);
        next_deadline += callback_period;

        if Instant::now() >= next_memory_sample {
            if let Some(rss) = resident_set_bytes() {
                rss_max = Some(rss_max.unwrap_or(rss).max(rss));
            }
            next_memory_sample += Duration::from_secs(1);
        }
    }

    let elapsed = started.elapsed();
    let rss_final = resident_set_bytes();
    if let Some(rss) = rss_final {
        rss_max = Some(rss_max.unwrap_or(rss).max(rss));
    }
    let metrics = engine.metrics();
    let discontinuity_reset = feedforward_after_discontinuity
        .is_some_and(|ppm| ppm.abs() <= f64::EPSILON)
        && correction_after_discontinuity.is_some_and(|ppm| ppm.abs() <= f64::EPSILON);
    let mut violations = Vec::new();

    if initial_trusted.is_none() {
        violations.push("jitter-filtered +250 ppm clock estimate never became trusted".to_owned());
    }
    if let Some(ppm) = initial_trusted {
        if (ppm - 250.0).abs() > 5.0 {
            violations.push(format!(
                "initial filtered clock estimate outside tolerance: {ppm}"
            ));
        }
    }
    match initial_feedforward {
        Some(ppm) if (ppm + 250.0).abs() <= 5.0 => {}
        Some(ppm) => violations.push(format!(
            "initial feed-forward outside tolerance or wrong sign: {ppm}"
        )),
        None => violations.push("initial feed-forward was never observed".to_owned()),
    }
    if !discontinuity_done {
        violations.push("clock discontinuity was not injected".to_owned());
    }
    if !discontinuity_reset {
        violations.push(format!(
            "clock discontinuity did not reset feed-forward/correction: feedforward={feedforward_after_discontinuity:?} correction={correction_after_discontinuity:?}"
        ));
    }
    if let Some(ppm) = reacquired {
        if (ppm + 250.0).abs() > 5.0 {
            violations.push(format!(
                "reacquired clock estimate outside tolerance: {ppm}"
            ));
        }
    } else {
        violations.push("reversed clock epoch was not reacquired".to_owned());
    }
    if (controller.feedforward_correction_ppm() - 250.0).abs() > 5.0 {
        violations.push("reacquired feed-forward has wrong sign or magnitude".to_owned());
    }
    if maximum_correction_step > MAX_CORRECTION_STEP_PPM {
        violations.push(format!(
            "in-epoch clock correction slew exceeded 2 ppm/update: {maximum_correction_step}"
        ));
    }
    if reconnect_backoff != [250, 500, 1_000]
        || reconnect_success_attempt != Some(3)
        || device.state() != DuplexStreamState::Running
        || !stable_reset_done
        || device.recovery_attempts() != 0
    {
        violations.push(format!(
            "bounded reconnect mismatch: backoff={reconnect_backoff:?} success={reconnect_success_attempt:?} state={:?} attempts={}",
            device.state(),
            device.recovery_attempts()
        ));
    }
    if callbacks == 0 || observed_peak <= f32::EPSILON {
        violations.push("engine produced no usable callbacks or remained silent".to_owned());
    }
    if deadline_misses != 0 {
        violations.push(format!(
            "{deadline_misses} callback processing deadline misses"
        ));
    }
    if metrics.input_underruns != 0 || metrics.output_underruns != 0 || metrics.dropped_blocks != 0
    {
        violations.push("engine underrun/drop counters changed during paced soak".to_owned());
    }
    if metrics.fault != RealTimeFault::None {
        violations.push(format!("engine fault: {:?}", metrics.fault));
    }
    if let (Some(start), Some(maximum)) = (rss_start, rss_max) {
        if maximum.saturating_sub(start) > MAX_RSS_GROWTH_BYTES {
            violations.push(format!("RSS grew more than {MAX_RSS_GROWTH_BYTES} bytes"));
        }
    }

    let report = FaultedWallClockReport {
        schema_version: 1,
        accepted: violations.is_empty(),
        wall_seconds_requested: wall_seconds,
        wall_seconds_observed: elapsed.as_secs_f64(),
        callbacks,
        processing_deadline_misses: deadline_misses,
        maximum_processing_microseconds: maximum_processing.as_secs_f64() * 1_000_000.0,
        observed_peak,
        rss_start_bytes: rss_start,
        rss_max_bytes: rss_max,
        rss_final_bytes: rss_final,
        clock_initial_trusted_ppm: initial_trusted,
        clock_initial_feedforward_ppm: initial_feedforward,
        clock_feedforward_after_discontinuity_ppm: feedforward_after_discontinuity,
        clock_correction_after_discontinuity_ppm: correction_after_discontinuity,
        clock_discontinuity_reset: discontinuity_reset,
        clock_reacquired_ppm: reacquired,
        clock_reacquired_feedforward_ppm: controller.feedforward_correction_ppm(),
        maximum_correction_step_ppm: maximum_correction_step,
        reconnect_backoff_ms: reconnect_backoff,
        reconnect_success_attempt,
        reconnect_stable_reset_attempts: device.recovery_attempts(),
        reconnect_final_state: format!("{:?}", device.state()),
        engine_input_underruns: metrics.input_underruns,
        engine_output_underruns: metrics.output_underruns,
        engine_dropped_blocks: metrics.dropped_blocks,
        engine_fault: format!("{:?}", metrics.fault),
        violations,
        truth_boundary: "One wall-clock-paced software process combines Aurora 7.1.4 callbacks and RSS/deadline observation with accelerated virtual clock-window observations, real DriftController/RubatoAsrc control, and simulated DuplexStateMachine device-loss recovery. The <=2 ppm/update slew assertion applies only within a continuous clock epoch; the explicit discontinuity boundary is reset/mute territory and is checked separately. Clock windows are intentionally compressed relative to wall time, and device loss is injected software control: neither is physical eARC/USB/TDM/hotplug evidence.",
    };

    if let Some(parent) = report_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)? + "\n")?;
    if !report.accepted {
        return Err(format!("faulted wall-clock soak failed: {:?}", report.violations).into());
    }
    println!(
        "AURORA FAULTED WALL CLOCK SOAK PASS wall_seconds={:.3} callbacks={} max_process_us={:.1} clock_reacquired={:?} reconnect_backoff={:?} report={}",
        report.wall_seconds_observed,
        report.callbacks,
        report.maximum_processing_microseconds,
        report.clock_reacquired_ppm,
        report.reconnect_backoff_ms,
        report_path.display()
    );
    Ok(())
}

fn input_frames_for_ppm(ppm: f64) -> u64 {
    ((CLOCK_WINDOW_OUTPUT_FRAMES as f64) * (1.0 + ppm / 1_000_000.0)).round() as u64
}

fn schedule_next_recovery(
    device: &DuplexStateMachine,
    recovery_due: &mut Option<Instant>,
    backoffs: &mut Vec<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let backoff = device
        .next_recovery_backoff_ms()
        .ok_or("recovery budget unexpectedly unavailable")?;
    backoffs.push(backoff);
    *recovery_due = Some(Instant::now() + Duration::from_millis(backoff));
    Ok(())
}

#[cfg(target_os = "linux")]
fn resident_set_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    kib.checked_mul(1024)
}

#[cfg(not(target_os = "linux"))]
fn resident_set_bytes() -> Option<u64> {
    None
}
