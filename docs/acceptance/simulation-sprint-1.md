# Simulation Sprint 1 Acceptance

## Status

**ACCEPTED and frozen on 2026-07-16.**

- Accepted implementation commit: `cf7979b6c47d81387ab36d6e05b598d70cfe42cb`
- Acceptance tag: `simulation-sprint-1-accepted`
- Scope: deterministic virtual audio hardware and full-system simulation validation
- Physical hardware validation: pending
- Later milestones started by this procedure: none

The accepted implementation commit identifies the exact verified source tree.
The annotated acceptance tag identifies the subsequent documentation-only
acceptance commit. This two-commit record is required because the repository had
no commit history before acceptance and a commit cannot contain its own hash.

## Acceptance Criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| Formatting | PASS | `cargo fmt --all --check` |
| Linting | PASS | workspace, all targets, all features, warnings denied |
| Tests | PASS | 112 passed, 0 failed, 5 explicitly ignored hardware tests |
| Benchmarks | PASS | complete release workspace benchmark exited successfully |
| USB 7.1 virtual 24-hour run | PASS | bounded, finite, zero underrun/overflow/drop |
| Virtual loopback latency | PASS | 777-frame truth recovered with zero median error |
| 7.1 routing | PASS | canonical order, unique routing, silence, gain, polarity |
| Fault fixtures | PASS | all six bounded and finite with expected fault effects |
| Determinism | PASS | repeated checksums matched |
| Scheduler allocation | PASS | zero steady-state allocations |
| Long-run memory | PASS | zero steady-state memory growth |
| Production unsafe | PASS | no simulator production unsafe; allocator audit is test-only |
| Dependencies | PASS | no new external dependency introduced for the simulator |
| Device ambiguity | PASS | structured `AmbiguousDeviceSelector` error |
| Public API documentation | PASS | rustdoc with `-D missing_docs` |
| Simulation terminology | PASS | reports use explicit `simulated_*` sources |

## Commands Executed

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo bench --workspace
$env:RUSTDOCFLAGS='-D missing_docs'; cargo doc -p aurora-realtime-audio-sim --no-deps

cargo run -p aurora-cli --all-features -- simulate-duplex --profile usb-7-1 --duration-hours 24 --seed 12345 --report output\simulation\acceptance-usb-7-1-24h.json
cargo run -p aurora-cli --all-features -- simulate-latency --profile usb-7-1 --loopback-delay-frames 777 --jitter-frames 3 --noise-db -60 --seed 42
cargo run -p aurora-cli --all-features -- simulate-output-validation --profile usb-7-1 --layout 7.1 --report output\simulation\acceptance-validation-7-1.json
```

Each JSON fixture under `fixtures/simulation/fault_scenarios` was executed with
`simulate-duplex --duration-hours 1 --seed 12345 --fault-script <fixture>`.

## Test And Simulation Results

- Workspace tests: `112 passed`, `0 failed`, `5 ignored`.
- Ignored tests are explicitly identified as requiring audio hardware, selected
  endpoints, a physical loopback cable, disconnect operation, or a one-hour soak.
- 24-hour source: `simulated_virtual_audio_hardware`.
- 24-hour deterministic checksum: `a39ef2203486cb2d`.
- Sample-pipeline checksum: `2cc43de3374b1db6`.
- Ring capacity: `16384` frames; steady-state memory growth: `0` bytes.
- Underruns / overflows / dropped frames: `0 / 0 / 0`.
- All report and sample-pipeline finite flags: `true`.
- Virtual latency source: `simulated_virtual_loopback_truth`.
- Virtual latency truth / median estimate: `777 / 777` frames.
- Routing source: `simulated_channel_routing_truth`.
- Routing checksum: `1388dc3ba1e02b73`.

Fault report checksums:

- output loss: `d70db3c8a0634b00`
- input callback stall: `afb132170cd1a22a`
- callback size change: `db32d404dbb21e38`
- clock jump: `a5b97b2a1567d209`
- unsupported format: `e3b41cd67a5fdb60`
- repeated stream errors: `3dd1783091940a17`

## Benchmarks

Release mode, 48 kHz, 256-frame full block:

| Channels | Median | Block budget |
| ---: | ---: | ---: |
| 2 | 4.589 us | 0.086% |
| 6 | 10.268 us | 0.193% |
| 8 | 13.090 us | 0.245% |
| 12 | 18.599 us | 0.349% |

The USB 7.1 one-hour virtual simulation benchmark median was `67.858 ms`
(range `63.383..72.175 ms`). Criterion does not provide p95/max for this
benchmark. Complete workspace passes showed host power/thermal variability;
the final stable full-block medians remained close to the master baseline and
no source change introduced a benchmark regression.

## Generated Reports

- `output/simulation/acceptance-usb-7-1-24h.json`
- `output/simulation/acceptance-validation-7-1.json`
- `output/simulation/acceptance-fault-output_loss.json`
- `output/simulation/acceptance-fault-input_callback_stall.json`
- `output/simulation/acceptance-fault-callback_size_change.json`
- `output/simulation/acceptance-fault-clock_jump.json`
- `output/simulation/acceptance-fault-unsupported_format.json`
- `output/simulation/acceptance-fault-repeated_stream_errors.json`

These generated local reports are excluded by `.gitignore` and can be recreated
from the commands above.

## Known Limitations

- Results are simulated facts, not physical measurements.
- The simulator cannot prove real driver scheduling, physical latency, clock
  quality, analog noise, USB behavior, endpoint stability, or hardware recovery.
- The accelerated model includes a bounded 128-callback real sample-path probe;
  it does not process every frame of a 24-hour run through wall-clock DSP.
- Benchmark timing depends on host power and thermal state.
- Hardware validation remains explicitly pending.

No Simulation Sprint 2, hardware integration, or later milestone work was
started during acceptance.
