# Immersive Wireless Audio Execution Roadmap

## Execution state

- `program_state`: `PRODUCT_IMPLEMENTATION_ACTIVE`
- `active_program`: `Immersive Audio Product Implementation 1`
- `active_work_item`: GitHub issue `#43`, Checkpoint A
- `next_work_item`: GitHub issue `#38`
- `governance_mode`: maintenance only
- `implementation_rule`: code, tests, artifacts, and reproducible commands are required

Aurora is no longer using governance expansion as the primary development path. Existing accepted architectural contracts remain valid, but new ADRs, passive contract crates, inspection layers, and planning-only milestones are not the default next step. They may be added only when an implementation is blocked by a concrete architectural decision that cannot be resolved inside a reviewable code PR.

The active priority is working product capability. The repository should now advance through small implementation PRs that produce executable code, automated tests, measurable artifacts, and honest limitations.

The currently landed binaural code must first be stabilized and classified honestly under issue `#43`, Checkpoint A. It is a geometric ITD/ILD prototype, not a true HRTF renderer. After that correction, work proceeds to issue `#38`, the first offline 3D loudspeaker-rendering vertical slice.

## Product objective

Aurora aims to become an open, low-cost immersive home-audio platform that can:

- render object-based or scene-based audio to arbitrary loudspeaker layouts, including height channels;
- provide a binaural headphone mode from the same scene model;
- distribute synchronized audio channels over ordinary IP networks to low-cost receiver nodes;
- support whole-home multiroom playback using the same transport and synchronization substrate;
- run the coordinator on commodity Linux hardware and later support Raspberry Pi-class endpoints.

Aurora does **not** claim Dolby Atmos compatibility unless a lawful licensed decoder is integrated. The target is a comparable immersive listening experience using open formats and Aurora-owned rendering, synchronization, DSP, and transport components.

## Current verified baseline

The repository currently contains:

- a real-time local audio engine;
- deterministic horizontal-plane 2D VBAP;
- a geometric stereo ITD/ILD binaural prototype that still requires Checkpoint A correction and full validation;
- local ASRC and drift-control components using Rubato;
- simulation and assurance infrastructure;
- decoder adapter boundaries, including an IAMF placeholder;
- local in-process bounded transport prototypes.

The following are not yet implemented and must not be described as complete:

- production IAMF decoding;
- 3D VBAP or another height-capable loudspeaker renderer;
- true HRTF convolution with measured HRIR data, elevation cues, and front/back cues;
- packetized network audio transport;
- network jitter buffering and packet-loss handling;
- distributed clock synchronization across real devices;
- Raspberry Pi receiver runtime;
- end-to-end multiroom operation.

## Development operating rules

1. One active implementation slice per PR.
2. Planning documents do not count as product delivery.
3. A capability is not complete until it compiles and its required tests pass.
4. Audible features require generated WAV evidence or another reproducible perceptual artifact.
5. Network features require packet, synchronization, loss, and latency evidence.
6. Simulation results must remain labeled as simulation and must not be presented as physical measurements.
7. The project must not create a new governance milestone merely because an implementation task is complex.
8. Existing governance work that does not unlock immediate product implementation may be closed, paused, or deferred.

## Delivery rule

A milestone is complete only when all four outputs exist:

1. production code or an explicitly labeled experimental implementation;
2. deterministic automated tests;
3. a measurable artifact, such as a rendered WAV, packet trace, synchronization report, or latency report;
4. documentation that states limitations and reproducible commands.

Design-only documents, empty crates, adapter placeholders, and passing interface tests do not count as product capability.

## Active stabilization — Geometric binaural Checkpoint A

### Scope

Correct and validate the geometric ITD/ILD binaural prototype already landed in the repository.

### Required work

- rename the mode to `GeometricBinaural` or another technically honest name;
- state explicitly that it is not HRTF and does not provide proven elevation or front/back discrimination;
- validate stereo-layout requirements and finite normalized output;
- reject negative, NaN, and infinite delay values with structured errors;
- verify final partial-block buffer handling and repeated-render behavior;
- run formatting, Clippy, workspace tests, strict rustdoc, and CI;
- keep Checkpoints B-D of issue `#43` out of this PR.

### Acceptance evidence

- successful compilation and exact validation commands;
- right/left ITD and ILD polarity tests;
- near-zero-distance and invalid-layout tests;
- documented dynamic-delay continuity limitation or a deterministic continuity result;
- no HRTF, Dolby Atmos-like, elevation, or front/back claim.

## Milestone I1 — Offline 3D loudspeaker rendering

### Scope

Implement an Aurora-owned height-capable renderer without changing the existing 2D renderer contract prematurely.

### Required work

- add `aurora-renderer-vbap3d` or an equivalent explicitly named experimental crate;
- construct and validate loudspeaker triplets in three dimensions;
- calculate normalized gains for azimuth and elevation;
- support at least 5.1.2 and 7.1.4 reference layouts;
- define deterministic behavior for sources outside the valid loudspeaker hull;
- preserve finite-output, bounded-memory, and allocation-free steady-state requirements where applicable.

### Acceptance evidence

- unit tests for canonical front, rear, side, and overhead positions;
- energy-normalization tests;
- continuity tests for moving sources;
- an offline WAV render of a source moving around and above the listener;
- a machine-readable gain trajectory report.

## Milestone I2 — Binaural/HRTF renderer

### Scope

Render the same Aurora scene model to stereo headphones for development and perceptual validation before purchasing multichannel hardware.

### Required work

- add an HRTF data interface that does not embed restrictively licensed datasets;
- support a documented redistributable HRTF dataset or require the user to supply one;
- implement convolution with interpolation between measured directions;
- include ITD, ILD, elevation, and rear/front cues represented by the selected dataset;
- provide an offline renderer first; real-time rendering is a later optimization.

### Acceptance evidence

- deterministic left/right impulse-response tests;
- stable interpolation tests;
- a binaural WAV containing front, side, rear, and overhead motion;
- convolution latency and CPU reports;
- explicit dataset licensing documentation.

## Milestone I3 — Real IAMF integration

### Scope

Replace the current IAMF adapter placeholder with an operational open-format decode path.

### Required work

- evaluate the official or reference IAMF implementation and its license;
- define a versioned out-of-process protocol before binding Aurora to a specific library ABI;
- decode IAMF input into Aurora-owned PCM buffers plus scene/object metadata;
- map decoded metadata into the Aurora scene model;
- isolate crashes and malformed input from the real-time engine.

### Acceptance evidence

- decode at least one legally redistributable IAMF conformance sample;
- verify channel/object timing and sample counts;
- produce a reproducible decoded PCM or rendered WAV artifact;
- test malformed, truncated, and unsupported input;
- retain `production_ready = false` until conformance and failure-isolation gates pass.

## Milestone N1 — Deterministic network simulator

### Scope

Validate transport and synchronization policy without buying hardware.

### Required work

- simulate one coordinator and multiple independent receiver clocks;
- model clock offset, frequency drift, variable jitter, burst delay, reordering, duplication, loss, and temporary disconnection;
- model bounded receiver buffers and startup synchronization;
- feed real or deterministic PCM blocks through the simulated packet path;
- connect receiver fill-level error to the existing drift controller and ASRC.

### Acceptance evidence

- deterministic seeded scenarios;
- synchronization-error percentiles for every receiver;
- underrun, overrun, loss, concealment, and resampling reports;
- pass/fail thresholds for music multiroom mode and low-latency theater mode;
- tests proving recovery after network disruption.

## Milestone N2 — Packetized IP audio transport

### Scope

Implement a real network transport only after the simulator establishes buffer and control-policy requirements.

### Required work

- define an Aurora packet envelope with stream, channel, sequence, sample-time, format, and integrity fields;
- begin with UDP on a trusted LAN;
- implement sender pacing, receiver reordering, bounded jitter buffering, duplicate rejection, and late-packet policy;
- maintain separate control and audio planes;
- add optional loss concealment suitable for short losses;
- avoid claiming sub-millisecond acoustic alignment until measured on physical devices.

### Acceptance evidence

- localhost and network-namespace integration tests;
- packet capture fixtures;
- induced impairment tests using the same scenarios as N1;
- end-to-end latency and inter-receiver skew reports;
- no unbounded allocations or queues in steady state.

## Milestone N3 — Linux receiver node and Raspberry Pi readiness

### Scope

Build a receiver daemon that can later run on Raspberry Pi-class hardware while remaining testable on ordinary Linux machines.

### Required work

- implement a headless receiver daemon;
- discover or configure the coordinator;
- expose device, stream, buffer, drift, and health status;
- output through CPAL/ALSA using explicit device selection;
- provide cross-compilation checks for ARM targets;
- add systemd service files and restart/recovery behavior;
- defer hardware-specific tuning until measured devices exist.

### Acceptance evidence

- two or more receiver processes running on one development machine with independent simulated clocks;
- ARM compilation in CI;
- restart and reconnection tests;
- documented minimum CPU, memory, network, DAC, and amplifier requirements based on measurements rather than guessed prices.

## Milestone M1 — Multiroom product mode

### Scope

Use the same transport to distribute synchronized program audio to rooms while keeping theater channel distribution a separate operating mode.

### Required work

- define room groups, zones, channel maps, volume, mute, and delay trim;
- support one program in multiple rooms first;
- add independent room programs only after resource and routing policies are explicit;
- keep multiroom latency targets separate from theater latency targets;
- provide deterministic group join, leave, pause, resume, and resynchronization behavior.

### Acceptance evidence

- group synchronization reports;
- join/leave without permanent desynchronization;
- volume and mute control tests;
- a reproducible multi-process demo.

## Integration order

The required order is:

1. stabilize and honestly classify the landed geometric binaural prototype under issue `#43`, Checkpoint A;
2. I1: offline 3D loudspeaker rendering under issue `#38`;
3. I2: true HRTF rendering and audible validation;
4. N1: deterministic network simulation;
5. I3: real IAMF decode integration, which may proceed in parallel after scene contracts stabilize;
6. N2: real packet transport;
7. N3: Linux/Raspberry Pi receiver runtime;
8. M1: multiroom product behavior;
9. physical hardware validation and cost selection.

## Hardware purchasing gate

No hardware purchase recommendation is valid until Aurora can report:

- measured coordinator CPU cost per object and output channel;
- measured receiver CPU and memory cost;
- required network bitrate and safe jitter-buffer range;
- end-to-end latency distribution;
- inter-receiver skew distribution;
- required audio output channel count per node;
- whether one node drives one speaker, a stereo pair, or multiple amplified channels.

The first physical experiment should use the smallest configuration that can falsify the design: one coordinator, two receiver nodes, and two independent DAC clocks. A complete 5.1.2 purchase is not the first step.

## Deferred governance work

Runtime materialization, additional inspection layers, and other passive control-plane expansions are not the active product path. Existing merged contracts remain available, but unfinished governance work should not block renderer, HRTF, simulation, transport, receiver, or multiroom implementation. Any future governance change must identify the exact implementation blocker it resolves and must remain smaller than the implementation it enables.

## Non-goals for the next implementation sprint

- licensed Dolby decoding;
- HDMI capture or eARC implementation;
- commercial certification claims;
- a polished consumer UI;
- a complete 5.1.2 hardware bill of materials based only on marketplace prices;
- merging all milestones into one unreviewable change;
- creating another planning-only milestone without an implementation blocker.

## Immediate next implementation slice

The active code PR is issue `#43`, Checkpoint A only. After it is reviewed and merged, the next code PR must implement only the first vertical slice of I1:

- a minimal 3D loudspeaker-triplet solver;
- one 5.1.2 layout fixture;
- canonical position tests;
- a CLI command that exports a gain-trajectory JSON artifact.

It must not include IAMF, networking, true HRTF, or Raspberry Pi runtime changes in the same PR.