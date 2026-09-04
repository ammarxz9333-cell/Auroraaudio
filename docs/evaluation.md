# Unified Renderer Evaluation

Issue `#44` establishes `aurora-evaluation` as Aurora's canonical renderer
evidence path. The crate consumes an already configured Aurora-owned `Renderer`
and does not modify renderer, DSP, realtime, transport, or hardware behavior.
The CLI owns filesystem access and WAV/JSON placement.

The operational rules for coding agents are in
[`CODEX_PROJECT_EXECUTION_REFERENCE.md`](../CODEX_PROJECT_EXECUTION_REFERENCE.md).

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
WAV + deterministic evidence + host observations
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
updates. Issue `#44` records that limitation and does not implement Issue `#43`
Checkpoint B.

## Artifact classes

Each renderer receives a separate directory containing:

- `rendered.wav`: multichannel 32-bit float WAVE_FORMAT_EXTENSIBLE output;
- `summary.json`: deterministic compact regression summary;
- `detail.json`: complete schema-2 run report, including host timing;
- `manifest.json`: renderer, scenario, commit, command, fingerprints, and artifact list;
- `gains.json`: block/channel gain trajectory;
- `delays.json`: block/channel delay trajectory;
- `discontinuity.json`: maximum gain, delay, and adjacent-sample deltas;
- `performance.json`: host timing, partial capacity accounting, and allocation state;
- `validation.json`: ordered deterministic criteria and required/optional classification.

`summary.json` intentionally excludes host timing. `detail.json` and
`performance.json` are stable in schema and field order but are not expected to
be byte-identical because host timing varies.

The configuration and audio fields named `*_fnv1a64` are deterministic 64-bit
regression fingerprints. They are useful for detecting unintended changes in
canonical fixtures. They are not cryptographic integrity digests and must not be
used as security or long-term artifact-authenticity proof.

## Aggregate validation policy

Each validation finding declares whether it is required.

- any required `fail` makes the aggregate `fail`;
- when no required finding fails but a required finding is `not_observed`, the
  aggregate is `not_observed` and must not be described as PASS;
- optional findings remain visible but do not control the required aggregate;
- malformed or non-finite threshold inputs are rejected before evaluation.

Current required deterministic checks cover finite output, clipping, gain-power
normalization, gain-step bounds, delay-step bounds, allocation evidence, and any
required external hook. Host timing is advisory and does not control the
canonical deterministic aggregate.

The adjacent rendered-sample delta is reported as `audio_sample_delta_proxy`.
It is optional because a raw adjacent-sample delta cannot distinguish a click
from legitimate high-frequency waveform slope, an impulse, or an input
transition. It must not be cited as proof of click-free output. Issue `#43`
Checkpoint B requires a renderer-specific boundary-local or reference-based
metric.

Schema-2 limits are 32 channels, 2,880,000 frames, 65,536 blocks, 32 probes, 64
hook records, and 512 UTF-8 bytes per supplied identifier.

## Host performance evidence

The timed region is the `Renderer::render_gains` call. It does not include the
complete offline render path, delay processing, WAV writing, JSON serialization,
or filesystem I/O. Fields therefore use renderer-call terminology.

The CLI threshold `--max-renderer-p99-ns` is an advisory host observation. The
report records the limit and whether the observed p99 met it, but generic CI host
timing does not decide deterministic PASS. Meaningful performance regression
policy requires equivalent runner class, toolchain, power plan, background load,
and benchmark method.

No host timing value is physical latency or callback deadline proof.

## Allocation evidence

`steady_state_allocations` is `null` and `not_observed` unless an allocation
audit supplies an actual count. The report never fabricates zero. Existing
realtime allocation tests remain the authoritative zero-allocation evidence for
the accepted callback path.

An agent adding an allocation result must identify the exact audited function,
warm-up procedure, allocator, thread scope, iteration count, and processing
status checks.

## Partial memory-capacity accounting

`bounded_working_set_bytes` is retained for schema continuity but is explicitly
classified with `coverage = partial_primary_payloads`. It currently accounts for:

- retained output-audio payload;
- trajectory storage payload;
- timing-sample storage payload;
- primary block input/output payload.

It does not claim to include:

- renderer-owned internal state;
- `RendererScratch` payload unless separately added to accounting;
- `DelayProcessor` internal storage;
- gains, delays, previous-state, or probe vectors;
- strings, JSON values, serialization buffers, WAV writer buffers;
- allocator metadata or fragmentation;
- process RSS or peak resident memory.

`peak_process_memory_bytes` therefore remains `null` unless supplied by an
external profiler. Do not describe partial accounting as actual working set or
memory usage.

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

Record profiler version, allocator, Rust toolchain, commit, exact command, peak
resident/allocated bytes, and report paths. External profiler observations are
host observations, not physical audio measurements. Bytehound is optional and
is not an Aurora dependency.

## Truth-source separation

Deterministic trajectories, regression fingerprints, deterministic validation,
and partial capacity accounting use `unit_test` semantics: in-process software
verification without a virtual or physical device.

CPU durations use `host_api_observation`. Renderer-reported and configured delay
latency are explicitly labeled as reported/configured, never physically measured.
Simulation evidence must use deterministic-simulation terminology. Physical
evidence may be produced only by a separately defined hardware procedure.

## Benchmarks

Run the dedicated Criterion benchmark or the complete workspace suite:

```powershell
cargo bench -p aurora-evaluation --bench renderer_evaluation
cargo bench --workspace
```

Criterion data remains under `target/criterion`. `summary.json` is suitable for
deterministic regression comparison. Host performance comparison must use the
host-observation fields and an explicitly controlled comparison policy.

## Required review tests

Before Issue `#44` can merge, independent review must verify:

- repeated deterministic summaries are equal;
- full reports may differ only in explicitly host-variable fields;
- missing required evidence never becomes PASS;
- optional unobserved hooks do not fail the required aggregate;
- non-finite renderer output fails;
- host timing cannot fail the deterministic aggregate;
- partial memory accounting is labeled partial;
- artifact and manifest references are consistent;
- CLI exit behavior for failed and incomplete required evaluations is intentional
  and tested;
- Linux, Windows, and MSRV checks pass at the reviewed head SHA.

## Known limits

- Current CLI coverage is limited to existing basic geometric modes.
- GeometricBinaural is not HRTF and is not yet click-free for moving delay.
- Normal runs do not observe process peak memory or allocation counts.
- The current memory byte total is partial capacity accounting.
- FNV-1a values are regression fingerprints, not integrity proofs.
- Host timing is advisory and not deterministic.
- No result is hardware validation, physical latency, room-acoustic validation,
  or an audible-quality claim.
