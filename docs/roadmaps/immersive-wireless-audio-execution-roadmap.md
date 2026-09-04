# Aurora Product Execution Roadmap

## 1. Program state

- `program_state`: `PRODUCT_IMPLEMENTATION_ACTIVE`
- `active_program`: `Immersive Audio Product Implementation 1`
- `completed_renderer_prerequisite`: issue `#43`, Checkpoint A, PR `#57`, merge commit `44e3df84cb2082f079e83622f8e950cc66da1b8a`
- `active_renderer_work_item`: issue `#44`
- `next_renderer_work_item`: issue `#45`
- `s6_appliance_baseline`: PR `#83`, merge commit `5aa97d10df9e8af28d5afea34c24b90bad6b47db`
- `s6_appliance_state`: `HOST_SOFTWARE_VALIDATED_PHYSICAL_GATES_OPEN`
- `governance_mode`: maintenance only
- `default_delivery_unit`: one reviewable implementation PR

Aurora advances through executable product slices. The renderer/product-evidence sequence and the Galaxy S6 appliance/hardware-enablement sequence are coordinated but separate lanes. Historical governance and accepted architecture remain valid, but they do not select the next task. New planning-only crates, ADRs, inspection layers, or contract programs require a concrete implementation blocker.

The S6 appliance baseline landed through PR #83. That integration does not satisfy, skip, or reorder the renderer acceptance gates below, and host/software evidence from that lane must not be presented as physical hardware validation.

## 2. Product objective

Aurora is an open, low-cost spatial-audio platform that will:

- render one Aurora scene to loudspeaker layouts with height channels;
- render the same scene to headphones using measured HRTF data;
- decode open immersive formats into Aurora-owned PCM and scene metadata;
- distribute synchronized audio over ordinary IP networks;
- run receiver nodes on commodity Linux and Raspberry Pi-class hardware;
- support theater-channel distribution and multiroom playback as separate operating modes;
- support a dedicated Galaxy S6 appliance path with managed source routing and realtime-MCU audio I/O where accepted by the corresponding hardware gates.

Aurora does not claim Dolby Atmos compatibility without a lawful licensed decoder. The project targets an open immersive listening system using Aurora-owned scene, rendering, timing, transport, evaluation, and orchestration layers.

## 3. Verified baseline

Implemented foundations:

- Aurora-owned scene, renderer, decoder, DSP, realtime-audio, diagnostics, configuration, measurement, and simulation boundaries;
- offline multichannel WAV rendering;
- local realtime engine and CPAL backend;
- horizontal 2D VBAP;
- accepted `GeometricBinaural` Checkpoint A baseline from PR `#57`, using geometric ITD/ILD and per-ear distance weighting with explicit non-HRTF labeling;
- Rubato-backed ASRC and drift control;
- deterministic virtual-device and fault simulation;
- functional offline CamillaDSP process adapter;
- placeholder or research adapters for IAMF, Cavern, and truehdd;
- S6 appliance bootstrap, reproducible build/runtime staging, and delivery packaging logic;
- host-tested S6 source manager and exclusive HDMI/eARC/local final source gate;
- host-tested live IEC61937 immersive ingest path and 7.1.4 PCM transport framing;
- host-tested portable realtime-MCU transport, IEC61937 carrier normalization, CONFIG/layout handling, XRUN recovery, and USB-reset behavior;
- S6 physical bring-up, flash, protocol, and target contracts that explicitly separate host proof from hardware proof.

Not yet accepted as product capability:

- unified renderer evaluation/artifact infrastructure under issue `#44`;
- true HRTF convolution;
- height-capable 3D loudspeaker rendering;
- operational IAMF decoding;
- packetized network audio;
- distributed receiver synchronization on physical devices;
- Raspberry Pi receiver runtime;
- end-to-end multiroom behavior;
- physical SM-G920F boot/display/touch/Wi-Fi acceptance;
- physical HDMI/eARC carrier capture and realtime-MCU target HAL acceptance;
- physical USB timing, TDM/DMA, DAC/amplifier/speaker output, thermal, xrun, and end-to-end latency acceptance;
- live streaming-service DD+ JOC to measured 7.1.4 acceptance.

For detailed S6 status, `platform/s6/COMPONENT_STATUS.md` is authoritative.

## 4. Completion rule

A capability is complete only when all of the following exist:

1. executable production code or an explicitly labeled experimental implementation;
2. deterministic automated tests;
3. measurable artifacts such as WAV, JSON trajectory, packet trace, synchronization report, or latency report;
4. reproducible commands;
5. documented limitations and licensing boundaries;
6. CI evidence for the exact commit;
7. physical measurement evidence when the capability claim depends on physical hardware behavior.

Compilation alone, interface-only crates, placeholders, documents, host-only tests, or simulated claims presented as physical evidence do not count.

## 5. Renderer/product-evidence execution sequence

### Phase 0 — Geometric binaural stabilization — COMPLETED

**Issue:** `#43`, Checkpoint A  
**Accepted through:** PR `#57`, merge commit `44e3df84cb2082f079e83622f8e950cc66da1b8a`

Accepted baseline:

- canonical mode name `GeometricBinaural`;
- explicit classification as geometric ITD, geometric ILD, and per-ear geometric distance weighting followed by power normalization;
- explicit statement that it is not an HRTF renderer and has no HRIR data, convolution, pinna cues, or elevation cues;
- stereo layout, finite/bounded delay, polarity, normalization, near-zero-distance, partial-block, repeated-render, and allocation tests;
- strict fmt, Clippy, tests, rustdoc, MSRV, benchmark, actionlint, and artifact checks reported by PR #57.

Checkpoint B dynamic-delay improvement and true HRTF remain separate future work. Completion of Phase 0 does not imply height rendering, HRTF, or physical hardware validation.

### Phase 1 — Establish common product evidence

#### 1A. Unified evaluation runner — ACTIVE

**Issue:** `#44`

Create one Aurora-owned evaluation path for every renderer:

- rendered WAV;
- gain and delay trajectories;
- discontinuity report;
- CPU and peak-memory report;
- configuration hash and commit SHA;
- deterministic fixtures for front, side, rear, overhead, and motion.

A prior draft implementation PR may exist, but issue `#44` is not complete until accepted and merged against the current canonical branch with exact-commit CI evidence.

#### 1B. Capability registry — NEXT

**Issue:** `#45`

Add a versioned machine-readable registry and `aurora-cli capabilities` output. README capability claims must match the registry. Placeholder, experimental, accepted, host-validated, physically validated, and production-ready states must remain distinct.

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

### Phase 11 — Minimum physical system validation and hardware selection

After the relevant software reports exist, test the smallest falsifiable distributed setup:

- one coordinator;
- two receiver nodes;
- two independent DAC clocks.

Before recommending a complete distributed system, measure:

- coordinator CPU per object and channel;
- receiver CPU and memory;
- network bitrate and safe jitter-buffer range;
- end-to-end latency;
- inter-receiver skew;
- restart and reconnection behavior;
- required DAC and amplification topology.

A full 5.1.2 purchase is not the first distributed-network experiment.

This phase does not prevent dedicated S6 appliance physical bring-up from occurring earlier. S6 flash/boot/eARC/USB/TDM/DAC/thermal acceptance is a separate evidence lane governed by the S6 bring-up documents and may proceed whenever the required hardware is available.

## 6. Parallel S6 appliance and realtime-MCU lane

PR `#83` established the integrated S6 host/software baseline. Future work in this lane must stay in dedicated PRs and use `platform/s6/COMPONENT_STATUS.md` as the evidence matrix.

Allowed next work includes:

- correcting defects in the landed source manager, source gate, live-ingest, protocol, build, packaging, or realtime-MCU portable layers;
- completing missing local-music, Bluetooth, or later network source adapters when their own roadmap dependencies are satisfied;
- executing flash/boot/display/touch/Wi-Fi gates on a physical SM-G920F;
- integrating and validating the selected realtime-MCU target USB/eARC/TDM HAL on hardware;
- measuring real carrier handling, xruns, latency, thermal behavior, DAC/amplifier output, and live JOC-to-7.1.4 behavior.

Promotion rules:

- `HOST-PASS` means host/software evidence only;
- `BUILD-SCRIPT` means reproducible build logic, not boot proof;
- `STAGED` means integrated payload, not end-to-end runtime proof;
- `HW-BLOCKED` remains open until accepted physical evidence exists;
- no S6 lane result may be called flash-ready, plug-and-play, production-ready, physically validated, or live streaming Atmos/JOC validated while required gates remain open.

S6 appliance work does not authorize new Dolby codec implementation or any claim of licensed Dolby compatibility.

## 7. Renderer dependency order

Completed:

- `#43A` geometric binaural stabilization — PR `#57` merged.

Mandatory current order:

1. `#44` evaluation runner;
2. `#45` capability registry;
3. `#38` offline 3D loudspeaker rendering;
4. `#46` SOFA/HRIR backend;
5. offline Aurora HRTF;
6. operational IAMF integration;
7. CamillaDSP runtime hardening;
8. deterministic network simulator;
9. packet transport;
10. Linux receiver and optional PipeWire backend;
11. multiroom mode;
12. external Steam Audio and Snapcast comparisons;
13. minimum distributed physical validation and hardware selection.

IAMF may proceed in parallel after scene and evaluation contracts stabilize, but it must not share a PR with HRTF or networking. Dedicated S6 maintenance and physical-bring-up work may proceed in parallel without being treated as completion of renderer gates.

## 8. Inactive or deferred paths

The following are not active product priorities:

- licensed Dolby decoding;
- new HDMI/eARC product expansion beyond the landed S6 baseline before its physical bring-up gates are accepted;
- Cavern integration without completed license review;
- truehdd product behavior;
- Resonance Audio integration;
- polished consumer UI until its underlying runtime contracts are ready;
- further governance expansion without an implementation blocker;
- hardware bills of materials based on guessed prices.

The existing S6 HDMI/eARC host-validated path is **not** inactive; it is an integrated appliance baseline with open physical acceptance gates.

## 9. Immediate action

For the renderer/product-evidence lane, the active implementation task is issue `#44`. After it is accepted on the current canonical branch, execute issue `#45`, then issue `#38`. Do not return to #43A as if it were unfinished; PR #57 already accepted that checkpoint. No renderer agent should skip directly to HRTF, IAMF, networking, or hardware procurement unless the earlier renderer gates are complete.

For the S6 appliance lane, retain the PR #83 baseline, fix regressions in dedicated PRs, and execute the documented physical bring-up gates when hardware is available. Do not describe host CI as hardware proof and do not mix S6 bring-up work into renderer checkpoint/evidence PRs.
