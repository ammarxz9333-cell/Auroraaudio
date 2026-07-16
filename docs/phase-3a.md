# Phase 3A -- Deterministic Offline Spatial Rendering Improvements

## Current State

- `execution_state`: `IN_PROGRESS`
- `evaluation_classification`: none
- branch: `phase-3a`
- branch base: `main-v2` at
  `bc2cf637bb8dc5d526a28271e1070efcff1e09d4`

Authorized implementation is underway. No acceptance or validation conclusion
is implied.

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
