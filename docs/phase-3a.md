# Phase 3A -- Deterministic Offline Spatial Rendering Improvements

## Current State

- `execution_state`: `CLOSED`
- `evaluation_classification`: `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`
- branch: `phase-3a`
- branch base: `main-v2` at
  `bc2cf637bb8dc5d526a28271e1070efcff1e09d4`

Formal software evaluation passed on 2026-07-16. The current execution cycle is
closed, but the milestone remains conditionally open because the physical gates
below have not run. This is not full acceptance and creates no accepted tag.

## Authorized Scope

Phase 3A may add a deterministic offline VBAP renderer that implements the
existing Aurora-owned `Renderer` trait. It may add focused fixtures, unit and
offline integration tests, and release benchmarks. The existing basic renderer
and all live defaults remain unchanged.

## Hardware Dependency Matrix

| Gate | Required evidence |
| --- | --- |
| Software-only | Deterministic VBAP geometry, finite output, canonical routing, caller-owned bounded storage, offline fixtures, and focused benchmarks |
| Deterministic simulation | Repeatable routing through the existing virtual backend only where relevant; no new simulator |
| Hardware-dependent | Physical 5.1/7.1 routing and channel identity, live endpoint behavior, physical stability and clock behavior, and audible or speaker-dependent conclusions |
| Phase 2 required for implementation | No |
| Phase 2 required for final acceptance | Yes for every hardware-dependent gate or claim |
| Conditional acceptance required | Yes while any hardware-dependent acceptance gate remains open |

Software evidence uses `unit_test`. Any later simulator evidence must use
`deterministic_simulation` or `virtual_audio_backend` as applicable. None of
these truth sources is physical evidence.

## Protected Contracts

Phase 3A must preserve the public `Renderer` trait, caller-owned buffer and
scratch ownership, zero steady-state allocation, bounded memory, canonical
channel order, WAV masks, callback behavior, fault and state semantics, device
selection, and report truth-source terminology.

## Explicit Exclusions

This milestone does not authorize spread or elevation behavior, irregular-layout
product claims, Ambisonics, HRTF, binaural rendering, CLI or live-default
selection, codecs, HDMI, networking, wireless audio, GUI, AI, calibration, a
duplicate simulator, or physical measurement claims.

## Stop Boundary

Stop after the independent VBAP implementation, focused evidence, benchmark,
documentation, final branch review, and an unmerged Phase 3A pull request. Any
public trait change or protected-contract change requires separate approval.

## Implemented Scope

- independent `aurora-renderer-vbap` crate implementing the existing `Renderer`
  trait;
- deterministic horizontal two-speaker VBAP pair selection and power
  normalization;
- deterministic nearest-direction fallback for geometry outside a valid pair;
- caller-owned output and scratch storage with preallocated smoothing history;
- canonical 5.1 and 7.1 fixture routing tests and scene-order independence;
- full-circle deterministic, finite, continuous transition validation;
- focused release benchmarks against the existing inverse-distance renderer.

The existing basic renderer, CLI, real-time defaults, simulator, channel order,
and WAV metadata are unchanged.

## Verification Evidence

Correctness and allocation evidence uses truth source `unit_test`.

- 15 renderer unit tests cover stereo geometry, exact direction, midpoint and
  wraparound behavior, duplicate-angle tie-breaking, listener coincidence,
  single-speaker and invalid-buffer cases, finite-input validation, extreme
  finite values, silence, determinism, structured no-speaker failure, unchanged
  capacities, and zero allocations over 1,000 warmed-up calls;
- 3 offline fixture tests cover canonical indices, finite 5.1/7.1 output,
  scene-order independence, deterministic full-circle output, and smooth gain
  transitions;
- the existing virtual backend does not select this optional offline renderer,
  so Phase 3A adds no simulator path and claims no simulation result.

## Performance

Release Criterion results on the development host at 48 kHz, 256 frames, and
one object use truth source `host_api_observation` because Criterion reads the
host timer:

| Renderer | Layout | Median estimate | 95% estimate interval | Block budget |
| --- | --- | --- | --- | --- |
| VBAP | 5.1 | 367.28 ns | 366.33--368.30 ns | 0.0069% |
| Basic inverse distance | 5.1 | 65.83 ns | 65.70--65.96 ns | 0.0012% |
| VBAP | 7.1 | 570.11 ns | 566.98--573.39 ns | 0.0107% |
| Basic inverse distance | 7.1 | 82.90 ns | 82.73--83.09 ns | 0.0016% |

VBAP was approximately 5.58 times the basic-renderer cost for 5.1 and 6.88 times
for 7.1. Finite-edge hardening increased the VBAP benchmark by approximately
12% relative to the pre-evaluation baseline while remaining below 0.011% of the
block budget. These are host benchmark observations, not physical latency
measurements. Criterion did not report p95 or maximum latency for this run.

## Formal Evaluation

- evaluation date: `2026-07-16`;
- submitted implementation commit:
  `1d64cbd1030dda0ff464113110807f48031b6823`;
- evaluated implementation and defect-fix commit:
  `501fb9eea3c19284d71bd9d9bfa21e664f6e5a55`;
- pull request: `#10`;
- result: all software, review, documentation, allocation, and benchmark gates
  passed; 130 tests passed and 5 hardware-only tests remained ignored;
- final classification: `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`.

The detailed criterion record and commands are in
`docs/acceptance/phase-3a.md`. No physical evidence was used.

## Pending Hardware Gates

- physical 5.1/7.1 routing and channel identity;
- live endpoint behavior with a separately approved renderer-selection path;
- physical driver stability, clock behavior, and long-run performance;
- audible and speaker-dependent conclusions.

Phase 2 remains open and incomplete. No current test or benchmark satisfies
these gates.
