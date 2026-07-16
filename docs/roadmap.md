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

- `execution_state`: `NOT_STARTED`
- `evaluation_classification`: `none`
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
