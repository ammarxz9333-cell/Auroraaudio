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

Current record: `execution_state=NOT_STARTED`,
`evaluation_classification=none`, branch `phase-3a`, based on `main-v2` at
`fb494cb31463dd0f562d7fef4e4548e3cf16850d`. Creating the branch did not start
implementation. The first authorized Phase 3A implementation commit changes the
execution state to `IN_PROGRESS`.

| Dependency | Decision |
| --- | --- |
| Software-only criteria | Deterministic VBAP geometry, finite output, canonical routing, offline fixtures, focused benchmarks |
| Simulation criteria | Repeatable virtual-backend routing where the existing simulator is relevant; no new simulator |
| Hardware criteria | Physical multichannel routing, endpoint behavior, stability, and audible/speaker-dependent conclusions |
| Phase 2 required for implementation | No, for offline work using existing contracts |
| Phase 2 required for final acceptance | Yes, for hardware-relevant claims and gates |
| Classification after implementation completes | `IMPLEMENTATION_COMPLETE_VALIDATION_PENDING` |
| Possible reviewed classification | `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE` |

Stop before public trait changes, live default changes, channel-order changes,
HRTF dependencies, codecs, HDMI, networking, wireless audio, GUI, AI, room
calibration, or any physical claim.

## Git workflow

- branch each parallel milestone from `main-v2`;
- keep `phase-2-physical-hardware-validation` open independently;
- do not merge automatically;
- never move or replace an accepted tag;
- state conditional status in the pull request and milestone documentation.
