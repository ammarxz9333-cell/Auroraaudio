# Hardware-Blocked Parallel Development

This document is a working guide to the policy in
`AURORA_MASTER_REFERENCE.md` Section 16.1. The master reference remains
authoritative if this summary differs from it.

## Purpose

Aurora may continue deterministic software development while Phase 2 physical
hardware validation is environmentally blocked. Parallel work cannot replace
physical evidence, alter protected contracts implicitly, or close Phase 2.

## Required milestone record

Before code is written, a parallel milestone must record:

| Field | Required content |
| --- | --- |
| Execution state | One master-reference execution state |
| Evaluation classification | `none` until required, then one master-reference classification |
| Software criteria | Unit and offline integration behavior |
| Simulation criteria | Deterministic virtual-backend behavior, if applicable |
| Hardware criteria | Every physical result still required |
| Implementation dependency | Whether Phase 2 blocks writing the software |
| Acceptance dependency | Whether Phase 2 blocks final acceptance |
| Truth sources | One approved source for each reported result |
| Protected contracts | Traits and invariants that must remain unchanged |
| Stop boundary | Explicitly excluded work and claims |

Execution state and evaluation classification are separate fields. No
classification is required while `execution_state=IN_PROGRESS`; a classification
is mandatory at `READY_FOR_EVALUATION`, and `CLOSED` requires one. `IN_PROGRESS`
is not acceptance. Only `ACCEPTED` fully closes a milestone as accepted;
conditional or blocked classifications must remain visible in commits, pull
requests, reports, and planning documents.

## Truth sources

Use exactly one source label per result:

- `unit_test`
- `deterministic_simulation`
- `virtual_audio_backend`
- `host_api_observation`
- `physical_measurement`

Only `physical_measurement` is physical evidence. A virtual cable, configured
delay, host timestamp, endpoint descriptor, or simulated clock is not measured
hardware behavior.

## Protected boundaries

Parallel work does not authorize silent changes to public traits, callback or
buffer ownership, real-time allocation and bounded-memory guarantees, fault or
state-machine semantics, device selection, or truth-source terminology. Use a
separate ADR or master amendment before changing any of them.

## Phase 2 blocker

Phase 2 is open and incomplete because the current environment lacks:

- a usable 2-channel input endpoint;
- a physical loopback path;
- an exact 6-channel or 8-channel output endpoint;
- independently identifiable multichannel physical paths.

This is `BLOCKED_BY_HARDWARE`, not pass or failure evidence.

## First recommended parallel milestone

**Phase 3A -- Deterministic Offline Spatial Rendering Improvements** is the
first safe candidate when constrained to existing contracts.

Current record: `execution_state=CLOSED`,
`evaluation_classification=CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`, branch
`phase-3a`, based on `main-v2` at
`bc2cf637bb8dc5d526a28271e1070efcff1e09d4`. Implementation reached its stop
boundary and formal software review passed on 2026-07-16, but the
hardware-dependent gates remain open.

| Dependency | Decision |
| --- | --- |
| Software-only criteria | Deterministic VBAP geometry, finite output, canonical routing, offline fixtures, focused benchmarks |
| Simulation criteria | Repeatable virtual-backend routing where the existing simulator is relevant; no new simulator |
| Hardware criteria | Physical multichannel routing, endpoint behavior, stability, and audible/speaker-dependent conclusions |
| Phase 2 required for implementation | No, for offline work using existing contracts |
| Phase 2 required for final acceptance | Yes, for hardware-relevant claims and gates |
| Final evaluated classification | `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE` |

Stop before public trait changes, live default changes, channel-order changes,
HRTF dependencies, codecs, HDMI, networking, wireless audio, GUI, AI, room
calibration, or any physical claim.

## Authorized successor: Phase 3B

The owner-authorized successor is **Phase 3B -- Deterministic Horizontal Source
Spread and Irregular Layout Support**. Its current record is
`execution_state=CLOSED`,
`evaluation_classification=CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`, on
branch `phase-3b-horizontal-spread` based on the merged governance definition.

It is parallel-safe because it extends the concrete Phase 3A VBAP crate without
changing the Aurora-owned renderer trait or any real-time, state, fault, device,
default, or reporting contract. Unit tests, deterministic fixture sweeps, and
host observations can establish its software claims. Audible behavior, physical
routing, endpoint behavior, level consistency, and hardware stability remain
conditional on Phase 2. The mandatory matrix, criteria, and stop boundary are in
`docs/planning/phase-3b-scope.md`.

## Authorized test-only successor

**Simulation Assurance Campaign 1 -- Massive Deterministic Property and Stress
Testing** is authorized only after its governance amendment merges. It reuses
the existing accepted simulator and Phase 3A/3B renderers and adds no product
feature or second simulator.

| Dependency | Decision |
| --- | --- |
| Software-only criteria | Bounded deterministic generation, property/metamorphic invariants, replay, shrinking, reports, workflows, and documentation |
| Simulation criteria | Required smoke, standard, repeated-seed, accelerated 24-hour, legacy-fixture, nightly, deep, and soak campaign levels |
| Hardware criteria for the campaign | None |
| Phase 2 required for implementation | No |
| Phase 2 required for campaign acceptance | No |
| Effect on Phase 2 | None; physical validation remains open and required |
| Expected final classification | `ACCEPTED` only after every campaign software criterion passes |

Campaign evidence may use only `unit_test`, `deterministic_simulation`, and
`host_api_observation`. It cannot close a hardware gate or use
`physical_measurement`. Protected contracts, accepted tags, and accepted
Simulation Sprint 1 records remain immutable. The full definition is in
`docs/planning/simulation-assurance-campaign-1.md`.

## Authorized software-only successor

**Diagnostics & Telemetry Framework 1** is authorized on
`feature/diagnostics-framework-1` as a hardware-independent control-plane
milestone. Its callback boundary is fixed numeric atomics only; event creation,
formatting, serialization, buffering, locks, and output are prohibited from
callback-reachable code.

Final record: `execution_state=CLOSED`,
`evaluation_classification=ACCEPTED` on 2026-07-17. This software-only
acceptance changes no Phase 2 gate and authorizes no later milestone.

| Dependency | Decision |
| --- | --- |
| Software-only criteria | Deterministic structured events, bounded logs, metrics, snapshots, reports, tests, benchmarks, and documentation |
| Simulation criteria | Logical-timestamp and deterministic-seed diagnostics over existing simulation facts |
| Hardware criteria | None |
| Phase 2 required for implementation | No |
| Phase 2 required for acceptance | No |
| Effect on Phase 2 | None; physical validation remains open and required |
| Expected final classification | `ACCEPTED` only after all framework criteria pass |

No existing public trait, callback ownership, renderer/DSP algorithm, state or
fault semantic, device selection behavior, truth-source term, accepted record,
or accepted tag may change. The complete matrix and stop boundary are in
`docs/planning/diagnostics-telemetry-framework-1.md`.

## Authorized software-only successor: Configuration & Preset System 1

**Configuration & Preset System 1** is authorized only after its governance
amendment merges. It is an independent control-plane library for immutable,
versioned configuration intent and bounded reusable presets.

| Dependency | Decision |
| --- | --- |
| Software-only criteria | Typed schemas, bounded validation, canonical serialization, presets, migrations, redaction, tests, benchmarks, and documentation |
| Deterministic criteria | Byte-identical fixture serialization and replay metadata representation; no simulator changes |
| Hardware criteria | None |
| Phase 2 required for implementation | No |
| Phase 2 required for acceptance | No |
| Effect on Phase 2 | None; physical validation remains open and required |
| Expected final classification | `ACCEPTED` only after every software criterion passes |

Configuration is intent, never proof that a device exists or a format was
negotiated. The crate remains CLI-independent and outside protected callbacks.
No public trait, renderer/DSP behavior, device selection semantics, callback
ownership, state/fault behavior, accepted record/tag, or runtime diagnostic
truth-source vocabulary may change. The complete scope and stop boundary are in
`docs/planning/configuration-preset-system-1.md`.

## Git workflow

- branch each parallel milestone from `main-v2`;
- keep `phase-2-physical-hardware-validation` open independently;
- do not merge automatically;
- never move or replace an accepted tag;
- state conditional status in the pull request and milestone documentation.
