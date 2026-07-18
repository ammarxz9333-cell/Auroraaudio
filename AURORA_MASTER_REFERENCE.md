# Aurora Audio OS — Master Project Reference

> **Status:** Living source of truth for Codex and any coding agent  
> **Repository:** `D:\aurora-audio`  
> **Primary language:** Rust  
> **Last consolidated milestone:** Runtime Assembly Contracts 1 accepted on
> 2026-07-18 as a software-only milestone; Phase 2 physical hardware validation
> remains open and incomplete. Runtime Plan Inspection 1 is the single active
> software-only milestone; Checkpoints A and B are complete and Checkpoint C is active. Hardware-blocked
> parallel development is governed by Section 16.1.

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

Every generated result and report must also identify exactly one truth source:

- `unit_test` -- deterministic in-process verification without a virtual device;
- `deterministic_simulation` -- deterministic modeled execution with explicit
  simulated truth;
- `virtual_audio_backend` -- execution through Aurora's software audio-device
  backend, still simulated and not physical;
- `host_api_observation` -- metadata or behavior observed through a host API
  without an accepted physical signal path;
- `physical_measurement` -- evidence captured from an explicitly documented
  physical signal path.

Only `physical_measurement` may be used as physical evidence. No other truth
source may satisfy a physical acceptance gate or be relabeled as measured.

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
Accepted and frozen on 2026-07-16.

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

Status: **OPEN and INCOMPLETE**. Phase 2 remains mandatory while parallel
software work proceeds under Section 16.1.

Goals:

- real capture/output interface;
- physical loopback latency;
- stereo soak;
- multichannel channel validation;
- one-hour stability;
- unplug/replug behavior;
- driver-specific limitations.

## Phase 3 — Spatial rendering improvements
Software-only work may begin under Section 16.1 while Phase 2 is blocked.
Hardware-relevant acceptance remains conditional until the applicable Phase 2
evidence exists.

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

## 16.1 Hardware-Blocked Parallel Development Policy

This policy permits controlled software progress while required physical
hardware is unavailable. It does not weaken, replace, postpone, or infer success
for any physical acceptance criterion.

### Phase 2 status and evidence boundary

Phase 2 remains **OPEN and INCOMPLETE**. Physical hardware validation is
mandatory. Simulation Sprint 1 is accepted and frozen, but it cannot satisfy or
replace Phase 2.

No acceptance for physical latency, physical channel routing, physical clock
drift, USB scheduling or behavior, driver behavior, unplug/replug behavior, or
analog loopback may be inferred from unit tests, deterministic simulation, a
virtual backend, or host API observations.

The current environmental blocker is:

- no usable 2-channel input endpoint;
- no physical loopback path;
- no exact 6-channel or 8-channel output endpoint;
- no independently identifiable multichannel physical paths.

This blocker is environmental. It is not evidence that Phase 2 passed or failed.

### Permission for parallel work

A later software milestone may begin before Phase 2 closes only when all of the
following are true:

- implementation does not require unresolved physical evidence;
- accepted real-time contracts remain unchanged unless separately approved;
- deterministic unit, integration, and simulation tests can validate the
  software behavior claimed by the milestone;
- any acceptance depending on hardware remains explicitly conditional;
- no fabricated, estimated, simulated, virtual, or host-observed value is
  consumed as a hardware measurement;
- the milestone has the dependency matrix required below;
- work uses a dedicated branch and does not merge automatically into `main-v2`.

Parallel work must not duplicate `aurora-realtime-audio-sim` or modify the
accepted Simulation Sprint 1 record. Physical validation remains on the open
`phase-2-physical-hardware-validation` branch until its acceptance criteria pass.

### Milestone execution states

Execution state records where authorized work is in its lifecycle. It is a
separate field from milestone evaluation classification and uses exactly one of
these values:

- `NOT_STARTED`: no implementation work has begun. Branch creation alone does
  not necessarily count as implementation.
- `IN_PROGRESS`: authorized milestone implementation is actively underway. No
  acceptance or validation conclusion is implied, and the milestone has not yet
  reached its evaluation boundary.
- `READY_FOR_EVALUATION`: implementation has reached the authorized stop
  boundary and the required checks and evidence are being evaluated. An
  evaluation classification is mandatory in this state.
- `CLOSED`: the current milestone execution cycle has received a terminal
  evaluation classification and no further work is permitted except through an
  approved follow-up or amendment. `CLOSED` does not by itself mean `ACCEPTED`.

No evaluation classification is required while `execution_state` is
`IN_PROGRESS`. `IN_PROGRESS` is not an acceptance classification.

### Milestone evaluation classifications

Evaluation classification records the conclusion supported by evidence, not
the progress of implementation. When required, it must use exactly one of these
values:

- `ACCEPTED`: every required software, simulation, review, and hardware gate has
  passed. This is the only classification that fully closes a milestone.
- `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`: software and simulation criteria
  have passed formal review, but identified hardware gates remain open. The
  milestone may be integrated only with its limitations visible and remains open.
- `IMPLEMENTATION_COMPLETE_VALIDATION_PENDING`: implementation is complete, but
  one or more required validation or review gates have not passed. This is not
  acceptance and does not authorize claims based on the pending evidence.
- `BLOCKED_BY_HARDWARE`: required progress or validation cannot continue because
  identified physical equipment or signal paths are unavailable. This is neither
  a pass nor a failure.
- `REJECTED`: the implementation or evidence failed an applicable criterion or
  architecture review and cannot advance without a new reviewed change.

Execution state and evaluation classification must be recorded as different
fields. Evaluation classification becomes mandatory when `execution_state` is
`READY_FOR_EVALUATION`, and `execution_state=CLOSED` requires a terminal
evaluation classification for the current execution cycle. Commits, pull
requests, reports, and milestone documentation
must state the current evaluation classification whenever one has been assigned
and it is not `ACCEPTED`.

### Hardware dependency matrix

Before implementation, every parallel milestone must publish a matrix with:

- software-only acceptance criteria;
- deterministic simulation acceptance criteria;
- hardware-dependent acceptance criteria;
- whether Phase 2 completion is a hard prerequisite for implementation;
- whether Phase 2 completion is a hard prerequisite for final acceptance;
- whether acceptance must remain conditional and why.

Missing hardware criteria may not be deleted, weakened, or converted into
simulation criteria. A milestone cannot move to `ACCEPTED` until every matrix
entry required for final acceptance has passed with the stated truth source.

### Contract protection

Parallel development must not silently change:

- Aurora-owned public traits;
- callback or buffer ownership;
- real-time allocation guarantees;
- bounded-memory guarantees;
- fault semantics;
- state-machine semantics;
- device-selection semantics;
- report truth-source or latency semantics.

Any such change requires a separate architecture amendment or accepted ADR,
including focused compatibility tests and documentation. This policy itself is
not approval for any contract change.

### Git policy

- `main-v2` remains the canonical integration branch.
- `phase-2-physical-hardware-validation` remains open until physical acceptance.
- each parallel milestone uses a dedicated branch from `main-v2`;
- no branch merges automatically into `main-v2`;
- accepted tags and accepted milestone records are immutable;
- conditional milestones must be labeled clearly in commits, pull requests, and
  documentation.

### First parallel-safe recommendation

The first technically safe later scope is **Phase 3A -- Deterministic Offline
Spatial Rendering Improvements**.

Current execution record:

- `execution_state`: `CLOSED`;
- `evaluation_classification`: `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`;
- branch: `phase-3a`;
- branch base: `main-v2` at
  `bc2cf637bb8dc5d526a28271e1070efcff1e09d4`.

Formal software evaluation passed on 2026-07-16. The execution cycle is closed,
but the milestone remains conditionally open because the hardware gates recorded
in `docs/phase-3a.md` have not run. This is not full acceptance, and no accepted
tag is authorized.

It may implement deterministic, offline renderer work that fits existing Aurora
contracts, beginning with VBAP geometry and focused spread/elevation or irregular
layout experiments. It may add fixtures, unit tests, offline integration tests,
and release benchmarks. Phase 2 completion is not a hard prerequisite for this
software implementation.

The following remain conditional on Phase 2 or later explicit evidence:

- physical 5.1/7.1 routing and channel identity;
- live multichannel callback behavior on real endpoints;
- physical driver stability, clock behavior, and long-run performance;
- audible or speaker-dependent quality conclusions.

Stop boundaries for Phase 3A:

- no Aurora-owned public trait change without a separate ADR or amendment;
- no channel-order, WAV-mask, callback-ownership, state, fault, or device change;
- no replacement of the accepted renderer or enabling a new default silently;
- no HRTF dependency, codec work, HDMI, networking, wireless audio, GUI, AI, or
  calibration scope;
- no physical claim and no `ACCEPTED` classification while required hardware
  matrix entries remain open.

After its implementation is complete, Phase 3A may use
`IMPLEMENTATION_COMPLETE_VALIDATION_PENDING`. It may reach
`CONDITIONALLY_ACCEPTED_PENDING_HARDWARE` only after formal software and
simulation review.

Phase 3A reached `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE` on 2026-07-16 after
formal software review of evaluated commit
`501fb9eea3c19284d71bd9d9bfa21e664f6e5a55`. The review recorded 130 passing
tests, 5 explicitly ignored hardware-only tests, zero warmed-up renderer
allocations, successful formatting, Clippy, Rustdoc, Actionlint, and workspace
benchmarks. Benchmark timing is `host_api_observation`, not a physical latency
measurement. Physical 5.1/7.1 routing, endpoint behavior, stability, and audible
conclusions remain pending under Phase 2. See `docs/acceptance/phase-3a.md`.

### Authorized next parallel milestone: Phase 3B

The owner-authorized next milestone is **Phase 3B -- Deterministic Horizontal
Source Spread and Irregular Layout Support**.

Current execution record:

- `execution_state`: `CLOSED`;
- `evaluation_classification`: `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`;
- planned branch: `phase-3b-horizontal-spread`;
- required base: `main-v2` after the governance amendment defining this scope.

Purpose and included scope:

- extend the existing `aurora-renderer-vbap` crate rather than creating a
  competing renderer;
- add a documented normalized horizontal spread parameter over the finite,
  inclusive domain `0.0..=1.0`;
- preserve accepted Phase 3A point-source output at spread zero within a stated
  floating-point tolerance;
- distribute normalized energy deterministically to nearby eligible horizontal
  speakers as spread increases;
- support uneven, asymmetric, sparse, dense, and more-than-eight-speaker
  horizontal layouts within configured caller-owned capacity;
- make non-degenerate layout permutations equivalent after applying the same
  output-channel permutation;
- document deterministic handling of wraparound, exact hits, midpoint ties,
  duplicate and near-duplicate angles, and degenerate one- or two-speaker
  layouts.

Phase 3B excludes elevation and 3D/triplet VBAP, HRTF or binaural rendering,
Ambisonics/HOA, room simulation, reflections, reverberation, new distance or
Doppler behavior, head tracking, listener orientation, hardware integration,
calibration, physical validation, and every Phase 3C or later feature. It may
not change the Aurora-owned `Renderer` trait, Basic renderer behavior, CLI or
live defaults, callback ownership, state or fault semantics, device selection,
or truth-source semantics.

Hardware dependency matrix:

- software-only gates: point-source compatibility, deterministic spread,
  irregular-layout correctness, power normalization, finite output, stable
  tie-breaking, layout-order independence, caller-owned bounded memory, zero
  warmed-up allocations, public API documentation, protected-contract audit,
  deterministic fixtures, and unit/integration tests;
- deterministic simulation gates: repeated fixture reproducibility, canonical
  and irregular scenarios, source sweeps across `-pi/+pi`, spread sweeps over
  the complete domain, and deterministic checksums where suitable;
- host-observation gates: representative offline renderer benchmarks and an
  allocation audit, labeled only `host_api_observation`;
- hardware-dependent gates: audible spread on independently identifiable
  speakers, physical 5.1/7.1 routing, endpoint behavior, real-path level
  consistency, and stability on physical hardware;
- Phase 2 required for implementation: no;
- Phase 2 required for software correctness evaluation: no;
- Phase 2 required for final physical acceptance: yes;
- Phase 3A dependency: mandatory; its point-source path and validation may not
  regress;
- expected classification after all non-hardware gates pass:
  `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`.

Permitted truth sources are `unit_test`, `deterministic_simulation`, and
`host_api_observation`. `physical_measurement` remains unavailable until real
hardware evidence exists. Benchmark timing is never physical latency.

Execution started as `NOT_STARTED` and moved to `IN_PROGRESS` with the first
authorized Phase 3B implementation commit. It reaches `READY_FOR_EVALUATION` at the stated
stop boundary, and becomes `CLOSED` only after criterion-by-criterion review.
The implementation stop boundary is the documented spread algorithm, bounded
implementation, required fixtures/tests, allocation audit, benchmarks, full
validation, evaluation record, and an unmerged Phase 3B pull request. A required
protected-contract change, unbounded memory, nondeterministic algorithm, Phase
3A incompatibility, or architecture expansion stops the milestone.

Phase 3B reached `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE` on 2026-07-16 after
formal evaluation of implementation commit
`f95c91b1f4acef01ab5613286cb8797900c6f2e9` and evidence commit
`04a37dca069e90c707954f88093fc1ba2858325b`. All software, deterministic,
documentation, allocation, and host-observation gates passed: 144 tests passed,
5 hardware-only tests remained explicitly ignored, warmed-up spread rendering
allocated zero times over 1,000 calls, and the irregular sweep checksum was
`0ef1fc03dfa5892e` on Windows and `6c25035a72424f0f` on Linux after quantizing
gains to the declared `1e-5` comparison tolerance. Physical audible behavior,
routing, endpoint behavior,
real-path levels, and stability remain pending. See
`docs/acceptance/phase-3b.md`.

### Authorized test-only milestone: Simulation Assurance Campaign 1

The owner-authorized next milestone is **Simulation Assurance Campaign 1 --
Massive Deterministic Property and Stress Testing**. It is a verification-only
campaign over the accepted simulator and the existing Phase 3A/3B renderers.
It must not create a second simulator or add product behavior.

Current execution record:

- `execution_state`: `CLOSED`;
- `evaluation_classification`: `ACCEPTED`;
- governance branch: `governance/define-simulation-assurance-campaign-1`;
- implementation branch: `test/simulation-assurance-campaign-1`;
- implementation base: `main-v2` at
  `1b28d514882d407446dac3a366ecd35841a2a95a`.

Included scope is property-based and metamorphic verification, deterministic
generated scenarios, bounded campaign sharding, reproducible failure records
and shrinking where practical, accelerated long-duration simulation, Phase
3A/3B renderer invariants, and bounded PR, nightly, manual-deep, and
manual-soak workflows. The existing `aurora-realtime-audio-sim` and renderer
crates are the only execution engines.

Campaign levels are 500--1,000 PR scenarios, 10,000 nightly scenarios,
configurable 100,000-scenario manual deep runs, and representative accelerated
24-hour, 7-day, and 30-day manual soak scenarios. Initial evidence must include
1,000 smoke scenarios, 10,000 standard scenarios, the same fixed 1,000 seeds
repeated three times, representative accelerated 24-hour runs, and every
accepted legacy simulation fixture.

Every generated scenario must carry a deterministic seed, stable scenario ID,
bounded configuration, `truth_source=deterministic_simulation`, reproducible
command, bounded report, and deterministic failure category. Required
properties cover panic/deadlock/loop freedom, finite output, bounded memory,
zero warmed-up callback and render allocations, valid transitions, target-
qualified deterministic checksums, explicit fallback/fault observability,
channel isolation, normalized renderer energy, spread-zero Phase 3A
compatibility, layout-permutation equivalence, and allowed recovery terminal
states.

The milestone excludes renderer features, Phase 3C, elevation, HRTF,
Ambisonics, hardware abstractions, physical acceptance, and changes to public
traits or any other protected contract. It may not use physical terminology
for campaign evidence or modify accepted Simulation Sprint 1 records or tags.

Hardware dependency matrix:

- software-only criteria: bounded deterministic generator, stable scenario
  identity, reproducible reports, shrinking/replay, required invariants,
  workflows, tests, and documentation;
- deterministic-simulation criteria: all required campaign levels and initial
  executions complete without an unresolved campaign defect;
- host-observation criteria: bounded runtime and memory observations, labeled
  only `host_api_observation`;
- hardware-dependent criteria for this campaign: none;
- Phase 2 required for implementation: no;
- Phase 2 required for campaign acceptance: no;
- effect on Phase 2: none; every physical gate remains open and required;
- expected final classification after all campaign criteria pass: `ACCEPTED`.

Permitted truth sources are `unit_test`, `deterministic_simulation`, and
`host_api_observation`. The campaign cannot produce `physical_measurement`.
Its stop boundary is a reviewed, unmerged implementation pull request after
the initial executions, full validation, bounded artifact review, and formal
campaign evaluation. No Phase 3C or later product milestone may begin.

Formal evaluation completed on `2026-07-16` against implementation commit
`a04d009752622ffe30b3b2fa24043aaa56a11344`. All required campaign levels,
initial executions, software gates, deterministic-simulation gates, bounded
resource checks, and host-observation gates passed. The target-qualified
Windows x86-64 checksums and complete evidence are recorded in
`docs/acceptance/simulation-assurance-campaign-1.md`. This acceptance applies
only to the test campaign. It does not alter Phase 2 or satisfy any physical
gate of Phase 3A or Phase 3B.

### Authorized software-only milestone: Diagnostics & Telemetry Framework 1

The owner-authorized next milestone is **Diagnostics & Telemetry Framework 1**,
a hardware-independent control-plane framework for structured events, bounded
logging, atomic callback metrics, diagnostic snapshots, and error reports.

Current execution record:

- `execution_state`: `CLOSED`;
- `evaluation_classification`: `ACCEPTED`;
- implementation branch: `feature/diagnostics-framework-1`;
- implementation base: `main-v2` at
  `2a299d748afce842ed3b4816e34d6bc485851c40`.

Hardware dependency matrix:

- software-only criteria: deterministic event/log/report serialization,
  bounded storage, filtering, concurrent control-thread access, snapshot
  validity, documented event taxonomy, and callback-safe numeric metrics;
- deterministic-simulation criteria: deterministic simulation event and
  throughput evidence using supplied logical timestamps and seeds;
- host-observation criteria: benchmark statistics, platform/build metadata,
  and memory-capacity summaries labeled `host_api_observation`;
- hardware-dependent criteria: none for this framework;
- Phase 2 required for implementation: no;
- Phase 2 required for final acceptance: no;
- effect on Phase 2: none; physical validation remains open and required;
- expected final classification: `ACCEPTED` only after every software,
  simulation, review, and documentation criterion passes.

Protected contracts remain unchanged. Callback-reachable integration may only
update fixed numeric atomics; event construction, formatting, serialization,
locking, buffering, filesystem access, and output remain control-thread work.
The milestone may not alter renderer/DSP algorithms, audio behavior, callback
or buffer ownership, state/fault/device semantics, accepted records or tags,
or begin Phase 3C. Its full scope and stop boundary are recorded in
`docs/planning/diagnostics-telemetry-framework-1.md` and ADR 0012.

Formal evaluation passed on 2026-07-17 against implementation and defect-fix
commit `d1da1b967a50bd42242e237f40b7f604e57b5652`. Formatting, all-target
all-feature Clippy, 164 workspace tests with five explicitly ignored hardware
tests, all workspace benchmarks, `actionlint`, warning-free workspace rustdoc,
three repeated deterministic contract runs, and both required GitHub workflows
passed. The acceptance record is
`docs/acceptance/diagnostics-framework-1.md`. This software-only acceptance
changes no physical gate and introduces no physical claim or callback producer
wiring.

### Authorized software-only milestone: Configuration & Preset System 1

The owner-authorized next milestone is **Configuration & Preset System 1**, an
independent hardware-free control-plane library for deterministic configuration
intent and reusable presets.

Current execution record:

- `execution_state`: `CLOSED`;
- `evaluation_classification`: `ACCEPTED`;
- planned implementation branch: `feature/configuration-preset-system-1`;
- implementation base: `main-v2` at
  `f8d2fb9eaa45fce2e9a2d9f05fa363ced5a4a531`;
- expected final classification: `ACCEPTED`.

Hardware dependency matrix:

- software-only criteria: immutable typed models, versioned schemas, bounded
  validation, canonical serialization, structured errors, deterministic
  presets, migrations, redaction, fixtures, tests, benchmarks, and docs;
- deterministic criteria: byte-identical canonical fixtures and stable
  simulation-profile/replay metadata representation without simulator changes;
- host-observation criteria: benchmark and build-tool results labeled only
  `host_api_observation`;
- hardware-dependent criteria: none;
- Phase 2 required for implementation: no;
- Phase 2 required for final acceptance: no;
- effect on Phase 2: none; physical validation remains open and required.

Dependencies are the accepted Simulation Sprint 1 vocabulary, Phase 3A
point-source renderer vocabulary, Phase 3B horizontal-spread vocabulary, and
Diagnostics Framework 1 redaction policy. Conditional Phase 3A/3B status is not
upgraded by referencing their configuration vocabulary.

The milestone may define intent for engine operation, devices, formats,
routing, horizontal speaker layouts, existing renderers, buffering,
diagnostics, simulation, and presets. It may not probe hardware, assert device
availability, activate elevation rendering, change renderer/DSP behavior,
change a protected public contract, wire protected callbacks, implement runtime
hot reload or UI, start Phase 3C, or make a physical claim. Configuration
evidence may use milestone-local `unit_test`, `deterministic_serialization`, and
`schema_validation` labels; these do not extend the runtime diagnostic truth
source enum and map to `unit_test` when runtime diagnostics are emitted.

Its complete scope, bounds, lifecycle, and stop boundary are recorded in
`docs/planning/configuration-preset-system-1.md`.

Formal evaluation passed on 2026-07-17 against implementation and evidence
commit `106c9d03921cbea7506854cd2ec02e7861332a2b`. Formatting, all-target
all-feature Clippy, 182 workspace tests with five explicitly ignored hardware
tests, workspace benchmarks, Actionlint, warning-free workspace rustdoc, three
repeated deterministic contract runs, canonical fixture checksums, immutable
read allocation audit, and both required GitHub workflows passed. The
acceptance record is
`docs/acceptance/configuration-preset-system-1.md`. This software-only
acceptance changes no physical gate, protected audio contract, or runtime
callback and introduces no physical claim.

### Accepted software-only milestone: Runtime Assembly Contracts 1

**Runtime Assembly Contracts 1** is an authorized hardware-independent
control-plane milestone. Checkpoints A, B, C, D, and E are complete. The final
architectural evaluation accepted the milestone on 2026-07-18.

Current execution record:

- `authorization_state`: `AUTHORIZED`;
- `milestone_status`: `COMPLETE`;
- `execution_state`: `CLOSED`;
- `evaluation_classification`: `ACCEPTED`;
- `completed_checkpoint`: `E`;
- Checkpoint D execution state: `COMPLETE`;
- Checkpoint E execution state: `COMPLETE`;
- implementation crate: `aurora-runtime-assembly`;
- expected final classification: `ACCEPTED`.

Authoritative checkpoint sequence after ADR 0015 merges:

- Checkpoint A: completed isolated contracts under ADR 0014;
- Checkpoint B: completed deterministic derivation under ADR 0014;
- Checkpoint C: deterministic descriptive setup planning under ADR 0015;
- Checkpoint D: evidence, contract tests, public API documentation, and CI;
- Checkpoint E: stop and architectural acceptance review.

Checkpoint C merged through PR `#25` at merge commit
`dbf06a8d2f613086cccf3e6b43e8c7f714e00671`.
Checkpoint D merged through PR `#26` at merge commit
`93f36464cac429bf7e25b257e894aab64a72e2d4`.

Hardware dependency matrix:

- software-only criteria: immutable Aurora-owned contracts, deterministic
  renderer/routing/device intent derivation, explicit no-DSP state under the
  current schema, bounded capacities, checked arithmetic, structured setup
  errors, and, after ADR 0015 merges, a deterministic descriptive setup-stage
  and dependency plan, followed by tests and documentation;
- deterministic-simulation criteria: none;
- host-observation criteria: build and CI evidence only, with no device
  observation;
- hardware-dependent criteria: none;
- Phase 2 required for implementation: no;
- Phase 2 required for final acceptance: no;
- effect on Phase 2: none; physical validation remains open and incomplete.

The selected placement is the separate `aurora-runtime-assembly` crate depending
directly only on `aurora-config` and `aurora-core`. It may not add device
discovery or negotiation, stream opening, a running engine, thread/process or
callback work, renderer/DSP implementation dependencies, diagnostics producer
wiring, product audio behavior changes, or physical claims. The current schema
has no full DSP intent or maximum-object field; the plan must preserve those as
explicit absent/deferred capacities and must not invent defaults.

ADR 0015 governs the immutable setup plan derived from `PreparedRuntimePlan`.
It describes canonical setup stages and dependencies, unresolved device intent,
requested format intent, renderer/DSP/backend setup intent, and a
non-observational `SetupPlanComplete` terminal stage. It contains no runtime
objects and makes no execution, negotiation, readiness, host, or physical claim.

Checkpoint D's contract tests, public API documentation, dependency and
prohibited-scope evidence, and validation are complete. Checkpoint E's final
criterion-by-criterion architectural review found no blocking issue. The
acceptance record is `docs/acceptance/runtime-assembly-contracts-1.md`.

This is software-only control-plane acceptance. No runtime subsystem executed;
no renderer, DSP, backend, stream, callback, or engine was constructed; no
hardware was validated; and no physical or latency claim is made. The
acceptance does not authorize Phase 2 implementation, Phase 3C, or any later
milestone.
The complete contract, dependency matrix, tests, non-goals, and stop boundary
are recorded in `docs/planning/runtime-assembly-contracts-1.md`, ADR 0014, and
ADR 0015.

### Active software-only milestone: Runtime Plan Inspection 1

Before governance PR `#29` merged, no implementation milestone was authorized.
Phase 2 is open and blocked by missing hardware. Phase 3A and Phase 3B are
closed and conditionally accepted pending physical gates. Simulation Sprint 1,
Simulation Assurance Campaign 1, Diagnostics & Telemetry Framework 1,
Configuration & Preset System 1, and Runtime Assembly Contracts 1 are closed.

The single active successor is **Runtime Plan Inspection 1**. Its governance
amendment merged normally through PR `#29` at
`02deb93374b162d11b18b4010452254f3ecd1c18`. Its current record is:

- `authorization_state`: `AUTHORIZED`;
- `execution_state`: `IN_PROGRESS`;
- `evaluation_classification`: none;
- governance branch: `governance/runtime-plan-inspection-1`;
- Checkpoint A merge: PR `#30`,
  `b847344c8a64e2b605aead3b6cef8979f39b9916`;
- Checkpoint B merge: PR `#31`,
  `6c28ab826a40e84d3fcdbc07999a0414dce1b1ca`;
- implementation branch: `feature/runtime-plan-inspection-1-checkpoint-c`;
- active checkpoint: C;
- completed checkpoints: A and B;
- Checkpoint D: `NOT_STARTED`;
- milestone class: software-only control-plane inspection;
- Phase 2 required for implementation: no;
- Phase 2 required for acceptance: no;
- hardware-dependent criteria: none;
- expected terminal classification: `ACCEPTED`.

Checkpoint A created the separate `aurora-runtime-inspection` leaf crate.
Checkpoint B added bounded projection and redaction; both merged through PRs
`#30` and `#31`. Checkpoint C may format inspection-owned reports as bounded
deterministic JSON and human-readable text and add fixed conformance findings.

Permitted direct Aurora dependency:

```text
aurora-runtime-inspection --> aurora-runtime-assembly
```

Existing workspace Serde dependencies may be private implementation details.
No reverse dependency or direct configuration, diagnostics, renderer, DSP,
engine, backend, simulator, CPAL, CLI, scene, or audio-I/O dependency is
authorized.

The milestone may not modify or serialize accepted prepared-plan types,
deserialize or reconstruct plans, create hashes or fingerprints, access a host
or filesystem, construct or execute runtime resources, wire diagnostics, add a
CLI or control API, change a protected contract, start Phase 2 or Phase 3C, or
make a runtime, negotiated, physical, measured, or latency claim. Its objective,
dependency matrix, checkpoints, acceptance criteria, validation, risks, and
stop boundary are authoritative in ADR 0016 and
`docs/planning/runtime-plan-inspection-1.md`.

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

## 17.1 Simulation Sprint 1 — ACCEPTED

- Acceptance date: `2026-07-16`
- Accepted implementation commit: `cf7979b6c47d81387ab36d6e05b598d70cfe42cb`
- Freeze tag: `simulation-sprint-1-accepted`
- Tests: `112 passed`, `0 failed`, `5 explicitly ignored hardware-only tests`
- Deterministic 24-hour checksum: `a39ef2203486cb2d`
- Sample-pipeline checksum: `2cc43de3374b1db6`
- 7.1 routing checksum: `1388dc3ba1e02b73`

Acceptance commands:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo bench --workspace
cargo run -p aurora-cli --all-features -- simulate-duplex --profile usb-7-1 --duration-hours 24 --seed 12345 --report output\simulation\acceptance-usb-7-1-24h.json
cargo run -p aurora-cli --all-features -- simulate-latency --profile usb-7-1 --loopback-delay-frames 777 --jitter-frames 3 --noise-db -60 --seed 42
cargo run -p aurora-cli --all-features -- simulate-output-validation --profile usb-7-1 --layout 7.1 --report output\simulation\acceptance-validation-7-1.json
```

All six fixtures under `fixtures/simulation/fault_scenarios` were also run
through `simulate-duplex`. Generated reports are under `output/simulation/` and
are intentionally excluded from Git.

Release benchmark summary at 48 kHz / 256 frames: full-block medians were
`4.589 µs` (2 channels), `10.268 µs` (6), `13.090 µs` (8), and `18.599 µs`
(12). The one-hour USB 7.1 virtual benchmark median was `67.858 ms`. Host power
and thermal state caused variation between complete benchmark passes; no code
benchmark regression was accepted.

The accepted results are simulation facts only. The simulator does not validate
host drivers, physical clock quality, DAC/ADC latency, analog behavior, USB
scheduling, endpoint identity, or real unplug/replug behavior. Physical hardware
validation remains pending and no physical latency was measured in this sprint.

---

# 18. Standard report template for agents

Every agent must finish with this structure:

```text
Milestone:
Execution state:
Evaluation classification:
Scope completed:
Scope intentionally not completed:

Hardware dependency matrix:
- software-only criteria:
- simulation criteria:
- hardware-dependent criteria:
- Phase 2 required for implementation:
- Phase 2 required for final acceptance:
- conditional acceptance required:

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

The accepted milestone is:

> **Simulation Sprint 1 — deterministic virtual audio hardware and full-system validation**

It is frozen. Phase 2 physical hardware validation is open and incomplete.
Phase 3A and Phase 3B are conditionally accepted pending their physical gates.
Simulation Assurance Campaign 1 is accepted as a test-only milestone.
Diagnostics & Telemetry Framework 1 is accepted as a software-only milestone.
Configuration & Preset System 1 is accepted as a software-only milestone. No
acceptance authorizes Phase 3C or any later product milestone.
Phase 2 physical hardware validation remains open and incomplete.

Runtime Assembly Contracts 1 is `CLOSED` and `ACCEPTED` as a software-only
control-plane milestone; Checkpoints A, B, C, D, and E are complete. The
acceptance authorizes no runtime integration, Phase 2 implementation, Phase 3C,
or later milestone and makes no hardware, physical, or latency claim.

Runtime Plan Inspection 1 is the single active software-only milestone. Its
governance is merged, execution is `IN_PROGRESS`, Checkpoints A and B are
`COMPLETE`, and only Checkpoint C is active. Checkpoint D remains `NOT_STARTED`. Work must proceed one
reviewed checkpoint at a time from
`docs/planning/runtime-plan-inspection-1.md`.

The next agent must not:

- buy hardware;
- add HDMI;
- add codecs;
- add wireless;
- add AI;
- add a GUI;
- add or change renderer behavior;
- start Phase 3C;
- implement Runtime Plan Inspection 1 beyond Checkpoint C;
- serialize, mutate, reconstruct, hash, or fingerprint prepared plans;
- construct or execute runtime resources under the inspection milestone;
- add physical latency claims.

No milestone other than Runtime Plan Inspection 1 is authorized. Checkpoint C
must stop at deterministic bounded formatting and conformance evidence.

The simulator is complete and frozen. The campaign may exercise it but must not
duplicate it, change its accepted record, or use it as a substitute for Phase 2
evidence.

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

### Maintenance record: 2026-07-16

- Milestone: Simulation Sprint 1 acceptance and freeze.
- Changed sections: document status, current roadmap Phase 1, Simulation Sprint
  1 acceptance record, and immediate next action.
- Reason: formal independent acceptance completed with all required commands,
  deterministic validations, fault scenarios, and implementation audit passing.

### Maintenance record: 2026-07-16 -- hardware-blocked parallel development

- Milestone: roadmap governance amendment; no product milestone accepted.
- Changed sections: document status, truth-source rules, roadmap Phase 2 and
  Phase 3 gates, hardware-blocked parallel development policy, report template,
  and immediate next action.
- Reason: permit controlled software-only progress while preserving every open
  physical validation requirement and the Simulation Sprint 1 freeze.

### Maintenance record: 2026-07-16 -- milestone execution states

- Milestone: governance clarification; no product milestone accepted.
- Changed sections: hardware-blocked milestone lifecycle, Phase 3A current
  execution record, and standard report template.
- Reason: separate implementation progress from evidence-based milestone
  evaluation and remove the pre-implementation classification ambiguity.

### Maintenance record: 2026-07-16 -- Phase 3B scope authorization

- Milestone: Phase 3B governance definition; implementation not started.
- Changed sections: hardware-blocked parallel milestone authorization,
  dependency matrix, execution/evaluation lifecycle, and stop boundary.
- Reason: owner authorization for deterministic horizontal spread and irregular
  layout work that reuses Phase 3A without changing protected contracts.

### Maintenance record: 2026-07-16 -- Phase 3B evaluation

- Milestone: Phase 3B deterministic horizontal spread and irregular layouts.
- Changed sections: Phase 3B execution record, evidence summary, final
  classification, and acceptance-document reference.
- Reason: all non-hardware gates passed formal review; required physical gates
  remain open, so the milestone is conditionally accepted without an accepted
  tag.

### Maintenance record: 2026-07-16 -- Simulation Assurance Campaign 1 scope

- Milestone: test-only deterministic assurance campaign; implementation not
  started.
- Changed sections: hardware-blocked milestone authorization, dependency
  matrix, campaign levels, truth sources, acceptance criteria, and stop
  boundary.
- Reason: authorize large reproducible software verification without creating
  product features or substituting simulation for Phase 2 physical evidence.

### Maintenance record: 2026-07-16 -- Simulation Assurance Campaign 1 evaluation

- Milestone: test-only deterministic assurance campaign.
- Changed sections: campaign execution record, evidence summary, final
  classification, immediate next action, and acceptance-document reference.
- Reason: every campaign software and deterministic-simulation criterion
  passed; the campaign has no physical acceptance criterion and therefore
  closes as `ACCEPTED` without changing any physical gate.

### Maintenance record: 2026-07-17 -- Diagnostics & Telemetry Framework 1 scope

- Milestone: software-only diagnostics and telemetry framework; implementation
  started only after this record.
- Changed sections: hardware-blocked milestone authorization, dependency
  matrix, callback diagnostics boundary, immediate next action, and stop
  boundary.
- Reason: authorize production diagnostics infrastructure without changing
  audio semantics, real-time contracts, Phase 2, or any physical evidence gate.

### Maintenance record: 2026-07-17 -- Diagnostics & Telemetry Framework 1 evaluation

- Milestone: software-only diagnostics and telemetry framework.
- Changed sections: diagnostics execution record, evidence summary, final
  classification, acceptance-document reference, and immediate next action.
- Reason: all software, deterministic, documentation, allocation, review,
  benchmark, and remote CI criteria passed; the milestone has no physical gate
  and therefore closes as `ACCEPTED` without changing Phase 2 or authorizing
  Phase 3C.

### Maintenance record: 2026-07-17 -- Configuration & Preset System 1 scope

- Milestone: software-only versioned configuration and preset infrastructure;
  implementation not started.
- Changed sections: hardware-blocked milestone authorization, dependency
  matrix, truth-source mapping, immediate next action, and stop boundary.
- Reason: authorize deterministic bounded control-plane configuration without
  changing audio semantics, protected contracts, Phase 2, or physical evidence.

### Maintenance record: 2026-07-17 -- Configuration & Preset System 1 evaluation

- Milestone: software-only versioned configuration and preset infrastructure.
- Changed sections: configuration execution record, evidence summary, final
  classification, acceptance-document reference, and immediate next action.
- Reason: all software, determinism, bounds, migration, redaction, review,
  benchmark, documentation, and remote CI criteria passed; the milestone has no
  physical gate and closes as `ACCEPTED` without changing Phase 2 or authorizing
  Phase 3C.

### Maintenance record: 2026-07-17 -- Runtime Assembly Contracts 1 scope

- Milestone: software-only immutable runtime preparation contracts;
  implementation not started.
- Changed sections: hardware-blocked milestone authorization, dependency
  direction, immediate next action, and stop boundary.
- Reason: authorize a later deterministic setup-only plan from validated
  configuration without changing audio semantics, protected contracts,
  callbacks, device behavior, Phase 2, or physical evidence.

### Maintenance record: 2026-07-18 -- Runtime setup-planning governance

- Milestone: Runtime Assembly Contracts 1, with Checkpoints A and B complete.
- Changed sections: execution record, remaining checkpoint authorization,
  dependency boundary, immediate next action, and stop boundary references.
- Reason: propose a deterministic descriptive setup-planning layer governed by
  ADR 0015 before final evidence review, without implementing it or changing
  runtime behavior, dependencies, Phase 2, Phase 3C, or physical evidence.

### Maintenance record: 2026-07-18 -- Runtime Assembly Checkpoint D evidence

- Milestone: Runtime Assembly Contracts 1, with Checkpoints A, B, and C merged.
- Changed sections: execution record, active checkpoint, immediate next action,
  and evidence boundary.
- Reason: reconcile merged Checkpoint C and record Checkpoint D evidence work
  without evaluating or accepting the milestone, starting Checkpoint E,
  changing runtime behavior, or introducing a physical claim.

### Maintenance record: 2026-07-18 -- Checkpoint D post-merge status

- Milestone: Runtime Assembly Contracts 1; milestone remains `IN_PROGRESS` and
  `NOT_EVALUATED`.
- Changed sections: Checkpoint completion record, immediate next action, and
  post-merge evidence status.
- Reason: PR `#26` merged at
  `93f36464cac429bf7e25b257e894aab64a72e2d4`; reconcile Checkpoint D from its
  review-time `IN_PROGRESS` state to `COMPLETE` while leaving Checkpoint E
  `NOT_STARTED` and making no acceptance, runtime, or physical claim.

### Maintenance record: 2026-07-18 -- Runtime Assembly final evaluation

- Milestone: Runtime Assembly Contracts 1.
- Changed sections: execution record, evaluation classification, completed
  checkpoints, immediate next action, and acceptance-document reference.
- Reason: Checkpoint E reviewed the complete merged implementation and evidence
  against ADRs 0014 and 0015 with no blocking finding. Close as `ACCEPTED`
  software-only control-plane work without runtime execution, hardware
  validation, physical claims, or later-milestone authorization.

### Maintenance record: 2026-07-18 -- Runtime Plan Inspection 1 scope

- Milestone: proposed software-only read-only runtime plan inspection; no
  implementation started.
- Changed sections: consolidated status, hardware-blocked successor queue,
  proposed milestone authorization, immediate next action, roadmap, ADR index,
  and architecture boundary.
- Reason: every previously authorized implementation milestone is closed or
  blocked by physical hardware. Define exactly one bounded deterministic
  consumer of accepted prepared plans without changing protected contracts,
  constructing runtime resources, accessing hardware, or authorizing Phase 2,
  Phase 3C, or any additional milestone.

### Maintenance record: 2026-07-18 -- Runtime Plan Inspection Checkpoint A

- Milestone: Runtime Plan Inspection 1; execution moves to `IN_PROGRESS` with
  Checkpoint A only.
- Changed sections: consolidated status, active milestone record, immediate
  next action, roadmap lifecycle, architecture workspace inventory, and
  Checkpoint A implementation record.
- Reason: governance PR `#29` merged normally; authorize the isolated leaf
  crate and empty public contract markers without plan access, projection,
  formatting, serialization, integration, runtime execution, hardware work,
  protected-contract changes, or later-checkpoint work.

### Maintenance record: 2026-07-18 -- Runtime Plan Inspection Checkpoint C

- Milestone: Runtime Plan Inspection 1; Checkpoints A and B are complete and
  Checkpoint C is the only active implementation scope.
- Changed sections: consolidated status, active milestone record, immediate
  next action, roadmap lifecycle, architecture boundary, and inspection-format
  documentation.
- Reason: Checkpoint B merged through PR `#31`; authorize only deterministic
  bounded formatting and inspection-owned conformance evidence without plan
  serialization, integration, runtime execution, Checkpoint D evaluation,
  hardware work, or physical claims.

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
