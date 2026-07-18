# Roadmap

## Milestone Status

- Phase 0 foundations: completed.
- Simulation Sprint 1: **ACCEPTED and frozen on 2026-07-16**.
- Phase 2 physical hardware validation: **OPEN, INCOMPLETE, and currently
  BLOCKED_BY_HARDWARE**.
- Parallel software development: permitted only by
  `AURORA_MASTER_REFERENCE.md` Section 16.1.

The acceptance record is in `docs/acceptance/simulation-sprint-1.md`. The
authoritative roadmap, classifications, and gates remain in
`AURORA_MASTER_REFERENCE.md`.

## Hardware-Blocked Parallel Queue

The first technically safe candidate was **Phase 3A -- Deterministic Offline
Spatial Rendering Improvements**. Its software evaluation passed on 2026-07-16.

- `execution_state`: `CLOSED`
- `evaluation_classification`: `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`
- branch: `phase-3a`
- branch base: `main-v2` at
  `bc2cf637bb8dc5d526a28271e1070efcff1e09d4`

The implementation and formal software review are complete. Hardware-dependent
gates remain open, so this is not full milestone acceptance and no accepted tag
exists.

| Gate | Current decision |
| --- | --- |
| Software-only criteria | VBAP geometry and bounded deterministic offline rendering using existing contracts |
| Simulation criteria | Existing simulator may verify routing; no duplicate simulator may be added |
| Hardware criteria | Physical 5.1/7.1 routing, endpoint behavior, stability, and audible conclusions remain pending |
| Phase 2 required to implement | No, for the constrained offline scope |
| Phase 2 required for final acceptance | Yes, wherever the result depends on physical hardware |
| Final evaluated classification | `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE` |
| Stop boundary | No protected contract changes, live defaults, HRTF dependency, codec, HDMI, network, wireless, GUI, AI, calibration, or physical claim |

No implementation is authorized by this queue entry alone. A Phase 3A branch
must first restate its dependency matrix and receive scope approval.

## Authorized Parallel Milestone: Phase 3B

**Phase 3B -- Deterministic Horizontal Source Spread and Irregular Layout
Support** is owner-authorized under Section 16.1 after its governance amendment
merges.

- `execution_state`: `CLOSED`
- `evaluation_classification`: `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`
- planned branch: `phase-3b-horizontal-spread`
- Phase 3A dependency: mandatory and unchanged
- Phase 2 required for implementation or software evaluation: no
- Phase 2 required for final physical acceptance: yes

The milestone adds a bounded concrete spread API to the existing VBAP crate,
irregular horizontal-layout evidence, deterministic sweeps, allocation tests,
and host benchmarks. It excludes elevation, 3D/triplet VBAP, HRTF, Ambisonics,
room behavior, calibration, hardware work, protected-contract changes, default
changes, and later milestones. The complete dependency matrix and stop boundary
are in `docs/planning/phase-3b-scope.md`.

## Authorized Test Milestone: Simulation Assurance Campaign 1

**Simulation Assurance Campaign 1 -- Massive Deterministic Property and Stress
Testing** is authorized as a verification-only successor after its governance
amendment merges.

- `execution_state`: `CLOSED`
- `evaluation_classification`: `ACCEPTED`
- implementation branch: `test/simulation-assurance-campaign-1`
- product features: none
- Phase 2 required for implementation or campaign acceptance: no
- effect on Phase 2 physical gates: none; all remain open
- expected classification after every campaign criterion passes: `ACCEPTED`

The campaign reuses the accepted simulator and Phase 3A/3B renderers for
bounded deterministic property, metamorphic, stress, replay, and accelerated
soak testing. It cannot create another simulator, change protected contracts,
start Phase 3C, or produce physical evidence. Its complete scope and execution
levels are in `docs/planning/simulation-assurance-campaign-1.md`.

Formal evaluation passed on 2026-07-16. This test-only acceptance changes no
physical gate and authorizes no later product milestone. The evidence record is
`docs/acceptance/simulation-assurance-campaign-1.md`.

## Authorized Software Milestone: Diagnostics & Telemetry Framework 1

**Diagnostics & Telemetry Framework 1** is owner-authorized as a software-only
control-plane milestone.

- `execution_state`: `CLOSED`
- `evaluation_classification`: `ACCEPTED`
- implementation branch: `feature/diagnostics-framework-1`
- Phase 2 required for implementation or acceptance: no
- hardware criteria: none
- product audio behavior changes: none

The milestone adds deterministic structured events, bounded control-thread
logging, atomic callback metrics, snapshots, reports, tests, benchmarks, and
documentation. It cannot change renderer/DSP behavior, protected contracts,
accepted tags, or start Phase 3C. See
`docs/planning/diagnostics-telemetry-framework-1.md`.

Formal evaluation passed on 2026-07-17. Every software-only criterion and both
required remote workflows passed; the milestone has no physical acceptance
gate. The evidence record is
`docs/acceptance/diagnostics-framework-1.md`. This acceptance does not alter
Phase 2 or authorize Phase 3C.

## Authorized Software Milestone: Configuration & Preset System 1

**Configuration & Preset System 1** is owner-authorized as a software-only
control-plane milestone after its governance amendment merges.

- `execution_state`: `CLOSED`
- `evaluation_classification`: `ACCEPTED`
- implementation branch: `feature/configuration-preset-system-1`
- implementation PR: `#17`, merged as
  `384a603b917bceb104ee3ef2f90d4bb5ee18094b`
- accepted tag: `configuration-preset-system-1-accepted`
- Phase 2 required for implementation or acceptance: no
- hardware criteria: none
- product audio behavior changes: none
- governance merge: `f8d2fb9eaa45fce2e9a2d9f05fa363ced5a4a531`

The milestone adds immutable versioned configuration, bounded validation,
canonical serialization, deterministic presets, migrations, redaction,
fixtures, tests, benchmarks, and documentation in an independent crate. It
cannot probe or accept physical devices, change renderer/DSP behavior or
protected contracts, wire protected callbacks, start Phase 3C, or add runtime
hot reload or UI. The complete dependency matrix and stop boundary are in
`docs/planning/configuration-preset-system-1.md`.

Formal software-only evaluation passed on 2026-07-17. Local validation and
required remote workflows passed with no protected-contract or physical gate.
The evidence record is
`docs/acceptance/configuration-preset-system-1.md`. The milestone is closed and
accepted; its merge does not alter Phase 2 or authorize Phase 3C.

## Accepted Software Milestone: Runtime Assembly Contracts 1

**Runtime Assembly Contracts 1** is an authorized hardware-independent
control-plane milestone. Checkpoints A, B, C, D, and E are complete, and the
final architectural evaluation accepted the milestone on 2026-07-18.

- `authorization_state`: `AUTHORIZED`
- `milestone_status`: `COMPLETE`
- `execution_state`: `CLOSED`
- `evaluation_classification`: `ACCEPTED`
- `completed_checkpoint`: `E`
- Checkpoint D execution state: `COMPLETE`
- Checkpoint E execution state: `COMPLETE`
- evidence branch: `docs/runtime-assembly-contracts-1-checkpoint-d`
- Checkpoint D merge: PR `#26`,
  `93f36464cac429bf7e25b257e894aab64a72e2d4`
- implementation crate: `aurora-runtime-assembly`
- Phase 2 required for implementation or acceptance: no
- hardware criteria: none
- product audio behavior changes: none

Checkpoint B deterministically derives the immutable runtime plan from
`&ValidatedConfiguration`; Checkpoint C deterministically derives immutable
descriptive setup intent from `PreparedRuntimePlan`. Neither constructs or
executes a runtime subsystem. The milestone may not probe or open devices,
construct a running engine, touch callbacks, modify protected contracts, start
Phase 3C, or make physical claims. The dependency matrix, checkpoints, tests,
and stop boundary are in `docs/planning/runtime-assembly-contracts-1.md` and
ADRs 0014 and 0015. Checkpoint D was marked `IN_PROGRESS` while PR `#26` was
under review and was reconciled to `COMPLETE` after merge. Checkpoint E's final
review found no blocking issue. The acceptance record is
`docs/acceptance/runtime-assembly-contracts-1.md`. This is software-only
control-plane acceptance: no runtime executed, no hardware was validated, no
physical claim was made, and no later milestone is authorized.

## Active Software Milestone: Runtime Plan Inspection 1

**Runtime Plan Inspection 1** is the single active software-only control-plane
milestone. Its governance amendment and ADR 0016 merged normally through PR
`#29` at `02deb93374b162d11b18b4010452254f3ecd1c18`.

- `authorization_state`: `AUTHORIZED`
- `execution_state`: `IN_PROGRESS`
- `evaluation_classification`: none
- governance branch: `governance/runtime-plan-inspection-1`
- Checkpoint A merge: PR `#30`,
  `b847344c8a64e2b605aead3b6cef8979f39b9916`
- Checkpoint B merge: PR `#31`,
  `6c28ab826a40e84d3fcdbc07999a0414dce1b1ca`
- implementation branch: `feature/runtime-plan-inspection-1-checkpoint-c`
- active checkpoint: C
- completed checkpoints: A and B
- Checkpoint D: `NOT_STARTED`
- predecessor: Runtime Assembly Contracts 1, `ACCEPTED`
- Phase 2 required for implementation or acceptance: no
- hardware criteria: none

The milestone may add a separate read-only inspection crate that projects the
accepted prepared runtime and setup plans into its own versioned, bounded,
redacted-by-default schema with deterministic JSON and human formatting. It may
not modify or serialize the accepted plan types, construct or execute runtime
resources, probe hosts, add CLI or diagnostics producer integration, or change
any protected contract.

This roadmap insertion comes after the accepted runtime-assembly contracts and
before any runtime materialization, control API, or Phase 3C proposal. The
complete objective, dependency matrix, exclusions, checkpoints, criteria,
validation strategy, risks, and stop boundary are in
`docs/planning/runtime-plan-inspection-1.md` and ADR 0016. This governance
change contains no implementation. Checkpoint A established the isolated crate,
and Checkpoint B added bounded deterministic projection and redaction. Both are
merged. Checkpoint C may add only deterministic bounded formatting and
conformance evidence; final evaluation remains Checkpoint D.

## Checkpoint 1: Architecture and Gain Simulation

- Create architecture, roadmap, licensing, and agent guardrail documentation.
- Scaffold the Rust workspace.
- Implement the shared core data model.
- Define the renderer trait.
- Implement one deterministic inverse-distance renderer.
- Add focused unit tests.
- Add a CLI command that prints calculated speaker gains for a moving source.

## Checkpoint 2: Offline WAV Rendering

- Add speaker layout JSON fixtures.
- Add scene trajectory JSON fixtures.
- Add mono WAV input and multichannel WAV output.
- Render a mono source block by block.
- Print peak levels, clipping warnings, and latency metadata.

## Checkpoint 3: Basic DSP

- Implement gain, mute, delay, polarity, high-pass, low-pass, parametric EQ, and simple crossover routing.
- Add channel-independent DSP tests.
- Document future CamillaDSP adapter options in more detail.

## Checkpoint 4: Measurement Skeleton

- Add test-signal generation.
- Add synthetic impulse-response fixtures.
- Implement synthetic time-of-arrival estimation.

## Checkpoint 5: Visualizer

- Build a minimal local UI showing the room, listener, speakers, draggable audio object, gain meters, current distance, delay, and renderer mode.
- Ensure visualizer gains match the offline renderer.
