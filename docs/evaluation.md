# Unified Renderer Evaluation

Issue `#44` establishes `aurora-evaluation` as Aurora's canonical renderer
evidence path. The crate consumes an already configured Aurora-owned `Renderer`
and does not modify renderer, DSP, realtime, transport, or hardware behavior.
The CLI owns filesystem access and WAV/JSON placement.

## Pipeline

```text
validated scene + mono WAV + direction fixture
                      |
                      v
configured Renderer trait implementation
                      |
          caller-owned gains and scratch
                      |
       existing fractional delay processor
                      |
                      v
WAV + trajectories + metrics + validation JSON
```

The runner preallocates block buffers, trajectory storage, output audio, and
timing storage before its processing loop. Renderer calls continue to use the
accepted caller-owned boundary. The evaluation harness is control-thread and
offline infrastructure; it is not callable from an audio callback.

## Command

```powershell
cargo run -p aurora-cli -- evaluate-renderer `
  --scene fixtures\scenes\geometric_binaural_circle.json `
  --input fixtures\audio\mono_sweep.wav `
  --output-dir output\evaluation `
  --fixture fixtures\evaluation\canonical_renderer_cases.json `
  --renderer all
```

`all` evaluates the existing `GeometricBinaural` and `InverseDistance` modes.
The same library function accepts any future implementation of the existing
`Renderer` trait. Geometric binaural still uses its current blockwise delay
updates; Issue `#44` measures those discontinuities and does not implement
Issue `#43` Checkpoint B.

## Artifacts

Each renderer receives a separate directory containing:

- `rendered.wav`: multichannel 32-bit float WAVE_FORMAT_EXTENSIBLE output;
- `summary.json`: compact CI baseline values;
- `detail.json`: complete schema-1 report;
- `manifest.json`: renderer, scenario, commit, command, hashes, and artifact list;
- `gains.json`: block/channel gain trajectory;
- `delays.json`: block/channel delay trajectory;
- `discontinuity.json`: maximum gain, delay, and adjacent-sample deltas;
- `performance.json`: p50/p95/p99/max cost, memory accounting, and allocation state;
- `validation.json`: ordered criteria and configured limits.

Struct field order and vector order define JSON ordering. The configuration and
audio checksums use explicitly named FNV-1a 64-bit digests for regression
identity, not for security or compatibility guarantees.

## Validation Policy

A run fails after writing its artifacts when any renderer produces NaN or
infinity, clips, exceeds gain-power normalization tolerance, exceeds configured
gain/delay/audio discontinuity limits, exceeds the configured p99 processing
limit, or receives a failed subsystem hook. Semantic records are bounded and
never silently truncated.

Schema-1 limits are 32 channels, 2,880,000 frames, 65,536 blocks, 32 probes, 64
hook records, and 512 UTF-8 bytes per supplied identifier. The default p99
limit is 5,000,000 ns. Performance baselines must be compared on equivalent
machines, toolchains, power plans, and loads. A benchmark run alone is not a
regression decision.

Transport, realtime-safety, and compatibility tests can supply bounded
`HookEvidence` records without introducing dependencies from those subsystems
back to the evaluation crate. No transport implementation is changed by this
milestone.

## Truth And Timing

Deterministic trajectories, checksums, validation, and capacity accounting use
`unit_test` truth semantics: in-process software verification without a virtual
or physical device. CPU durations use `host_api_observation` and vary with the
host. Renderer and configured delay latency are explicitly labeled as reported
or configured, never physically measured.

`steady_state_allocations` is `null` and `not_observed` unless an allocation
audit supplies an actual count. The existing realtime allocation tests remain
the authoritative zero-allocation evidence. The report never fabricates a zero.

## Memory Profiling

`bounded_working_set_bytes` accounts deterministically for retained output,
trajectory, timing, and primary processing-buffer payloads. It is not process
RSS, allocator overhead, or a sampled peak, so `peak_process_memory_bytes`
remains `null` in normal runs.

On Linux, profile a release build externally with Bytehound or an equivalent
allocator profiler:

```bash
cargo build -p aurora-cli --release
bytehound target/release/aurora-cli evaluate-renderer \
  --scene fixtures/scenes/geometric_binaural_circle.json \
  --input fixtures/audio/mono_sweep.wav \
  --output-dir output/evaluation-profile \
  --renderer all
```

Record the profiler version, allocator, Rust toolchain, commit, command, peak
resident/allocated bytes, and report paths. External profiler observations are
host observations, not physical audio measurements. Bytehound is optional and
is not an Aurora dependency.

## Benchmarks

Run the dedicated Criterion benchmark or the complete workspace suite:

```powershell
cargo bench -p aurora-evaluation --bench renderer_evaluation
cargo bench --workspace
```

Criterion data remains under `target/criterion`. The evaluation `summary.json`
is the stable CI-facing baseline input. Bencher remains optional until the
schema and cross-machine comparison policy are accepted.

## Known Limits

- Host timing is not byte-deterministic and is separated from deterministic
  renderer evidence.
- Normal runs do not observe process peak memory or allocation counts.
- Current CLI coverage is limited to existing basic geometric modes; future
  HRTF, convolution, channel-bed comparison, and spectral-transition evidence
  require their separately authorized implementations.
- No evaluation result is hardware validation, physical latency, or an audible
  quality claim.
