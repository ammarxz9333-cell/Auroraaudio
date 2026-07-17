# Configuration & Preset System 1

## Authorization

- Milestone: Configuration & Preset System 1
- Milestone kind: hardware-independent control-plane infrastructure
- Planned implementation branch: `feature/configuration-preset-system-1`
- `execution_state`: `CLOSED`
- `evaluation_classification`: `ACCEPTED`
- Expected terminal classification: `ACCEPTED`
- Phase 2 required for implementation: no
- Phase 2 required for final acceptance: no
- Hardware acceptance criteria: none

Implementation may begin only after the governance-only pull request containing
this definition is merged normally into `main-v2`.

Governance PR `#16` passed CI and merged normally at
`f8d2fb9eaa45fce2e9a2d9f05fa363ced5a4a531`. Implementation therefore began
on the dedicated branch from that exact base.

The implementation reached its stop boundary on 2026-07-17. Local format,
Clippy, 182 workspace tests with five hardware-only tests ignored, strict
Rustdoc, Actionlint, workspace benchmarks, repeated deterministic contracts,
fixture checksums, migration, redaction, and immutable-read allocation checks
passed before required pull-request CI and criterion-by-criterion review.

CI run `53` and Simulation Assurance PR Smoke run `7` passed on evaluated
commit `106c9d03921cbea7506854cd2ec02e7861332a2b`. Criterion-by-criterion review
found no unresolved in-scope defect, protected-contract change, or physical
criterion. The milestone therefore closed as `ACCEPTED`; complete evidence is
in `docs/acceptance/configuration-preset-system-1.md`.

## Purpose

Provide Aurora with an immutable, validated, versioned, deterministic
control-plane representation for engine intent, device-selection intent, audio
formats, routing, speaker layouts, renderer selection, horizontal spread,
buffering, diagnostics, simulations, and reusable presets.

Configuration describes intent only. It does not prove that a device exists,
that a format was negotiated, or that physical behavior was observed.

## Included Scope

- immutable validated configuration models;
- explicitly versioned schemas and bounded migrations;
- canonical JSON and deterministic human-readable diagnostics;
- structured, stable validation errors;
- bounded preset storage and materialization;
- deterministic, non-recursive preset composition with explicit precedence;
- redacted control-thread diagnostic representations;
- CLI-independent library APIs;
- bounded fixtures, tests, benchmarks, documentation, and CI gates.

## Excluded Scope

- live device probing as a truth source or physical device acceptance;
- real-time callback mutation, producer wiring, or automatic reconfiguration;
- UI, network control, remote APIs, cloud synchronization, or databases;
- Phase 3C, elevation rendering, HRTF, Ambisonics, room correction, codecs, or
  Dolby features;
- renderer or DSP algorithm changes;
- changes to accepted tags, records, protected traits, callback ownership,
  fault/state semantics, device-selection behavior, or truth terminology.

An optional elevation field may exist only as inactive reserved metadata. Any
attempt to activate elevation rendering must return an unsupported-field error.

## Dependency Matrix

| Dependency | Decision |
| --- | --- |
| Simulation Sprint 1 | Accepted dependency for deterministic simulation vocabulary |
| Phase 3A | Conditional dependency for point-source VBAP configuration vocabulary only |
| Phase 3B | Conditional dependency for horizontal spread vocabulary only |
| Diagnostics Framework 1 | Accepted dependency for redacted control-thread reporting policy |
| Phase 2 | Not required for implementation or acceptance; remains open and unchanged |

Software acceptance criteria are typed models, bounded validation, canonical
serialization, deterministic presets, migration, redaction, fixtures, tests,
documentation, and host-observed benchmarks. Deterministic simulation criteria
are limited to stable simulation-profile representation and replay metadata;
this milestone does not execute or modify the accepted simulator. There are no
hardware-dependent criteria.

## Truth Sources

Permitted evidence labels are:

- `unit_test` for deterministic in-process validation;
- `deterministic_serialization` for byte-identical canonical output;
- `schema_validation` for version and fixture conformance;
- `host_api_observation` for benchmark and build-tool observations.

The first three are milestone-local evidence categories, not additions to the
runtime diagnostic `TruthSource` enum. They must map to `unit_test` when a
runtime diagnostic truth source is required. No result may use
`physical_measurement`.

## Required Models

The implementation must cover schema metadata, engine configuration, audio
format intent, device-selection intent, routing, speaker layouts, Basic/Phase
3A/Phase 3B renderer selection, buffering policy, diagnostics policy,
simulation profiles, and typed presets.

Preset composition, if implemented, uses explicit precedence, forbids cycles,
has a fixed depth limit, reports conflicts structurally, and always produces a
deterministic materialized result. Migration must be explicit, deterministic,
offline, bounded, loss-aware, and validate its destination.

## Bounds

The implementation must publish and enforce finite limits for speakers,
routes, presets, tags, strings, composition depth, serialized input, and
retained migration diagnostics. No unbounded recursion, collection, error
aggregation, or arbitrary expression evaluation is permitted.

## Protected Contracts

The milestone may not change Aurora-owned public traits, renderer or DSP
semantics, channel order, WAV masks, callback/buffer ownership, state/fault
semantics, device-selection behavior, or diagnostic truth-source terminology.
The new crate must use `#![forbid(unsafe_code)]` and remain outside protected
callbacks. A required change to any protected contract stops implementation.

## Lifecycle And Acceptance

The lifecycle uses `NOT_STARTED`, `IN_PROGRESS`, `READY_FOR_EVALUATION`, and
`CLOSED`. Evaluation classification remains absent until evaluation begins.
The milestone may close as `ACCEPTED` only after every software criterion,
review, local validation, and required remote CI check passes with no unresolved
in-scope defect.

## Stop Boundary

Stop after the validated configuration and preset infrastructure, bounded
fixtures, documentation, benchmarks, formal evaluation, and an unmerged
implementation pull request exist. Do not implement runtime hot reload, CLI
commands, automatic reconfiguration, UI, Phase 3C, or hardware integration.
