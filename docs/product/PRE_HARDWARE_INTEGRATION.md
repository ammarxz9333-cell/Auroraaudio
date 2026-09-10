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

## Direct eARC software checkpoint (2026-09-10)

The feature branch now contains an integrated native direct-eARC software path:
Aurora-owned ALSA S32_LE capture on a dedicated bounded producer thread,
carrier normalization, IEC61937 parsing/telemetry, AC-3/E-AC-3 handling, OpenJOC
admission/render integration, canonical 48 kHz 7.1.4 speaker DSP and native ALSA
output. The legacy STM32/USB source remains an explicit fallback that converges
only after source-local ingest.

Recent correctness fixes include removal of a duplicate decoder-engine method
that could block compilation, hard failure propagation for FFmpeg-worker stdout
read/panic/incomplete-PCM conditions, transport-wide eARC cadence tracking that
is no longer distorted by display/extraction codec filters, and CPAL 0.15.3
Windows ownership/channel-range fixes. Generic elementary AC-3 finite EOF now
has checked finalization: a partial syncword, header or payload is surfaced as a
decoder error instead of being silently discarded. Regression cases cover a
complete final frame, truncated header, truncated payload and non-sync garbage
tail.

IEC61937 data type `0x15` remains transport evidence only. It establishes an
E-AC-3 burst classification, not Atmos/JOC. Software JOC evidence requires
successful JOC admission and successful OpenJOC speaker rendering. Physical
product acceptance additionally requires sustained real eARC/TDM capture and
output evidence.

### Validation state

The current root `Cargo.lock` is stale relative to the workspace and cannot be
accepted as a reproducibility artifact. In particular, the current workspace
contains the direct-eARC/open-decoder crates and OpenJOC/oxideav dependencies
that are absent from the committed lockfile. The lockfile must be regenerated by
a real Cargo 1.85 resolver against the current workspace and then committed; it
must not be hand-edited or reconstructed from an older branch.

The Direct eARC workflow now runs `cargo generate-lockfile`, validates with
`--locked`, and is prepared to upload the resolver-generated `Cargo.lock` as a
short-lived artifact before the final committed-lockfile gate. This provides an
exact recovery path once a GitHub-hosted runner actually starts.

As of 2026-09-10, GitHub Actions jobs for this repository are failing before
step execution. Multiple direct-eARC runs produced a job record with no step
list and no logs, including after changing the runner label from
`ubuntu-24.04` to `ubuntu-latest`. An independent HOA Renderer workflow failed
with the same no-steps/no-logs signature. Therefore no current commit may be
called CI-green, and the failure must not be attributed to Rust build/test code
until a runner actually executes the workflow. Repository/account Actions
provisioning, quota, billing or policy must be checked outside the source tree.

## Remaining pre-hardware work (not claimed complete)

- Regenerate and commit the current workspace `Cargo.lock` with Cargo 1.85, then
  execute the direct-eARC `fmt`/`check`/`test`/`clippy` gates with `--locked`.
- Restore executable GitHub Actions runner service for this repository and retain
  logs/artifacts from an actually executed validation run.
- Production local/Bluetooth/network source adapters; staged packages are insufficient.
- Live decoder telemetry and a user-facing status UI driven by actual telemetry.
- Automatic calibration capture/analysis/application integration (manual validated
  calibration application is available in this branch).
- Thermal supervision, recovery/update packaging, and target-specific system UI.
- Deterministic network transport/receiver integration before any wireless claim.
- ARM64 build and runtime profiling. Host x86 timings do not predict S6 deadlines.

Hardware bring-up, physical eARC/TDM lock and sustained transport, live-service
JOC recognition, actual height acoustics, round-trip latency, temperature and
Q995 listening/measurement comparisons remain physical gates.
