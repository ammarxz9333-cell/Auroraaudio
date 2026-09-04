# Renderer evaluation evidence

Issue #44 establishes one deterministic evidence path for Aurora renderers. This document describes the first executable slice.

## Command

```bash
cargo run --release -p aurora-cli --bin aurora-evaluate-renderer -- \
  --renderer all \
  --output-dir output/evaluation
```

`--renderer all` evaluates the accepted built-in geometric binaural baseline and the existing inverse-distance loudspeaker renderer against the same deterministic source trajectory.

The trajectory performs a full horizontal circle around the listener and includes a vertical excursion. It therefore contains front, side, rear, and elevated source positions without claiming that any renderer correctly reproduces every perceptual cue.

## Artifacts

Each renderer writes its own directory containing:

- `rendered-reference.wav`: deterministic gain-routing reference audio;
- `gain-trajectory.json`: per-block speaker gains and gain-power normalization evidence;
- `delay-trajectory.json`: per-block renderer delay values;
- `discontinuities.json`: configured thresholds, observed maxima, and violation counts;
- `performance.json`: p50, p95, p99, maximum processing cost, and process peak-memory evidence where available;
- `metadata.json`: renderer identity, layout, configuration hash, commit SHA, latency, and evidence semantics;
- `command.txt`: the command used for reproduction;
- `summary.json`: pass/fail summary and artifact inventory.

The reference WAV applies renderer gains to a deterministic mono sine source. It intentionally does **not** apply the reported propagation delay trajectory. Delay evidence remains separate until a later evaluation slice owns a canonical delay/convolution render path.

## Failure rules

The runner exits non-zero when any selected renderer exceeds a configured evidence threshold:

- non-finite gain or delay values;
- gain-power normalization outside the configured tolerance;
- inter-block gain-step threshold violations;
- inter-block delay-step threshold violations;
- p95 renderer processing time above the configured threshold.

Thresholds are command-line configuration and are recorded in the generated evidence.

## Performance interpretation

Timing measures only the `Renderer::render_gains` call. File IO, JSON serialization, WAV generation, and process startup are excluded from block timing.

On Linux, process peak resident memory is reported from `/proc/self/status` `VmHWM`. On other platforms the in-process peak-memory field is unavailable rather than fabricated.

For allocator-level memory investigation, run a release build under Bytehound (or an equivalent external profiler) and retain the profiler capture alongside the JSON artifacts. One example workflow is:

```bash
cargo build --release -p aurora-cli --bin aurora-evaluate-renderer
LD_PRELOAD=/path/to/libbytehound.so \
  target/release/aurora-evaluate-renderer \
  --renderer all \
  --output-dir output/evaluation-bytehound
```

The exact Bytehound preload path depends on the local installation. External profiler output is supplemental evidence; CI does not require Bytehound.

## Evidence boundary

This runner is software evidence only. It does not establish physical S6 performance, eARC capture, USB timing, MCU/TDM behavior, DAC output, acoustic response, wireless synchronization, Dolby Atmos/JOC compatibility, true HRTF capability, elevation accuracy, or product readiness.

Future #44 slices may move this logic behind `aurora-cli evaluate-renderer`, add stable benchmark-baseline schemas, add canonical delayed/convolved WAV rendering, and broaden renderer fixtures. Those changes must preserve the same explicit evidence boundary.
