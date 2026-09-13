use std::{env, fs, path::PathBuf};

use aurora_realtime_audio_api::AudioStreamFault;
use aurora_realtime_engine::{
    create_adaptive_duplex_bridge, AdaptiveDuplexFault, AsynchronousResampler, DriftController,
    DriftControllerConfig, DuplexBridgeConfig, DuplexFaultPolicy, DuplexHealth, DuplexStateEvent,
    DuplexStateMachine, DuplexStreamState, RubatoAsrc,
};
use serde_json::{json, Value};

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_SIZE: usize = 256;
const CHANNELS: usize = 2;
const TARGET_FILL: usize = 2_048;
const CAPACITY_FRAMES: usize = 4_096;
const ESTIMATOR_WINDOW_OUTPUT_FRAMES: u64 = 480_000;
const LONG_RUN_SECONDS: u64 = 24 * 60 * 60;
const EXPECTED_BACKOFF_MS: [u64; 5] = [250, 500, 1_000, 2_000, 4_000];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("aurora-resilience-sim.json"));

    let positive = clock_case(250.0)?;
    let negative = clock_case(-250.0)?;
    let discontinuity = clock_discontinuity_reacquire_case()?;
    let out_of_range = clock_out_of_range_case()?;
    let hard_latch = adaptive_bridge_hard_latch_case()?;
    let reconnect = reconnect_case()?;
    let exhaustion = reconnect_exhaustion_case()?;
    let flapping = reconnect_flapping_case()?;

    let report = json!({
        "schema_version": 1,
        "verdict": "pass",
        "source": "aurora-realtime-audio-sim-real-rust-components",
        "adaptive_clock_rate_correction": {
            "cases": [positive, negative],
            "discontinuity_reacquire": discontinuity,
            "out_of_range_fail_closed": out_of_range,
            "adaptive_fault_latch": hard_latch,
            "truth_boundary": "hardware-independent virtual clocks using Aurora DriftController estimator/feed-forward/slew logic plus the production adaptive duplex/RubatoAsrc path; not physical clock measurement"
        },
        "device_reconnect_recovery": {
            "successful_reconnect": reconnect,
            "budget_exhaustion": exhaustion,
            "flapping_device": flapping,
            "truth_boundary": "Aurora DuplexStateMachine bounded recovery policy; backend reopen timing is simulated and no physical device hotplug is claimed"
        }
    });

    if let Some(parent) = report_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
    println!(
        "AURORA-RESILIENCE-SIM-PASS report={} clock_cases=5 reconnect_cases=3 bounded_attempts=5",
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
    ((ESTIMATOR_WINDOW_OUTPUT_FRAMES as f64) * (1.0 + ppm / 1_000_000.0)).round() as u64
}

fn acquire_clock_estimate(
    controller: &mut DriftController,
    ppm: f64,
) -> Result<f64, Box<dyn std::error::Error>> {
    let input_per_window = input_frames_for_ppm(ppm);
    let mut trusted_estimate = None;
    for _ in 0..3 {
        trusted_estimate = controller.observe_clock_frames(
            input_per_window,
            ESTIMATOR_WINDOW_OUTPUT_FRAMES,
            false,
        )?;
    }
    trusted_estimate.ok_or_else(|| "clock estimator did not become trusted".into())
}

fn clock_case(input_clock_ppm: f64) -> Result<Value, Box<dyn std::error::Error>> {
    let mut controller = controller()?;
    let trusted_estimate = acquire_clock_estimate(&mut controller, input_clock_ppm)?;
    if (trusted_estimate - input_clock_ppm).abs() > 5.0 {
        return Err(format!(
            "clock estimate outside tolerance: requested={input_clock_ppm} estimated={trusted_estimate}"
        )
        .into());
    }
    if (controller.feedforward_correction_ppm() + input_clock_ppm).abs() > 5.0 {
        return Err("feed-forward correction has wrong sign or magnitude".into());
    }

    let mut asrc = RubatoAsrc::default();
    asrc.configure(SAMPLE_RATE, SAMPLE_RATE, CHANNELS, BLOCK_SIZE)?;

    let mut fill = TARGET_FILL as f64;
    let mut minimum_fill = fill;
    let mut maximum_fill = fill;
    let mut final_correction_ppm = 0.0;
    let mut finite_sample_blocks = 0_u64;
    let mut phase = 0.0_f32;

    for second in 0..LONG_RUN_SECONDS {
        let report = controller.update(
            fill.round().clamp(0.0, usize::MAX as f64) as usize,
            (fill - TARGET_FILL as f64)
                .round()
                .clamp(i64::MIN as f64, i64::MAX as f64) as i64,
            SAMPLE_RATE as usize,
        )?;
        asrc.set_ratio(report.ratio)?;
        final_correction_ppm = report.correction_ppm;
        fill += f64::from(SAMPLE_RATE) * (input_clock_ppm + report.correction_ppm) / 1_000_000.0;
        minimum_fill = minimum_fill.min(fill);
        maximum_fill = maximum_fill.max(fill);
        if !(0.0..CAPACITY_FRAMES as f64).contains(&fill) {
            return Err(format!(
                "adaptive clock correction escaped fixed ring capacity: ppm={input_clock_ppm} fill={fill}"
            )
            .into());
        }

        if second % 3_600 == 0 || second + 1 == LONG_RUN_SECONDS {
            let required = asrc.required_input_frames();
            let mut input = vec![0.0_f32; required * CHANNELS];
            for frame in input.chunks_exact_mut(CHANNELS) {
                let sample = phase.sin() * 0.1;
                frame[0] = sample;
                frame[1] = -sample;
                phase = (phase + 0.071_f32) % std::f32::consts::TAU;
            }
            let mut output = vec![0.0_f32; BLOCK_SIZE * CHANNELS];
            let process = asrc.process(&input, &mut output)?;
            if process.output_frames != BLOCK_SIZE
                || process.input_frames != required
                || output.iter().any(|sample| !sample.is_finite())
            {
                return Err("ASRC sample-path probe was non-finite or shape-mismatched".into());
            }
            finite_sample_blocks = finite_sample_blocks.saturating_add(1);
        }
    }

    if (final_correction_ppm + input_clock_ppm).abs() > 5.0 {
        return Err(format!(
            "final correction did not converge: ppm={input_clock_ppm} correction={final_correction_ppm}"
        )
        .into());
    }

    Ok(json!({
        "input_clock_ppm": input_clock_ppm,
        "trusted_estimate_ppm": trusted_estimate,
        "feedforward_correction_ppm": controller.feedforward_correction_ppm(),
        "final_correction_ppm": final_correction_ppm,
        "duration_seconds": LONG_RUN_SECONDS,
        "ring_capacity_frames": CAPACITY_FRAMES,
        "ring_fill_minimum": minimum_fill,
        "ring_fill_maximum": maximum_fill,
        "finite_asrc_sample_blocks": finite_sample_blocks,
        "bounded": true
    }))
}

fn clock_discontinuity_reacquire_case() -> Result<Value, Box<dyn std::error::Error>> {
    let mut controller = controller()?;
    let before = acquire_clock_estimate(&mut controller, 250.0)?;
    if (before - 250.0).abs() > 5.0 || (controller.feedforward_correction_ppm() + 250.0).abs() > 5.0
    {
        return Err("failed to acquire initial +250 ppm clock epoch".into());
    }

    let reset = controller.observe_clock_frames(0, 0, true)?;
    if reset.is_some() || controller.feedforward_correction_ppm() != 0.0 {
        return Err("clock discontinuity did not clear estimator trust/feed-forward".into());
    }

    let after = acquire_clock_estimate(&mut controller, -250.0)?;
    if (after + 250.0).abs() > 5.0 || (controller.feedforward_correction_ppm() - 250.0).abs() > 5.0
    {
        return Err("failed to reacquire -250 ppm clock epoch after discontinuity".into());
    }

    Ok(json!({
        "initial_estimate_ppm": before,
        "feedforward_cleared_on_discontinuity": true,
        "reacquired_estimate_ppm": after,
        "reacquired_feedforward_ppm": controller.feedforward_correction_ppm(),
        "passed": true
    }))
}

fn clock_out_of_range_case() -> Result<Value, Box<dyn std::error::Error>> {
    let mut controller = controller()?;
    let result = controller.observe_clock_frames(
        input_frames_for_ppm(5_000.0),
        ESTIMATOR_WINDOW_OUTPUT_FRAMES,
        false,
    );
    if result.is_ok() || controller.feedforward_correction_ppm() != 0.0 {
        return Err(
            "out-of-range clock estimate did not fail closed with feed-forward cleared".into(),
        );
    }
    Ok(json!({
        "input_clock_ppm": 5_000.0,
        "rejected": true,
        "feedforward_after_rejection_ppm": controller.feedforward_correction_ppm()
    }))
}

fn adaptive_bridge_hard_latch_case() -> Result<Value, Box<dyn std::error::Error>> {
    let adaptive_config = DuplexBridgeConfig {
        channels: 1,
        capacity_frames: 4_096,
        target_fill_frames: 1_024,
        correction_threshold_frames: 128,
    };
    let controller_config = DriftControllerConfig {
        input_rate: SAMPLE_RATE,
        output_rate: SAMPLE_RATE,
        target_fill_frames: 1_024,
        maximum_correction_ppm: 1.0,
        fatal_saturation_updates: 1,
        ..DriftControllerConfig::default()
    };
    let (producer, mut consumer, status) = create_adaptive_duplex_bridge(
        adaptive_config,
        DuplexFaultPolicy {
            maximum_excursion_frames: 4_096,
            ..DuplexFaultPolicy::default()
        },
        controller_config,
        Box::new(RubatoAsrc::default()),
        BLOCK_SIZE,
    )?;

    producer.push_interleaved(&vec![0.25; 2_048], 1);
    let mut first = vec![1.0; BLOCK_SIZE];
    consumer.read_interleaved(&mut first, 1);
    let faulted = status.snapshot();
    if first.iter().any(|sample| *sample != 0.0)
        || faulted.fault != AdaptiveDuplexFault::Controller
        || faulted.duplex.health != DuplexHealth::Fatal
        || faulted.duplex.underflow_count != 0
    {
        return Err("unsupported adaptive mismatch did not enter a clean fatal mute latch".into());
    }

    producer.push_interleaved(&vec![0.25; BLOCK_SIZE], 1);
    let mut second = vec![1.0; BLOCK_SIZE];
    consumer.read_interleaved(&mut second, 1);
    let still_faulted = status.snapshot();
    if second.iter().any(|sample| *sample != 0.0)
        || still_faulted.fault != AdaptiveDuplexFault::Controller
        || still_faulted.duplex.health != DuplexHealth::Fatal
        || still_faulted.duplex.underflow_count != 0
        || still_faulted.clock_epoch != faulted.clock_epoch
    {
        return Err("latched adaptive fault recovered or mutated itself without control-plane rebuild".into());
    }

    Ok(json!({
        "controller_maximum_correction_ppm": 1.0,
        "fault": format!("{:?}", still_faulted.fault),
        "health": format!("{:?}", still_faulted.duplex.health),
        "first_callback_muted": true,
        "second_callback_muted": true,
        "fault_latched": true,
        "underflow_count": still_faulted.duplex.underflow_count,
        "clock_epoch_unchanged_after_latched_callback": true,
        "requires_control_plane_bridge_rebuild": true,
        "passed": true
    }))
}

fn start_machine(machine: &mut DuplexStateMachine) -> Result<(), Box<dyn std::error::Error>> {
    machine.transition(DuplexStateEvent::StartRequested)?;
    machine.transition(DuplexStateEvent::StreamsStarted)?;
    Ok(())
}

fn reconnect_case() -> Result<Value, Box<dyn std::error::Error>> {
    let mut machine = DuplexStateMachine::default();
    start_machine(&mut machine)?;
    machine.transition(DuplexStateEvent::StreamFault(AudioStreamFault::DeviceLost))?;

    let device_available_after_ms = 5_000_u64;
    let mut elapsed_ms = 0_u64;
    let mut backoffs = Vec::new();
    let mut succeeded_on_attempt = None;

    while machine.can_attempt_recovery() {
        let backoff = machine
            .next_recovery_backoff_ms()
            .ok_or("recovery budget unexpectedly unavailable")?;
        backoffs.push(backoff);
        elapsed_ms = elapsed_ms.saturating_add(backoff);
        machine.transition(DuplexStateEvent::RecoveryRequested)?;
        if elapsed_ms >= device_available_after_ms {
            machine.transition(DuplexStateEvent::RecoverySucceeded)?;
            succeeded_on_attempt = Some(machine.recovery_attempts());
            break;
        }
        machine.transition(DuplexStateEvent::RecoveryFailed)?;
    }

    if backoffs != EXPECTED_BACKOFF_MS
        || succeeded_on_attempt != Some(5)
        || machine.state() != DuplexStreamState::Running
    {
        return Err(format!(
            "bounded reconnect schedule mismatch: backoffs={backoffs:?} attempt={succeeded_on_attempt:?} state={:?}",
            machine.state()
        )
        .into());
    }

    machine.transition(DuplexStateEvent::StableRunObserved)?;
    if machine.recovery_attempts() != 0 {
        return Err("stable run did not clear recovery episode history".into());
    }

    Ok(json!({
        "device_available_after_ms": device_available_after_ms,
        "backoff_ms": backoffs,
        "success_elapsed_ms": elapsed_ms,
        "succeeded_on_attempt": succeeded_on_attempt,
        "stable_run_reset_attempts": machine.recovery_attempts(),
        "final_state": format!("{:?}", machine.state())
    }))
}

fn reconnect_exhaustion_case() -> Result<Value, Box<dyn std::error::Error>> {
    let mut machine = DuplexStateMachine::default();
    start_machine(&mut machine)?;
    machine.transition(DuplexStateEvent::StreamFault(AudioStreamFault::DeviceLost))?;

    let mut elapsed_ms = 0_u64;
    let mut backoffs = Vec::new();
    while machine.can_attempt_recovery() {
        let backoff = machine
            .next_recovery_backoff_ms()
            .ok_or("missing expected bounded recovery backoff")?;
        backoffs.push(backoff);
        elapsed_ms = elapsed_ms.saturating_add(backoff);
        machine.transition(DuplexStateEvent::RecoveryRequested)?;
        machine.transition(DuplexStateEvent::RecoveryFailed)?;
    }

    if backoffs != EXPECTED_BACKOFF_MS
        || machine.recovery_attempts() != 5
        || machine.can_attempt_recovery()
        || machine.next_recovery_backoff_ms().is_some()
        || machine.state() != DuplexStreamState::Faulted
    {
        return Err("reconnect exhaustion did not fail closed after five attempts".into());
    }

    Ok(json!({
        "backoff_ms": backoffs,
        "elapsed_ms": elapsed_ms,
        "attempts": machine.recovery_attempts(),
        "budget_exhausted": true,
        "final_state": format!("{:?}", machine.state())
    }))
}

fn reconnect_flapping_case() -> Result<Value, Box<dyn std::error::Error>> {
    let mut machine = DuplexStateMachine::default();
    start_machine(&mut machine)?;
    let mut observed_backoffs = Vec::new();

    for expected_attempt in 1_u32..=5 {
        machine.transition(DuplexStateEvent::StreamFault(AudioStreamFault::DeviceLost))?;
        let backoff = machine
            .next_recovery_backoff_ms()
            .ok_or("flapping device unexpectedly lost recovery budget early")?;
        observed_backoffs.push(backoff);
        machine.transition(DuplexStateEvent::RecoveryRequested)?;
        machine.transition(DuplexStateEvent::RecoverySucceeded)?;
        if machine.recovery_attempts() != expected_attempt
            || machine.state() != DuplexStreamState::Running
        {
            return Err(
                "successful reopen incorrectly reset flapping-device recovery history".into(),
            );
        }
    }

    machine.transition(DuplexStateEvent::StreamFault(AudioStreamFault::DeviceLost))?;
    if observed_backoffs != EXPECTED_BACKOFF_MS
        || machine.recovery_attempts() != 5
        || machine.can_attempt_recovery()
        || machine.next_recovery_backoff_ms().is_some()
        || machine.state() != DuplexStreamState::Faulted
    {
        return Err("flapping device did not exhaust bounded recovery history fail-closed".into());
    }

    Ok(json!({
        "successful_reopens_without_stable_run": 5,
        "backoff_ms": observed_backoffs,
        "attempts_retained": machine.recovery_attempts(),
        "sixth_fault_recovery_permitted": false,
        "final_state": format!("{:?}", machine.state())
    }))
}
