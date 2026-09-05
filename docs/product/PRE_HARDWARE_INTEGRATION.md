# Pre-hardware integration

This is an integration branch, not a completed product or a Q995 parity claim.
No hardware acceptance changes. The owner authorized completing and validating
software before physical tests on 2026-09-05.

## Consolidation decisions

- Reuse the additive 3D renderer from #91 and evaluator from #92.
- Reuse canonical capability/evaluation infrastructure already merged in #86–#90.
- Recover only 7.1.4 core semantics and scene from #85, not its alternative
  renderer, capability JSON, evaluation crate or temporary workflow.
- Extract the S6 DSP into aurora-dsp-basic, preserving 80/100/120/20 Hz defaults.
- Keep MSRV tests. Distinguish GNU and MSVC golden numeric baselines explicitly.
- Share WAV export instead of retaining a second RIFF writer in the 3D evaluator.

## Implemented software changes

- Twelve-channel semantic roles, including top-rear channels.
- Canonical-to-WAV channel permutation and duplicate-role rejection.
- Shared output DSP with validated speaker PEQ, trim, polarity and delay.
- Mute after lip-sync delay, smooth delay transitions and nonfinite recovery.
- Hardware-free channel self-test with WAV and machine-readable timing/report.
- Executable stream integration test for S6 postprocessor calibration and failures.
- Full workspace lint/test gates, allocation guards and Criterion output benchmark.

## Linux host execution

An isolated Alpine 3.23.5 x86_64 QEMU guest on Windows passed all 15 selected
S6 C/static gates: source arbitration, final gate, control acknowledgements,
live ingest, FunctionFS compile, portable MCU state machines and wiring audits.
The real shared Rust postprocessor was cross-built for x86_64 Linux MUSL from
this worktree and executed in the guest. Calibration/error handling, broker
ASRC/recovery and manager-to-gate-to-DSP lip-sync tests passed. The latter
checks an exact 2400-frame delay and replay after restarting the DSP.
This is Linux host execution, not a full Linux workspace build or ARM64 proof.

The control socket accepts only lip-sync messages, is mode 0600 and rejects
occupied endpoints. The gate replays absolute delay every 100 ms. Production
local DSP needs its own adapter and optional AURORA_LOCAL_DSP_CONTROL_SOCKET.

## Renderer evaluation conditions

CI evaluates both height layouts at 48 kHz, 40-frame blocks and a two-second
trajectory, matching the configured appliance period. The gain-step limit
remains 0.15. A 5.1.2 two-second run at 256 frames failed (0.186835), while
40 frames passed (0.029918); slowing the 256-frame trajectory to four seconds
also passed (0.095381). This criterion depends on angular displacement per
block and does not establish smooth rendering at arbitrary update rates.
The evaluator still honors fixture block size unless explicitly overridden.

## Local validation checkpoint (2026-09-05)

- `cargo fmt --all --check`: passed.
- Stable GNU `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- Stable GNU and Rust 1.78 GNU `cargo test --workspace --all-features`: passed.
- No tests were removed to make MSRV pass. The shared allocation audit also
  detects deliberate allocation/reallocation and survives panic and concurrent tests.
- `cargo bench --workspace`: passed before the subsequent control/test-only changes.
- Release 5.1.2 and 7.1.4 evaluations at 40 frames: passed, maximum gain steps
  0.029918 and 0.011707 respectively, with zero normalization/nonfinite failures.
- Release 12-channel shared-output self-test: all channels passed.
- Linux C/static gates: 15 passed on the updated source.
- Real Linux DSP calibration/failure and lip-sync/restart tests: passed.
- Broker/ASRC recovery test: passed three consecutive runs after correcting its
  assumption about already-queued pre-reset packets. Old packets remain bounded
  and validated; the first new-epoch packet still must carry the discontinuity flag.

Host timing is load-dependent: the latest self-test measured p50 11.1 us,
p99 13.3 us and maximum 118 us per 40-frame DSP block. This excludes decoder,
ASRC, transport and hardware. Remote CI, full Linux/ARM64 workspace execution
and hardware acceptance are not represented by these local results.

## Remaining pre-hardware work (not claimed complete)

- Production local/Bluetooth/network source adapters; staged packages are insufficient.
- Live decoder telemetry and a user-facing status UI driven by actual telemetry.
- Automatic calibration capture/analysis/application integration (manual validated
  calibration application is available in this branch).
- Thermal supervision, recovery/update packaging, and target-specific system UI.
- Deterministic network transport/receiver integration before any wireless claim.
- ARM64 build and runtime profiling. Host x86 timings do not predict S6 deadlines.

Hardware bring-up, live-service JOC recognition, actual height acoustics, round-trip
latency, temperature and Q995 listening/measurement comparisons remain physical gates.
