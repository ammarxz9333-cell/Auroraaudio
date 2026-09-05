# Renderer evaluation evidence

Issue #44 establishes one deterministic Aurora-owned evidence path for renderer comparison.

## Command

```bash
cargo run --release -p aurora-cli --bin aurora-evaluate-renderer -- \
  --renderer all \
  --output-dir output/evaluation
```

`--renderer all` evaluates the accepted built-in geometric binaural baseline and the existing inverse-distance loudspeaker renderer against the same deterministic source trajectory.

The command is a thin wrapper around the shared `aurora_cli::evaluation` module. Later CLI surfaces and automated product tooling must call the same implementation rather than duplicate evaluation logic.

The trajectory performs a full horizontal circle around the listener and includes a vertical excursion. It therefore contains front, side, rear, and elevated source positions without claiming that any renderer correctly reproduces every perceptual cue.

## Artifact schema

All renderer JSON artifacts are wrapped in a stable envelope:

```json
{
  "schema_version": 1,
  "artifact": "performance",
  "payload": {}
}
```

`schema_version` changes only when a consumer-visible artifact contract changes. `artifact` is a stable artifact kind. The payload remains specific to that artifact.

Each renderer writes its own directory containing:

- `rendered-reference.wav`: deterministic gain-routing reference audio;
- `gain-trajectory.json`: per-block speaker gains and gain-power normalization evidence;
- `delay-trajectory.json`: per-block renderer delay values;
- `discontinuities.json`: configured thresholds, observed maxima, and violation counts;
- `performance.json`: p50, p95, p99, maximum processing cost, configured p95 limit, and process peak-memory evidence where available;
- `metadata.json`: renderer identity, layout, configuration hash, commit SHA, latency, evidence semantics, and evidence boundary;
- `command.txt`: the exact command used for reproduction;
- `summary.json`: pass/fail summary and artifact inventory.

The reference WAV applies renderer gains to a deterministic mono sine source. It intentionally does **not** apply the reported propagation delay trajectory. Delay evidence remains separate until a later evaluation slice owns a canonical delay/convolution render path.

## Failure rules

The renderer runner exits non-zero when any selected renderer exceeds a configured evidence threshold:

- non-finite gain or delay values;
- gain-power normalization outside the configured tolerance;
- inter-block gain-step threshold violations;
- inter-block delay-step threshold violations;
- p95 renderer processing time above the configured threshold.

Thresholds are configuration, are included in the configuration hash, and are recorded in generated evidence. They are not silently inferred from the current host.

## Performance interpretation

Timing measures only the `Renderer::render_gains` call. File IO, JSON serialization, WAV generation, and process startup are excluded from block timing.

On Linux, bounded-memory smoke evidence records process peak resident memory from `/proc/self/status` `VmHWM`. On other platforms that field is explicitly unavailable rather than fabricated.

For allocator-level memory investigation, run a release build under Bytehound (or an equivalent external profiler) and retain the profiler capture alongside the JSON artifacts:

```bash
cargo build --release -p aurora-cli --bin aurora-evaluate-renderer
LD_PRELOAD=/path/to/libbytehound.so \
  target/release/aurora-evaluate-renderer \
  --renderer all \
  --output-dir output/evaluation-bytehound
```

The exact Bytehound preload path depends on the local installation. External profiler output is supplemental evidence; CI does not require Bytehound.

## Criterion regression policy

Aurora maintains Criterion microbenchmarks in `aurora-realtime-engine` for renderer gain calculation, full multichannel block processing, geometric fractional delay, DSP kernels, rotating-source updates, adaptive ASRC, deterministic reference convolution, and transport operations.

Linux CI runs the performance benchmark in Criterion quick mode and enforces `config/criterion-policy-v1.json` with `scripts/check_criterion_policy.py`. The versioned policy watches four 12-channel hot paths:

- renderer gain calculation;
- adaptive ASRC at 48 kHz / 256 frames;
- deterministic 128-tap convolution at 12 channels / 256 frames;
- contiguous transport round trip at 12 channels / 256 frames.

The policy uses Criterion's `mean.point_estimate` and explicit absolute ceilings. These ceilings are deliberately wider than a single developer workstation baseline so normal GitHub-host variance does not become a false regression. They are still bounded relative to the 5.333 ms 48 kHz / 256-frame block budget and can be tightened only through a reviewed policy change.

The checker emits `output/evaluation/criterion-policy-summary.json` with schema version, commit SHA, policy path, measured nanoseconds/microseconds, block-budget percentage, configured ceiling, and pass/fail status for every watched benchmark. A missing estimate, malformed estimate, non-finite value, or ceiling violation fails CI. The summary and the exact policy file are uploaded as CI evidence.

This policy is Aurora's machine-readable benchmark baseline contract. Bencher or another external benchmark service may consume it later, but no external service is required for local development or CI correctness.

## Evidence boundary

This runner and benchmark policy are software evidence only. They do not establish physical S6 performance, eARC capture, USB timing, MCU/TDM behavior, DAC output, acoustic response, wireless synchronization, Dolby Atmos/JOC compatibility, true HRTF capability, elevation accuracy, or product readiness.
