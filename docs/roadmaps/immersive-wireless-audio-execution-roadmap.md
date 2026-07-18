# Aurora Product Execution Roadmap

## 1. Program state

- `program_state`: `PRODUCT_IMPLEMENTATION_ACTIVE`
- `active_program`: `Immersive Audio Product Implementation 1`
- `active_work_item`: issue `#43`, Checkpoint A
- `next_work_item`: issue `#44`
- `governance_mode`: maintenance only
- `default_delivery_unit`: one reviewable implementation PR

Aurora advances through executable product slices. Historical governance and accepted architecture remain valid, but they do not select the next task. New planning-only crates, ADRs, inspection layers, or contract programs require a concrete implementation blocker.

## 2. Product objective

Aurora is an open, low-cost spatial-audio platform that will:

- ingest common audio files into Aurora-owned PCM;
- render one Aurora scene to loudspeaker layouts with height channels;
- render the same scene to headphones using measured HRTF data;
- decode open immersive formats into Aurora-owned PCM and scene metadata;
- distribute synchronized audio over ordinary IP networks;
- run receiver nodes on commodity Linux and Raspberry Pi-class hardware;
- support theater-channel distribution and multiroom playback as separate operating modes.

Aurora does not claim Dolby Atmos compatibility without a lawful licensed decoder. The project targets an open immersive listening system using Aurora-owned PCM, scene, rendering, timing, transport, evaluation, and orchestration layers.

## 3. Verified baseline

Implemented foundations:

- Aurora-owned scene, renderer, decoder, DSP, realtime-audio, diagnostics, configuration, measurement, and simulation boundaries;
- offline multichannel WAV rendering;
- local realtime engine and CPAL backend;
- horizontal 2D VBAP;
- geometric stereo ITD/ILD prototype;
- Rubato-backed ASRC and drift control;
- deterministic virtual-device and fault simulation;
- Criterion-based benchmark foundations;
- structured tracing foundations;
- functional offline CamillaDSP process adapter;
- placeholder or research adapters for IAMF, Cavern, and truehdd.

Not yet accepted as product capability:

- multi-format Symphonia ingestion;
- unified performance and memory regression evidence;
- true HRTF convolution;
- height-capable 3D loudspeaker rendering;
- operational IAMF decoding;
- packetized network audio;
- distributed receiver synchronization on physical devices;
- Raspberry Pi receiver runtime;
- end-to-end multiroom behavior.

## 4. Completion rule

A capability is complete only when all of the following exist:

1. executable production code or an explicitly labeled experimental implementation;
2. deterministic automated tests;
3. measurable artifacts such as WAV, JSON trajectory, packet trace, synchronization report, latency report, or benchmark report;
4. reproducible commands;
5. documented limitations, unsafe boundaries, and licensing obligations;
6. CI evidence for the exact commit.

Compilation alone, interface-only crates, placeholders, documents, or simulated claims presented as physical evidence do not count.

## 5. Cross-cutting Rust foundation rules

These rules apply from the beginning rather than being retrofitted later.

### Audio ingestion

- Use `Symphonia` behind Aurora-owned interfaces for file decoding.
- Support WAV, FLAC, and Ogg/Vorbis first.
- Convert all decoded data into validated Aurora-owned PCM buffers and metadata.
- Perform decoding, resampling preparation, and format probing outside realtime callbacks.
- Keep `hound` only where a small deterministic WAV-only fixture path is simpler and already justified.
- Do not add Rodio as a product audio layer; Aurora requires direct timing, channel-map, and callback control through CPAL.

### Realtime and async separation

- Retain CPAL as the cross-platform realtime audio backend.
- Use Tokio for control-plane services, discovery, socket orchestration, reconnect logic, health APIs, and background tasks.
- Do not expose Tokio runtimes, `async` APIs, mutexes, filesystem operations, or unbounded channels to renderer, DSP, or device callbacks.
- Exchange data with realtime code only through bounded queues and immutable prevalidated snapshots.
- Evaluate `mio` or lower-level sockets only if benchmarks prove Tokio does not meet requirements.

### Performance and memory evidence

- Use Criterion for renderer, ASRC, convolution, packet, and jitter-buffer microbenchmarks.
- Emit machine-readable summaries and explicit regression thresholds.
- Report p50, p95, and p99 where meaningful.
- Document a Linux memory-profiling workflow using Bytehound or an equivalent external profiler.
- Bencher may be evaluated only after Aurora's benchmark artifact schema stabilizes; it remains optional.

### FFI and native code

- Keep every unsafe/native boundary inside a small dedicated adapter crate.
- Do not expose raw pointers or native structs through Aurora-owned public APIs.
- Validate ownership, lengths, nullability, versions, sample rates, and native error codes at the boundary.
- Use a narrow in-process FFI only when the API is small, pinned, reviewed, and latency-sensitive.
- Prefer an out-of-process protocol where crash isolation or ABI instability matters more than call overhead.
- `libmysofa` may use narrow reviewed in-process FFI; IAMF remains out-of-process by default.

### Observability

- Use structured `tracing` fields for stream, receiver, renderer, sequence, buffer fill, drift, ASRC ratio, loss, underrun, and recovery state.
- Do not perform heavy formatting or blocking output inside realtime paths.

## 6. Clean execution sequence

### Phase 0 — Stabilize the landed binaural prototype

**Issue:** `#43`, Checkpoint A

Deliver only:

- rename the current mode to `GeometricBinaural` or another technically accurate name;
- state that it is geometric ITD/ILD, not HRTF;
- validate stereo layout, finite normalized gains, near-zero distance, and delay inputs;
- verify partial-block and repeated-render behavior;
- document dynamic-delay continuity limitations;
- run fmt, Clippy, tests, strict rustdoc, and CI.

Do not include true HRTF, IAMF, networking, or Raspberry Pi work in this PR.

### Phase 1 — Establish common product evidence and input

#### 1A. Unified evaluation and performance runner

**Issue:** `#44`

Create one Aurora-owned evaluation path for every renderer and processing subsystem:

- rendered WAV;
- gain and delay trajectories;
- discontinuity report;
- CPU and peak-memory report;
- configuration hash and commit SHA;
- Criterion summaries with regression thresholds;
- deterministic fixtures for front, side, rear, overhead, and motion;
- documented Bytehound or equivalent memory-profiling procedure.

#### 1B. Multi-format offline audio ingestion

**Issue:** `#48`

Integrate Symphonia behind Aurora-owned PCM contracts:

- WAV, FLAC, and Ogg/Vorbis first;
- sample-rate, channel-count, duration, and source-format metadata;
- deterministic channel-map and resampling policy;
- malformed and truncated media isolation;
- bounded streaming decode;
- CLI integration with the evaluation runner;
- no decoding inside realtime callbacks.

#### 1C. Capability registry

**Issue:** `#45`

Add a versioned machine-readable registry and `aurora-cli capabilities` output. README capability claims must match the registry. Placeholder, experimental, accepted, and production-ready states must remain distinct.

Issue `#48` and issue `#45` may proceed in parallel after issue `#44`, but they must use separate PRs. Phase 1 is required before new engines are called complete.

### Phase 2 — Height-capable loudspeaker rendering

**Issue:** `#38`

Implement an Aurora-owned offline 3D loudspeaker renderer:

- validated loudspeaker triplets;
- 5.1.2 fixture first, then 7.1.4;
- azimuth and elevation gains;
- normalized energy and moving-source continuity;
- deterministic behavior outside the loudspeaker hull;
- WAV, gain-trajectory, CPU, and memory artifacts through issue `#44` infrastructure;
- test media accepted through issue `#48` ingestion where useful.

Recommended crate: `aurora-renderer-vbap3d`.

### Phase 3 — True offline HRTF

#### 3A. SOFA/HRIR dataset backend

**Issue:** `#46`

Create `aurora-hrtf-data` or an equivalent interface:

- user-supplied or redistributable SOFA data;
- optional `libmysofa` backend;
- narrow reviewed FFI isolated in a dedicated adapter crate;
- no raw native types across Aurora APIs;
- direction lookup and coordinate conventions;
- sample-rate and resampling policy;
- malformed-dataset validation;
- explicit dataset, native-version, and licensing documentation.

#### 3B. Aurora HRTF renderer

Continue issue `#43` as separate Checkpoint C PRs after `#46`:

- separate `aurora-renderer-hrtf` crate;
- left/right HRIR convolution;
- directional interpolation;
- azimuth, elevation, front/back, and pinna cues represented by the dataset;
- offline first;
- comparison against `GeometricBinaural` through issue `#44`.

#### 3C. Realtime HRTF

Proceed only after offline acceptance:

- preallocated convolution state;
- no callback allocation, locks, filesystem access, decoding, or dataset loading;
- CPU-budget and underrun tests;
- geometric mode retained as a low-cost fallback.

### Phase 4 — Open immersive input

**Issue:** `#39`

Implement an operational out-of-process IAMF path using `iamf-tools`, `libiamf`, or another reviewed reference implementation:

- decode a legally redistributable sample;
- return Aurora-owned PCM and metadata;
- map metadata to the Aurora scene;
- isolate crashes, timeouts, malformed inputs, and ABI changes;
- produce decoded PCM or rendered WAV evidence;
- keep `production_ready = false` until conformance and legal review pass.

Do not prioritize Cavern or truehdd for the first product release.

### Phase 5 — DSP integration

Keep Aurora basic DSP for bounded core operations. Expand the existing external CamillaDSP adapter rather than recreating a full room-correction engine:

- daemon/process health monitoring;
- generated configuration validation;
- FIR and filter-file validation;
- controlled reload outside the realtime callback;
- latency reporting and failure recovery.

CamillaDSP remains optional and external. Redistribution and license obligations must stay documented.

### Phase 6 — Deterministic network simulation

**Issue:** `#40`, simulator portion first

Model:

- independent receiver clocks;
- offset and frequency drift;
- jitter, burst delay, reorder, duplicate, loss, and disconnect;
- bounded startup and jitter buffers;
- existing DriftController and ASRC behavior;
- recovery after disruption.

Produce seeded synchronization, underrun, overrun, loss, concealment, and resampling reports. Multiroom and theater modes require separate thresholds.

### Phase 7 — Packetized IP audio

**Issue:** `#40`, transport portion after simulator gates

Implement:

- Aurora UDP packet envelope;
- stream, channel, sequence, sample-time, format, and integrity fields;
- Tokio-based control plane and socket orchestration outside realtime callbacks;
- sender pacing;
- bounded reorder and jitter buffering;
- duplicate and late-packet policy;
- separate audio and control planes;
- bounded queues into the realtime engine;
- localhost and impaired-network integration tests;
- Criterion packet and jitter-buffer benchmarks.

Benchmark Tokio before considering `mio`. Do not claim Wi-Fi synchronization from in-process queues.

### Phase 8 — Linux receiver runtime

**Issue:** `#41`, receiver portion

Implement a headless receiver daemon:

- explicit device selection;
- stream, buffer, drift, and health telemetry;
- Tokio-based service orchestration outside the audio callback;
- reconnect and restart behavior;
- ARM cross-compilation CI;
- systemd service files.

Use CPAL first. Evaluate a separate optional PipeWire backend for advanced Linux graph and latency control; do not replace the cross-platform backend prematurely.

### Phase 9 — Multiroom product mode

**Issue:** `#41`, multiroom portion

Implement one synchronized program across multiple rooms first:

- rooms, groups, channel maps, volume, mute, and delay trim;
- deterministic join, leave, pause, resume, and resynchronization;
- independent latency profiles for theater and multiroom modes;
- reproducible multi-process demo.

Use Snapcast only as an external benchmark/reference system. Do not copy or embed its GPL implementation into Aurora.

### Phase 10 — External comparison harnesses

After Aurora-owned renderers work:

- evaluate Steam Audio as an optional external reference for HRTF and room simulation;
- compare WAVs, impulse responses, CPU, latency, and cue coverage;
- keep it outside Aurora core and behind explicit licensing review;
- keep Resonance Audio out of the active dependency plan because it is archived.

Reference engines validate Aurora; they do not become the product architecture.

### Phase 11 — Physical validation and hardware selection

Only after software reports exist, test the smallest falsifiable physical setup:

- one coordinator;
- two receiver nodes;
- two independent DAC clocks.

Before recommending a complete system, measure:

- coordinator CPU per object and channel;
- receiver CPU and memory;
- network bitrate and safe jitter-buffer range;
- end-to-end latency;
- inter-receiver skew;
- restart and reconnection behavior;
- required DAC and amplification topology.

A full 5.1.2 purchase is not the first experiment.

## 7. Mandatory dependency order

1. `#43A` geometric binaural stabilization;
2. `#44` evaluation, benchmark, and memory-evidence runner;
3. `#48` Symphonia ingestion and `#45` capability registry, in parallel but separate PRs;
4. `#38` offline 3D loudspeaker rendering;
5. `#46` SOFA/HRIR backend and FFI boundary;
6. offline Aurora HRTF;
7. operational IAMF integration;
8. CamillaDSP runtime hardening;
9. deterministic network simulator;
10. Tokio-based packet transport outside realtime boundaries;
11. Linux receiver and optional PipeWire backend;
12. multiroom mode;
13. external Steam Audio and Snapcast comparisons;
14. physical validation and hardware selection.

IAMF may proceed in parallel after scene, ingestion, and evaluation contracts stabilize, but it must not share a PR with HRTF or networking.

## 8. Inactive or deferred paths

The following are not active product priorities:

- licensed Dolby decoding;
- HDMI/eARC capture;
- Rodio as Aurora's playback architecture;
- PortAudio as a duplicate default backend;
- `mio` without measured evidence that Tokio is insufficient;
- mandatory Bencher service integration before benchmark schema stability;
- Cavern integration without completed license review;
- truehdd product behavior;
- Resonance Audio integration;
- polished consumer UI;
- further governance expansion without an implementation blocker;
- hardware bills of materials based on guessed prices.

## 9. Immediate action

The active PR must implement issue `#43`, Checkpoint A only. After merge, execute issue `#44`; then issues `#48` and `#45` may proceed in parallel on separate branches. Issue `#38` follows only after these common foundations are accepted. No agent should skip directly to HRTF, IAMF, networking, or hardware procurement unless the earlier acceptance gates are complete.