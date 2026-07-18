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

- render one Aurora scene to loudspeaker layouts with height channels;
- render the same scene to headphones using measured HRTF data;
- decode open immersive formats into Aurora-owned PCM and scene metadata;
- distribute synchronized audio over ordinary IP networks;
- run receiver nodes on commodity Linux and Raspberry Pi-class hardware;
- support theater-channel distribution and multiroom playback as separate operating modes.

Aurora does not claim Dolby Atmos compatibility without a lawful licensed decoder. The project targets an open immersive listening system using Aurora-owned scene, rendering, timing, transport, evaluation, and orchestration layers.

## 3. Verified baseline

Implemented foundations:

- Aurora-owned scene, renderer, decoder, DSP, realtime-audio, diagnostics, configuration, measurement, and simulation boundaries;
- offline multichannel WAV rendering;
- local realtime engine and CPAL backend;
- horizontal 2D VBAP;
- geometric stereo ITD/ILD prototype;
- Rubato-backed ASRC and drift control;
- deterministic virtual-device and fault simulation;
- functional offline CamillaDSP process adapter;
- placeholder or research adapters for IAMF, Cavern, and truehdd.

Not yet accepted as product capability:

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
3. measurable artifacts such as WAV, JSON trajectory, packet trace, synchronization report, or latency report;
4. reproducible commands;
5. documented limitations and licensing boundaries;
6. CI evidence for the exact commit.

Compilation alone, interface-only crates, placeholders, documents, or simulated claims presented as physical evidence do not count.

## 5. Clean execution sequence

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

### Phase 1 — Establish common product evidence

#### 1A. Unified evaluation runner

**Issue:** `#44`

Create one Aurora-owned evaluation path for every renderer:

- rendered WAV;
- gain and delay trajectories;
- discontinuity report;
- CPU and peak-memory report;
- configuration hash and commit SHA;
- deterministic fixtures for front, side, rear, overhead, and motion.

#### 1B. Capability registry

**Issue:** `#45`

Add a versioned machine-readable registry and `aurora-cli capabilities` output. README capability claims must match the registry. Placeholder, experimental, accepted, and production-ready states must remain distinct.

Phase 1 is required before new engines are called complete.

### Phase 2 — Height-capable loudspeaker rendering

**Issue:** `#38`

Implement an Aurora-owned offline 3D loudspeaker renderer:

- validated loudspeaker triplets;
- 5.1.2 fixture first, then 7.1.4;
- azimuth and elevation gains;
- normalized energy and moving-source continuity;
- deterministic behavior outside the loudspeaker hull;
- WAV and gain-trajectory artifacts through issue `#44` infrastructure.

Recommended crate: `aurora-renderer-vbap3d`.

### Phase 3 — True offline HRTF

#### 3A. SOFA/HRIR dataset backend

**Issue:** `#46`

Create `aurora-hrtf-data` or an equivalent interface:

- user-supplied or redistributable SOFA data;
- optional `libmysofa` backend;
- direction lookup and coordinate conventions;
- sample-rate and resampling policy;
- malformed-dataset validation;
- explicit dataset licensing documentation.

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
- no callback allocation, locks, filesystem access, or dataset loading;
- CPU-budget and underrun tests;
- geometric mode retained as a low-cost fallback.

### Phase 4 — Open immersive input

Split the previous combined issue `#39`; IAMF must be independent from HRTF.

Implement an operational out-of-process IAMF path using `iamf-tools`, `libiamf`, or another reviewed reference implementation:

- decode a legally redistributable sample;
- return Aurora-owned PCM and metadata;
- map metadata to the Aurora scene;
- isolate crashes and malformed inputs;
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
- sender pacing;
- bounded reorder and jitter buffering;
- duplicate and late-packet policy;
- separate audio and control planes;
- localhost and impaired-network integration tests.

Do not claim Wi-Fi synchronization from in-process queues.

### Phase 8 — Linux receiver runtime

**Issue:** `#41`, receiver portion

Implement a headless receiver daemon:

- explicit device selection;
- stream, buffer, drift, and health telemetry;
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

## 6. Dependency order

The mandatory order is:

1. `#43A` geometric binaural stabilization;
2. `#44` evaluation runner;
3. `#45` capability registry;
4. `#38` offline 3D loudspeaker rendering;
5. `#46` SOFA/HRIR backend;
6. offline Aurora HRTF;
7. operational IAMF integration;
8. CamillaDSP runtime hardening;
9. deterministic network simulator;
10. packet transport;
11. Linux receiver and optional PipeWire backend;
12. multiroom mode;
13. external Steam Audio and Snapcast comparisons;
14. physical validation and hardware selection.

IAMF may proceed in parallel after scene and evaluation contracts stabilize, but it must not share a PR with HRTF or networking.

## 7. Inactive or deferred paths

The following are not active product priorities:

- licensed Dolby decoding;
- HDMI/eARC capture;
- Cavern integration without completed license review;
- truehdd product behavior;
- Resonance Audio integration;
- polished consumer UI;
- further governance expansion without an implementation blocker;
- hardware bills of materials based on guessed prices.

## 8. Immediate action

The active PR must implement issue `#43`, Checkpoint A only. After merge, execute issues `#44` and `#45`, then issue `#38`. No agent should skip directly to HRTF, IAMF, networking, or hardware procurement unless the earlier acceptance gates are complete.