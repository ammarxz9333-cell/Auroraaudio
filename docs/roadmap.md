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

The first technically safe candidate is **Phase 3A -- Deterministic Offline
Spatial Rendering Improvements**. Authorized implementation is underway.

- `execution_state`: `IN_PROGRESS`
- `evaluation_classification`: none
- branch: `phase-3a`
- branch base: `main-v2` at
  `bc2cf637bb8dc5d526a28271e1070efcff1e09d4`

No acceptance or validation conclusion is implied while implementation remains
in progress.

| Gate | Current decision |
| --- | --- |
| Software-only criteria | VBAP geometry and bounded deterministic offline rendering using existing contracts |
| Simulation criteria | Existing simulator may verify routing; no duplicate simulator may be added |
| Hardware criteria | Physical 5.1/7.1 routing, endpoint behavior, stability, and audible conclusions remain pending |
| Phase 2 required to implement | No, for the constrained offline scope |
| Phase 2 required for final acceptance | Yes, wherever the result depends on physical hardware |
| Classification after implementation completes | `IMPLEMENTATION_COMPLETE_VALIDATION_PENDING` |
| Stop boundary | No protected contract changes, live defaults, HRTF dependency, codec, HDMI, network, wireless, GUI, AI, calibration, or physical claim |

No implementation is authorized by this queue entry alone. A Phase 3A branch
must first restate its dependency matrix and receive scope approval.

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
