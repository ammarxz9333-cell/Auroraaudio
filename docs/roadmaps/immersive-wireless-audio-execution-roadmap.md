# Immersive Wireless Audio Execution Roadmap

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
- local ASRC and drift-control components using Rubato;
- simulation and assurance infrastructure;
- decoder adapter boundaries, including an IAMF placeholder;
- local in-process bounded transport prototypes.

The following are not yet implemented and must not be described as complete:

- production IAMF decoding;
- 3D VBAP or another height-capable loudspeaker renderer;
- HRTF/binaural rendering;
- packetized network audio transport;
- network jitter buffering and packet-loss handling;
- distributed clock synchronization across real devices;
- Raspberry Pi receiver runtime;
- end-to-end multiroom operation.

## Delivery rule

A milestone is complete only when all four outputs exist:

1. production code or an explicitly labeled experimental implementation;
2. deterministic automated tests;
3. a measurable artifact, such as a rendered WAV, packet trace, synchronization report, or latency report;
4. documentation that states limitations and reproducible commands.

Design-only documents, empty crates, adapter placeholders, and passing interface tests do not count as product capability.

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

1. I1: offline 3D loudspeaker rendering;
2. I2: binaural rendering and audible validation;
3. N1: deterministic network simulation;
4. I3: real IAMF decode integration, which may proceed in parallel after scene contracts stabilize;
5. N2: real packet transport;
6. N3: Linux/Raspberry Pi receiver runtime;
7. M1: multiroom product behavior;
8. physical hardware validation and cost selection.

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

## Non-goals for the next implementation sprint

- licensed Dolby decoding;
- HDMI capture or eARC implementation;
- commercial certification claims;
- a polished consumer UI;
- a complete 5.1.2 hardware bill of materials based only on marketplace prices;
- merging all milestones into one unreviewable change.

## Immediate next implementation slice

The next code PR should implement only the first vertical slice of I1:

- a minimal 3D loudspeaker-triplet solver;
- one 5.1.2 layout fixture;
- canonical position tests;
- a CLI command that exports a gain-trajectory JSON artifact.

It must not include IAMF, networking, HRTF, or Raspberry Pi runtime changes in the same PR.