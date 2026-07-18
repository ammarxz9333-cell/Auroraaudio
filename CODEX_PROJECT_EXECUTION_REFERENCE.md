# Aurora Codex Project Execution Reference

> Status: canonical agent execution guide under review with Issue #44 / PR #58.
> This file does not authorize a later checkpoint by itself. `PROJECT_EXECUTION_STATE.md`
> selects the active work item; this file defines how an implementation agent must execute it.

## 1. Purpose

This document is the operational reference for Codex and other coding agents working on Aurora.
It exists to prevent scope drift, duplicated architecture, fabricated evidence, stale-roadmap work,
and changes based on old pull requests.

Every agent must use this sequence:

1. identify the exact active work item;
2. verify the required base commit and PR state;
3. read the authoritative files listed below;
4. audit existing ownership before creating code;
5. implement one bounded checkpoint only;
6. run the complete validation contract;
7. publish exact evidence and stop;
8. never begin the next checkpoint automatically.

## 2. Authoritative-source order

When documents conflict, apply this order:

1. `PROJECT_EXECUTION_STATE.md` for the current active work item and immediate sequence;
2. `AURORA_MASTER_REFERENCE.md` for project-wide invariants and evidence language;
3. accepted ADRs in `docs/adr/` for architecture ownership and dependency direction;
4. the active GitHub issue and its accepted checkpoint boundary;
5. `docs/roadmaps/immersive-wireless-audio-execution-roadmap.md` for future ordering;
6. this execution reference for agent procedure;
7. other planning documents and historical PR descriptions.

Open or closed historical PRs are not authoritative when their branch predates the current
`main-v2` state. Never copy implementation from a stale PR without a fresh base-to-head audit.

## 3. Current product truth

Aurora is an active Rust spatial-audio project with strong software validation but incomplete
product integration and incomplete physical validation.

Implemented or accepted software foundations include:

- Aurora-owned core geometry and channel-role contracts;
- renderer API and existing basic renderers;
- horizontal VBAP and horizontal spread software evidence;
- realtime engine and CPAL/simulator backend foundations;
- deterministic simulator and stress campaign infrastructure;
- diagnostics and telemetry contracts;
- configuration and preset contracts;
- runtime assembly and runtime inspection contracts;
- GeometricBinaural Checkpoint A stabilization.

Important limitations that must remain explicit:

- GeometricBinaural is not HRTF and has no HRIR, SOFA, convolution, pinna, or elevation cues;
- moving geometric delay is updated at block boundaries and is not yet claimed click-free;
- the current dynamic-delay capacity is an explicit fixed implementation bound, not a
  scene-derived universal capacity;
- physical 5.1/7.1 routing, real endpoint behavior, round-trip latency, acoustic behavior,
  and long-running hardware stability remain unvalidated;
- multiple architectural crates describe plans and evidence, but a complete production
  vertical slice is still a future integration objective.

## 4. Repository architecture and ownership

### 4.1 Core and domain contracts

Own stable domain values only. Do not add host access, filesystem access, timing, or rendering.

### 4.2 Renderer crates

Own spatial gain and renderer-reported delay calculation. Renderer callback-facing paths must:

- use caller-owned or preallocated storage;
- avoid locks, blocking, filesystem access, formatting, logging, process launch, and allocation;
- validate configuration on the control thread;
- return structured errors;
- preserve deterministic channel ordering.

### 4.3 DSP crates

Own signal processing after renderer decisions. They do not own scene semantics, product claims,
device discovery, or transport.

### 4.4 Realtime engine and audio backends

Own callback orchestration, bounded buffers, backend negotiation, and process status. Control-plane
work must not leak into callbacks.

### 4.5 Simulation

Own deterministic virtual hardware and fault scenarios. Simulation evidence must never be labeled
as physical measurement.

### 4.6 Evaluation

`aurora-evaluation` is a leaf consumer of existing APIs. It may evaluate, classify, and report.
It may not change renderer, DSP, callback, backend, transport, or hardware behavior.

Evaluation evidence classes:

- deterministic contract evidence;
- deterministic regression fingerprints;
- host-observed performance evidence;
- partial deterministic capacity accounting;
- externally supplied allocation/profiler evidence;
- simulated evidence;
- physical evidence.

These classes must never be silently merged.

### 4.7 CLI

The CLI owns filesystem paths, artifact writing, argument parsing, and human-facing command output.
It must not become the owner of renderer mathematics or duplicate validation logic.

### 4.8 Runtime assembly and inspection

These crates own immutable plans and read-only projections. They do not prove that a real runtime
was constructed. Future materialization must be justified by a concrete product vertical slice,
not by governance expansion alone.

## 5. Evidence and truth rules

### 5.1 Aggregate status

A required failed finding produces `fail`.
A required unobserved finding produces `incomplete` unless another required finding failed.
`not_observed` must never be converted to `pass`.
Optional observations must be explicitly marked optional and excluded from the required aggregate.

### 5.2 Deterministic versus host-observed output

A deterministic artifact may contain only values reproducible under its declared determinism class.
Host timing, process RSS, machine identity, timestamps, random paths, thread identifiers, and
scheduler-dependent values must be written to a separate host-observation artifact or clearly
excluded from deterministic byte comparisons.

### 5.3 Fingerprints

FNV-1a 64-bit values are regression fingerprints only. They are not cryptographic integrity proofs.
A future artifact-integrity layer may use SHA-256 or another approved cryptographic digest after its
dependency and schema impact are reviewed.

### 5.4 Memory evidence

Static byte accounting is partial capacity accounting, not process working set and not peak RSS.
Every report must state coverage and exclusions. Missing profiler observations remain null and
`not_observed`.

### 5.5 Performance evidence

Host timing is advisory unless a checkpoint explicitly defines a controlled runner class and a
reviewed regression policy. Generic CI timing must not determine the canonical deterministic PASS.

### 5.6 Discontinuity evidence

Adjacent-sample delta is a signal-slope proxy, not a general click detector. It must not be used to
claim click-free output. Renderer-specific click validation requires boundary-local residual or
reference-based metrics and multiple suitable signals.

## 6. Immediate cleanup and execution sequence

### Stage 0 — Correct Issue #44 / PR #58

Required corrections before merge:

1. required `not_observed` evidence makes the aggregate `incomplete`;
2. deterministic summary excludes host timing;
3. host timing is advisory and excluded from canonical deterministic validation;
4. FNV fields and documentation are classified as regression fingerprints;
5. partial memory accounting is labeled partial and lists exclusions;
6. adjacent-sample delta is labeled a proxy and excluded from click-free claims;
7. tests cover incomplete aggregation and deterministic-summary stability;
8. documentation and CLI descriptions use the same evidence terminology.

Stop after the corrected PR passes independent review and CI. Do not merge automatically.

### Stage 1 — Governance reconciliation after Issue #44 merge

In one documentation-only change:

- record Issue #44 as merged;
- close or mark stale planning PRs as superseded/on-hold where appropriate;
- place Issue #43 Checkpoint B explicitly in the active sequence;
- keep hardware validation open;
- forbid new passive governance layers unless they remove a product blocker.

### Stage 2 — Issue #43 Checkpoint B: continuous moving-source delay

Goal: eliminate block-step delay behavior without claiming HRTF.

Required work:

- define interpolation semantics and bounds;
- preserve realtime allocation-free behavior;
- derive or validate delay capacity from accepted runtime inputs;
- test stationary equivalence, movement continuity, partial blocks, extreme finite movement,
  repeated output, and allocation behavior;
- evaluate with the merged evaluation framework using a renderer-specific boundary metric;
- preserve Checkpoint A ITD/ILD polarity and normalization unless the issue explicitly changes them.

Non-goals: HRTF, HRIR, SOFA, elevation, room simulation, networking, hardware claims.

### Stage 3 — Issue #45: capability registry

Goal: expose honest capability state without creating another large governance subsystem.

Every capability must report one of:

- implemented;
- software_validated;
- simulated_only;
- hardware_blocked;
- planned;
- unsupported.

The registry must be data-driven, bounded, versioned, and consumed by the CLI. It must distinguish
feature availability from acceptance evidence.

### Stage 4 — Issue #38: first offline 3D loudspeaker vertical slice

Goal: build a real product path from scene and PCM input to multichannel output using one owned
renderer path and canonical artifacts.

Required proof:

- finite 3D geometry handling;
- deterministic routing;
- bounded channel/object counts;
- clear elevation limitations or implementation;
- end-to-end output artifact;
- evaluation integration;
- no realtime or hardware claim unless separately measured.

### Stage 5 — Issue #46: SOFA/HRIR data backend

Own data ingestion and validation only. Native/unsafe code, if required, must remain inside a small
adapter crate with reviewed lifetime, bounds, error, and threading contracts.

### Stage 6 — True Aurora HRTF renderer

May begin only after the data backend exists. Must define convolution ownership, filter switching,
latency, state size, interpolation, determinism class, and realtime strategy.

### Stage 7 — Integrated runtime vertical slice

Connect validated configuration to actual renderer/DSP/backend construction with explicit ownership.
This is the point where passive plans must prove practical value.

### Stage 8 — Network simulation and transport

Simulator first, then packet transport. Separate deterministic network evidence from physical LAN/Wi-Fi
evidence. Use bounded realtime queues and keep async runtimes outside audio callbacks.

### Stage 9 — Receiver and multiroom

Implement receiver nodes, clock/drift handling, synchronization, reconnect, and bounded buffering.
Do not claim room-level synchronization before physical measurement.

### Stage 10 — Physical validation

Execute the existing hardware gates using suitable input, loopback, and independently identifiable
multichannel outputs. Record exact hardware, drivers, sample formats, negotiated buffers, and
measurement methods.

## 7. Standard agent workflow

For every checkpoint, Codex must perform the following.

### 7.1 Preflight

- fetch `main-v2` and verify the required base SHA;
- verify the preceding PR is merged, not merely green or mergeable;
- verify the target issue is open and authorized;
- inspect all open PRs that touch the same files;
- read this reference and the authoritative sources;
- state the exact allowed and prohibited scope.

If the required base or authorization is false, stop without creating a branch or changing files.

### 7.2 Audit before code

- locate the existing owner crate and API;
- inspect dependency direction;
- identify protected public contracts;
- identify allocation-sensitive paths;
- identify existing tests and fixtures;
- identify stale duplicate implementation;
- determine the evidence truth source for every acceptance criterion.

### 7.3 Implementation discipline

- create one branch for one checkpoint;
- use small atomic commits;
- avoid broad formatting or unrelated renames;
- never duplicate logic already owned by another crate;
- use checked arithmetic for externally influenced sizes;
- reject non-finite values at control-thread boundaries;
- preserve stable ordering explicitly;
- do not add dependencies without documenting MSRV, license, ownership, and callback impact;
- do not add unsafe code outside an explicitly authorized adapter boundary.

### 7.4 Required validation

Run from a clean checkout at the exact head SHA:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo +1.78.0 check --workspace --all-targets --all-features --locked
actionlint
git diff --check main-v2...HEAD
cargo bench --workspace
```

Also run every checkpoint-specific deterministic, allocation, simulation, artifact, and platform test.
Never report PASS for a command that was not run. Distinguish local from remote results.

### 7.5 PR report

Every implementation PR must state:

- base SHA and head SHA;
- issue/checkpoint;
- exact files changed;
- architecture and dependency impact;
- protected contracts changed or explicitly unchanged;
- deterministic evidence;
- host-observation evidence;
- simulated evidence;
- physical evidence or explicit absence;
- ignored/unobserved gates;
- known limitations;
- complete validation commands and results;
- confirmation that the next checkpoint was not started.

### 7.6 Review and merge

- implementation remains Draft until independent review;
- blocking findings require corrections on the same branch;
- review is repeated at the corrected SHA;
- merge only after explicit PASS and green required checks;
- verify the merge commit and canonical branch state;
- update current-state documentation only after the implementation is actually merged;
- do not begin the next checkpoint in the merge operation.

## 8. File organization rules

- root files: only canonical entry points and repository-wide references;
- `docs/adr/`: durable architecture decisions, never status diaries;
- `docs/roadmaps/`: future dependency-ordered plans;
- `docs/planning/`: bounded milestone specifications that are not current-state truth;
- `docs/evidence/`: reproducible evidence tied to an exact commit;
- `docs/acceptance/`: terminal reviewed milestone decisions only;
- crate-local `README` or rustdoc: implementation ownership and API usage;
- fixtures grouped by subsystem and schema;
- generated reports and WAV files remain under `target/` or external artifact storage, never committed.

Do not create a new top-level planning document when an existing canonical file can own the information.
Historical documents must be labeled historical and must not redefine the active sequence.

## 9. Open-PR hygiene

At each governance reconciliation:

- one active implementation PR is preferred;
- stale PRs must be closed, rebased, or labeled superseded/on-hold;
- a hardware-blocked PR may remain open only when its blocked state and stale base are explicit;
- planning-only PRs must not appear as active implementation;
- agents must not merge a stale PR solely because GitHub marks it mergeable.

## 10. Product-level definition of done

Aurora is not product-complete until all of the following exist:

- integrated configuration-to-runtime construction;
- stable offline and realtime rendering paths;
- honest capability reporting;
- true HRTF if advertised;
- transport and receiver integration if multiroom is advertised;
- deterministic simulation evidence;
- physical routing, latency, stability, and synchronization evidence;
- reproducible installation and operator documentation;
- no critical hardware gate represented as software PASS.

## 11. Final instruction to Codex

Implement the smallest authorized product-improving checkpoint. Preserve evidence honesty. Do not
invent capability, measurement, readiness, or acceptance. Stop after the requested checkpoint and
return exact evidence for independent review.
