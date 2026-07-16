# Aurora Audio OS — Master Project Reference

> **Status:** Living source of truth for Codex and any coding agent  
> **Repository:** `D:\aurora-audio`  
> **Primary language:** Rust  
> **Last consolidated milestone:** Optimization Sprint 2B complete; simulation-first validation requested next.

---

## 1. Purpose of this document

This file is the authoritative operating manual for Aurora Audio OS.

Every coding agent must read this document before making changes. It exists to prevent:

- agents inventing architecture;
- duplicated work;
- accidental scope expansion;
- regressions in real-time guarantees;
- illegal or license-incompatible integrations;
- misleading claims about measured latency or production readiness;
- uncontrolled feature accumulation;
- code being added before interfaces, tests, and acceptance criteria are defined.

When this document conflicts with an old prompt, chat message, generated plan, or abandoned experiment, **this document wins**, unless the user explicitly approves an update to this reference.

---

# 2. Product vision

Aurora is an open, modular, hardware-independent spatial-audio platform.

Its long-term goal is to provide:

- real-time multichannel rendering;
- arbitrary speaker layouts;
- canonical home-theater channel layouts;
- format-independent internal audio objects and PCM;
- modular decoder, renderer, DSP, I/O, and calibration adapters;
- deterministic simulation;
- room measurement and calibration;
- multichannel wired and wireless endpoints;
- future embedded and OEM deployment;
- support for open immersive formats such as IAMF/Eclipsa;
- optional legal codec integrations through explicit adapters and licensing review.

Aurora is **not** a clone of Yamaha, Denon, Sonos, Dolby Atmos, DTS:X, or any proprietary receiver.

Aurora is a platform that can accept multiple input formats and render them through an open processing pipeline.

---

# 3. Explicit non-goals

Aurora must never:

- bypass codec licensing or DRM;
- copy proprietary codec implementations;
- claim Dolby/DTS compatibility without legal authorization;
- present simulated latency as physically measured latency;
- hard-code itself to one speaker brand or hardware vendor;
- expose third-party library types in Aurora-owned public APIs;
- allocate memory inside steady-state audio callbacks;
- block, log, access files, spawn processes, or panic inside audio callbacks;
- silently switch devices after a failure;
- add unverified features without tests and benchmarks;
- merge experimental reverse-engineered codecs into the production core;
- make marketing claims before hardware validation;
- treat proof-of-concept drift correction as production ASRC;
- invent test results when hardware is unavailable.

---

# 4. Source-of-truth hierarchy

Agents must follow this priority order:

1. `AURORA_MASTER_REFERENCE.md` — this document.
2. `AGENTS.md` — repository-wide operational rules.
3. Accepted ADRs in `docs/adr/`.
4. Current public Rust APIs and tests.
5. Architecture and subsystem documentation under `docs/`.
6. Current milestone prompt approved by the user.
7. Older prompts, reports, chat history, and experiments.

If any two sources conflict, stop and report the conflict before coding.

---

# 5. Current project state

## 5.1 Completed foundations

The following are implemented and verified:

### Core architecture
- Rust workspace.
- Strongly typed shared domain model.
- Aurora-owned interfaces for:
  - renderers;
  - DSP engines;
  - decoders;
  - real-time input/output backends;
  - asynchronous resampling;
  - duplex transport.
- Third-party implementations isolated behind adapters.
- CI, formatting, clippy, unit tests, integration tests, benchmarks, and documentation.

### Offline rendering
- Mono WAV input.
- Scene trajectory loading.
- Stereo, 5.1, 7.1, and 5.1.2 output.
- Multichannel float WAV export.
- Canonical channel order.
- WAVE_FORMAT_EXTENSIBLE masks where representable.
- Deterministic block rendering.
- Clipping detection.
- Scene-order-independent standard layouts.

### Channel model
Canonical Aurora order:

- Stereo: `FL, FR`
- 5.1: `FL, FR, FC, LFE, SL, SR`
- 7.1: `FL, FR, FC, LFE, SL, SR, SBL, SBR`
- 5.1.2: `FL, FR, FC, LFE, SL, SR, TFL, TFR`

5.1.2 WAV channel masks represent physical top-front positions, but do not encode “up-firing” intent.

### DSP
- Aurora basic DSP abstractions.
- Gain.
- Mute.
- Polarity.
- Delay.
- High-pass / low-pass / PEQ model.
- Geometric speaker delay calculation.
- CamillaDSP out-of-process adapter.
- CamillaDSP 4.1.3 validated on Windows.
- Generated YAML compatibility verified.
- Offline gain, delay, polarity, and filter processing verified.
- CamillaDSP streaming WAV sizes finalized after processing.

### Third-party adapters
Adapter crates exist for:

- `libiamf`
- `truehdd`
- `CamillaDSP`
- `Cavern`

Rules:

- no third-party source is copied or vendored;
- all adapters are optional;
- Aurora core builds without them;
- `libiamf` is the preferred open immersive-decoder candidate;
- `truehdd` is experimental/offline-only and high risk;
- `Cavern` stays disabled pending license review;
- CamillaDSP is external-process integration, not a fork.

### Real-time engine
- CPAL backend.
- Windows output-device enumeration.
- Real-time test signals.
- Stereo smoke test completed.
- Caller-owned renderer buffers.
- Zero steady-state allocations after warm-up.
- No callback logging, filesystem, process work, blocking locks, or panic paths.
- Numeric callback fault propagation.
- Preallocated buffers.
- Callback metrics.
- Oversized host callback chunking.
- 60-second stereo smoke:
  - no drops;
  - no faults;
  - callback timing well within budget.

### Performance
At 48 kHz / 256 frames:

- Stereo full block: ~4.50 µs
- 5.1 full block: ~9.98 µs
- 8 channels: ~12.59 µs
- 12 channels: ~18.17 µs
- Synthetic DSP kernel, 8 channels: ~154 µs

The engine is currently far below the available block budget.

### Duplex transport
Compared transports:

- per-sample `ArrayQueue<f32>`;
- fixed block pool;
- contiguous preallocated SPSC frame ring.

Selected:

- contiguous bounded SPSC frame ring;
- no allocation after startup;
- no locks;
- bounded memory;
- coherent multichannel frame ordering;
- approximately six atomic operations per normal block.

### ASRC and drift
- Aurora-owned ASRC trait.
- `rubato 0.16.2` adapter.
- MIT license.
- Multichannel sinc processing.
- Shared ratio across channels.
- Smooth ratio updates.
- Preallocated steady-state processing.
- PI drift controller:
  - ±500 ppm clamp;
  - anti-windup;
  - maximum 2 ppm ratio movement per update;
  - prolonged-saturation fault.
- Simulated ±10 to ±250 ppm over up to eight hours remained bounded.
- Raw sample slips are not used in the normal adaptive path.

### Duplex state machine
States:

- `Stopped`
- `Starting`
- `Running`
- `Degraded`
- `Faulted`
- `Recovering`
- `Stopping`

No silent switching to a different device is allowed.

---

# 6. Known limitations

The following are not yet proven:

- live input/output duplex on physical capture hardware;
- physical round-trip latency;
- multichannel 5.1/7.1 hardware output;
- device unplug/replug behavior on real hardware;
- one-hour or longer hardware soak;
- stable Windows endpoint identity beyond CPAL’s available descriptors;
- full live-input CLI path on the current development machine;
- HDMI/eARC input;
- real IAMF decoding integration;
- Dolby or DTS decoding;
- wireless speaker endpoints;
- room calibration;
- AI;
- production ASRC quality on diverse hardware;
- production-grade device recovery across every driver.

Current development machine has only stereo output devices and no usable input device.

---

# 7. Repository architecture

Expected high-level structure:

```text
aurora-audio/
├── AGENTS.md
├── AURORA_MASTER_REFERENCE.md
├── VISION.md
├── README.md
├── Cargo.toml
├── Cargo.lock
├── THIRD_PARTY_LICENSES.md
├── docs/
│   ├── architecture.md
│   ├── roadmap.md
│   ├── licensing-risks.md
│   ├── latency-budget.md
│   ├── realtime-audio.md
│   ├── threading-model.md
│   ├── device-support.md
│   ├── backend-timing.md
│   ├── duplex-audio.md
│   ├── duplex-transport.md
│   ├── drift-compensation.md
│   ├── asynchronous-resampling.md
│   ├── drift-controller.md
│   ├── live-duplex.md
│   ├── device-state-machine.md
│   ├── physical-latency-validation.md
│   ├── simulation-backend.md
│   ├── virtual-clock.md
│   ├── fault-injection.md
│   ├── simulation-validation.md
│   ├── profiling.md
│   ├── adapters.md
│   ├── realtime-allocation-audit.md
│   └── adr/
├── crates/
│   ├── aurora-core/
│   ├── aurora-scene/
│   ├── aurora-renderer-api/
│   ├── aurora-renderer-basic/
│   ├── aurora-dsp-api/
│   ├── aurora-dsp-basic/
│   ├── aurora-audio-io/
│   ├── aurora-measurement/
│   ├── aurora-decoder-api/
│   ├── aurora-decoder-iamf/
│   ├── aurora-decoder-truehdd/
│   ├── aurora-dsp-camilladsp/
│   ├── aurora-renderer-cavern/
│   ├── aurora-realtime-audio-api/
│   ├── aurora-realtime-audio-cpal/
│   ├── aurora-realtime-engine/
│   ├── aurora-realtime-audio-sim/
│   └── aurora-cli/
├── fixtures/
├── output/
└── .github/workflows/
```

Agents must inspect the actual workspace before assuming every listed path already exists.

---

# 8. Architectural invariants

## 8.1 Ownership
Aurora owns all public interfaces.

Third-party types must remain private to adapter crates.

## 8.2 Core purity
`aurora-core` contains shared domain types and structured errors only.

It must not contain:

- CLI helpers;
- filesystem logic;
- codec-specific types;
- CPAL types;
- CamillaDSP YAML types;
- third-party process models;
- UI types.

## 8.3 Renderer boundary
A renderer:

- accepts Aurora domain objects;
- writes into caller-owned buffers;
- never allocates in steady state;
- reports latency;
- remains replaceable;
- does not decode codecs;
- does not perform file I/O.

## 8.4 DSP boundary
A DSP engine:

- accepts Aurora-owned configuration;
- does not expose backend-specific types;
- reports latency;
- supports offline and future real-time adapters;
- must not force CamillaDSP into the core.

## 8.5 Decoder boundary
A decoder converts supported input into Aurora PCM and/or Aurora audio-object metadata.

It does not:

- render speakers;
- perform room correction;
- own output-device logic.

## 8.6 Real-time callback rules
Inside callbacks:

- no heap allocation;
- no buffer growth;
- no `String`;
- no `HashMap` creation;
- no blocking mutex;
- no filesystem;
- no process spawning;
- no logging;
- no sleeping;
- no panic;
- no `unwrap()` or `expect()`;
- no unbounded work.

## 8.7 Metrics
Callbacks update fixed numeric counters and fault states only.

Formatting and printing belong on the control thread.

## 8.8 Determinism
Simulation and offline tests must be deterministic when supplied the same seed and fixtures.

## 8.9 Honesty
Terms must be precise:

- **requested** — user-requested setting;
- **negotiated** — backend-accepted format;
- **observed** — callback behavior seen at runtime;
- **estimated** — derived without physical measurement;
- **simulated** — produced by the simulator;
- **measured** — captured from real hardware.

Never interchange these labels.

---

# 9. Licensing and legal rules

Before adding any dependency or external tool, the agent must report:

- project name;
- version;
- license;
- whether commercial use is allowed;
- whether static/dynamic/process isolation matters;
- whether source redistribution is required;
- whether trademarks or patents are implicated;
- whether the dependency is production-ready;
- whether it remains optional.

Current known posture:

- `hound`: Apache-2.0
- `serde_json`: permissive
- `cpal`: permissive
- `criterion`: MIT OR Apache-2.0
- `crossbeam-queue`: MIT OR Apache-2.0
- `rubato 0.16.2`: MIT
- `CamillaDSP`: external executable; license boundary documented
- `libiamf`: preferred open-format candidate; adapter only
- `truehdd`: experimental, offline-only, high commercial risk
- `Cavern`: disabled by default pending license review

No agent may enable experimental codec adapters in production paths without explicit approval.

---

# 10. Required agent workflow

Every milestone must follow this sequence.

## Step 1 — Read
Read:

- this file;
- `AGENTS.md`;
- relevant ADRs;
- relevant subsystem docs;
- current crate APIs;
- tests;
- current milestone prompt.

## Step 2 — Inspect
Before editing, report:

- affected crates;
- existing interfaces;
- likely risks;
- possible license impact;
- whether the milestone conflicts with current architecture.

## Step 3 — Plan
Write a scoped implementation plan.

The plan must include:

- goals;
- non-goals;
- files expected to change;
- interfaces;
- tests;
- benchmarks;
- acceptance criteria;
- stop condition.

## Step 4 — Implement smallest viable increment
Do not implement the whole roadmap.

Implement one checkpoint at a time.

## Step 5 — Verify
Always run:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

When performance-sensitive code changes:

```powershell
cargo bench --workspace
```

When relevant, run exact CLI regression commands documented in `README.md`.

## Step 6 — Report
Every report must include:

- files changed;
- API changes;
- dependencies and licenses;
- tests added;
- command results;
- benchmark deltas;
- measured/simulated values with correct labels;
- known limitations;
- remaining risks;
- recommended next milestone.

## Step 7 — Stop
Do not continue into the next milestone without user approval.

---

# 11. Change-control rules

An agent must stop and ask before:

- changing a public trait;
- adding a dependency;
- enabling an external adapter by default;
- changing channel order;
- changing WAV masks;
- changing real-time buffer ownership;
- changing the state machine;
- changing fault semantics;
- changing latency terminology;
- replacing the ASRC;
- replacing the transport;
- adding unsafe Rust;
- adding codec logic;
- adding networking;
- adding HDMI/eARC;
- adding wireless audio;
- adding AI;
- adding a GUI;
- deleting tests;
- relaxing acceptance criteria;
- accepting benchmark regressions above 10%.

Small internal refactors that preserve behavior may proceed with tests.

---

# 12. Definition of done

A milestone is complete only when:

- the implementation stays within scope;
- no excluded feature was added;
- architecture remains modular;
- public interfaces are documented;
- tests cover success and failure paths;
- benchmarks exist for performance-critical work;
- zero-allocation guarantees remain verified where applicable;
- all verification commands pass;
- documentation is updated;
- license records are updated;
- outputs are described honestly;
- known limitations are explicit;
- the agent stops.

---

# 13. Test policy

Tests must cover:

## Correctness
- canonical channel ordering;
- deterministic output;
- finite samples;
- silence preservation;
- delay continuity;
- routing correctness;
- ASRC channel coherence;
- state transitions;
- fault propagation;
- selector ambiguity;
- scene-order independence.

## Real-time safety
- zero warmed-up allocations;
- unchanged buffer capacities;
- no panic paths;
- bounded queues;
- no deadlocks;
- no unbounded memory growth.

## Simulation
- same seed, same checksum;
- independent clocks;
- callback jitter;
- variable callback sizes;
- starvation;
- burst callbacks;
- device loss;
- recovery;
- loopback truth recovery;
- low-confidence rejection.

## Regression
- offline WAV rendering;
- CamillaDSP integration;
- renderer benchmarks;
- transport benchmarks;
- real-time smoke where hardware is available.

---

# 14. Benchmark policy

Performance results must include:

- release mode;
- sample rate;
- block size;
- channel count;
- median;
- p95 when available;
- maximum when meaningful;
- percentage of block budget;
- baseline comparison;
- machine/environment notes.

Do not use directional “sustainable channel count” estimates as product claims.

No explicit SIMD until benchmarks prove a real hotspot.

---

# 15. Current roadmap

## Phase 0 — Core prototype
Completed:

- architecture;
- core model;
- renderer interface;
- offline rendering;
- DSP;
- CamillaDSP adapter;
- canonical channel layouts;
- geometric delay;
- real-time engine;
- zero-allocation cleanup;
- duplex transport;
- ASRC;
- drift controller;
- state machine.

## Phase 1 — Deterministic simulator
Next priority.

Goals:

- full virtual audio backend;
- deterministic virtual clock;
- independent input/output clocks;
- virtual callback scheduling;
- variable callback sizes;
- jitter;
- starvation;
- burst callbacks;
- device-loss scripts;
- virtual loopback;
- multichannel device profiles;
- long accelerated runs;
- truth-vs-estimate latency validation;
- machine-readable reports.

## Phase 2 — Physical hardware validation
Only after simulator approval.

Goals:

- real capture/output interface;
- physical loopback latency;
- stereo soak;
- multichannel channel validation;
- one-hour stability;
- unplug/replug behavior;
- driver-specific limitations.

## Phase 3 — Spatial rendering improvements
Only after transport/backend validation.

Candidates:

- VBAP;
- spread;
- elevation;
- custom irregular layouts;
- Ambisonics;
- HRTF/binaural preview.

## Phase 4 — Open immersive formats
Preferred order:

1. PCM
2. IAMF/Eclipsa via legal adapter
3. optional research adapters
4. licensed proprietary codecs only after legal review

## Phase 5 — Input hardware
Research:

- eARC receiver;
- HDMI audio input;
- I²S/USB bridge;
- embedded hardware;
- legal HDMI/HDCP compliance.

## Phase 6 — Calibration
- measurement capture;
- impulse response;
- distance;
- gain;
- EQ;
- crossover;
- multi-seat optimization;
- later AI-assisted recommendations.

## Phase 7 — Wireless endpoints
Only after wired real-time correctness.

- clock synchronization;
- bounded latency;
- packet loss;
- jitter buffers;
- endpoint discovery;
- encryption;
- channel coherence;
- long soak tests.

---

# 16. Simulation-first policy

No hardware purchase is required until the simulator can demonstrate:

- full duplex without CPAL;
- 2/6/8/12 channel devices;
- independent clock drift;
- ASRC boundedness;
- loopback latency truth recovery;
- channel routing;
- polarity and gain validation;
- device loss and recovery;
- deterministic 24-hour simulated runs;
- zero steady-state allocation;
- bounded memory.

The simulator cannot prove:

- real driver behavior;
- analog noise;
- real hardware latency;
- physical clock quality;
- USB scheduling quirks;
- real unplug/replug behavior;
- amplifier/speaker behavior.

Those require Phase 2 hardware validation.

---

# 17. Simulation Sprint 1 requirements

The next agent should implement only the simulator.

Expected crate:

```text
aurora-realtime-audio-sim
```

It must implement Aurora-owned real-time backend interfaces.

Required profiles:

### Stereo consumer
- 2 in / 2 out
- 48 kHz
- fixed 256-frame callbacks
- configurable drift

### USB 5.1
- 2 in / 6 out
- 44.1/48/96 kHz
- 64–512 callbacks
- ±50 ppm

### USB 7.1
- 8 in / 8 out
- 48/96 kHz
- variable callbacks
- ±25 ppm

### Development 12-channel
- 12 in / 12 out
- 48 kHz
- 128 frames
- ±10 ppm

### Broken driver
- duplicate names;
- callback jitter;
- loss;
- stalls;
- format rejection;
- stream errors.

Required commands:

```text
aurora simulate-duplex
aurora simulate-latency
aurora simulate-output-validation
```

All reports must label results as **simulated**.

---

# 18. Standard report template for agents

Every agent must finish with this structure:

```text
Milestone:
Scope completed:
Scope intentionally not completed:

Files changed:
- ...

Public API changes:
- ...

Dependencies:
- name
- version
- license
- reason
- optional/default status

Architecture decisions:
- ...

Tests added:
- ...

Verification:
- cargo fmt:
- cargo clippy:
- cargo test:
- cargo bench:
- CLI/integration commands:

Performance:
- configuration
- median
- p95
- max
- budget percentage
- baseline delta

Results:
- clearly mark simulated / estimated / negotiated / observed / measured

Known limitations:
- ...

Remaining risks:
- ...

Recommended next milestone:
- ...

STOPPED:
Yes. No next milestone work was started.
```

---

# 19. Prompt preamble for every coding agent

Paste this before any milestone-specific prompt:

```text
Before doing any work:

1. Read AURORA_MASTER_REFERENCE.md completely.
2. Read AGENTS.md.
3. Read the relevant ADRs and subsystem documentation.
4. Inspect the current code and tests.
5. Treat AURORA_MASTER_REFERENCE.md as the source of truth.
6. Do not invent architecture, features, test results, hardware results, or legal conclusions.
7. Stay strictly within the supplied milestone.
8. Stop and report conflicts before coding.
9. Do not proceed to another milestone after completion.
10. Preserve all real-time, licensing, determinism, testing, and reporting rules.
```

---

# 20. Immediate next action

The next milestone is:

> **Simulation Sprint 1 — deterministic virtual audio hardware and full-system validation**

The next agent must not:

- buy hardware;
- add HDMI;
- add codecs;
- add wireless;
- add AI;
- add a GUI;
- add VBAP;
- add physical latency claims.

The simulator must be completed and reviewed first.

---

# 21. Maintenance rules for this reference

Update this document only when:

- a milestone is accepted;
- an ADR changes architecture;
- a dependency is added;
- a risk changes;
- a roadmap gate is completed;
- a legal or licensing conclusion is formally reviewed;
- a new invariant is approved.

Every update must include:

- date;
- milestone;
- changed sections;
- reason.

Do not rewrite history. Record replaced decisions in ADRs.

---

## Final instruction to all agents

Aurora is not judged by how many features it contains.

Aurora is judged by:

- correctness;
- determinism;
- measurable performance;
- real-time safety;
- modularity;
- legal clarity;
- honest validation;
- reproducibility;
- disciplined scope control.

When uncertain, stop, inspect, test, document, and ask.
