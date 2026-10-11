# Aurora Acoustic Auto-Aim: manually triggered V1

Status: SOFTWARE STATE MACHINE ONLY. The phone microphone frontend, actual motor control, acoustic measurements, wireless transport and a production UI are NOT implemented or physically proven.

## Exact intended interaction

1. User puts the phone at ear height at the listening position, opens Aurora and presses Recalibrate.
2. The app asks for microphone permission and establishes a one-run ephemeral phone session ID.
3. Aurora sequentially commands candidate motor angles. The motor adapter independently checks safe end stops, obstruction, feedback and settled position before acknowledging a move.
4. Aurora plays a fixed, safe-volume 48 kHz exponential sine sweep from ONE speaker at a time. The existing aurora-measurement stimulus generator supplies deterministic reference samples.
5. A mobile audio frontend records with verifiably controlled gain and processing. DSP deconvolves recorded reference/chirp into an impulse response and estimates arrival onset. Neither raw recordings nor estimated network delay can be substituted for this.
6. An on-demand Rust AutoAimSession checks capture identity, format and quality, then ranks the tested angles.
7. The proposed preset is moved to and verified by hardware, persisted atomically, then acknowledged to the session. Cancel/disconnect/error aborts the session and requires restoring previous angles.
8. Microphone capture closes. NO CONTINUOUS TRACKING, NO background measurement, and NO automatic motor movement after the run.

## Engine API

Source: crates/aurora-measurement/src/auto_aim.rs

- begin_from_calibrate_button(plans, ephemeral_phone_session_id) -> session + MoveMotor
- motor_settled(module_id, angle_deg) -> PlaySweep
- submit_impulse_response(module_id, angle_deg, deconvolved_ir, onset_frame, quality) -> MoveMotor or ProposePreset
- saved() -> Complete ONLY after externally verified motor movement and durable preset write
- abort() -> RestorePrevious (must be carried out by host)

The driver MUST handle timeouts, stop/mute, obstruction and rollback; the Rust state machine never directly drives the motor or opens the phone microphone. The test action is a command, not an acknowledgement of successful playback.

## Scoring and epistemic limits

For a 48 kHz deconvolved IR, windows measured from first arrival are direct 0–5 ms, early 5–60 ms, late 60–200 ms. V1 scores a fraction of energy in the desired window, penalizing late energy. It NEVER chooses the simply loudest angle. DirectArrival prioritizes first-arrival energy. EarlyReflection prioritizes an early reflected component. This is an experimental heuristic, not a validated perceptual objective.

A single phone microphone CANNOT uniquely determine the direction of an acoustic arrival, nor whether an early reflection originated from a wall or ceiling. Therefore the height-speaker/Atmos result is not established. Room placement may also affect phase, frequency response, early/late energy, and listening quality; acoustically meaningful optimization requires multi-position measurements and real-world subjective tests.

To admit a capture V1 requires the same nonblank ephemeral phone session, 48 kHz reference/IR, at least 20 dB signal-to-noise ratio, at most 0.1% clipped samples, a deconvolved finite 200 ms IR, and verified unprocessed recording. In many phone/browser configurations automatic gain, NS and AEC cannot be reliably disabled: such captures MUST NOT claim unprocessed_capture = true without platform evidence. Network packets merely carry commands and evidence; Wi-Fi does NOT measure sound reflection angles.

## Safety and acceptance

- Software unit tests: manual entry, acknowledgement order, candidate optimization, failed-capture rejection, duplicate/unsafe angles, cancel and rollback.
- Pending Android/iOS phone app: microphone permission/close, unprocessed capture and chirp deconvolution verified, endpoint session identity and local Wi-Fi.
- Pending mechanical: actual feedback, encoder/servo driver, torque limits, cable protection, obstruction/limit switch and hardware mute.
- Pending acoustic: physical baseline vs recommended angles for repeatability in a real room, across multiple listening positions and repeat trials.
- No physical pass, latency/synchronization claim, or Dolby/Atmos parity claim from software-only tests.

This is an experimental optional module and is not on the physical eARC/decoder critical path.
