# Simulation Assurance Campaign 1 Scope

## Record

- Milestone: Simulation Assurance Campaign 1 -- Massive Deterministic Property
  and Stress Testing
- `execution_state`: `IN_PROGRESS`
- `evaluation_classification`: none
- Governance branch: `governance/define-simulation-assurance-campaign-1`
- Implementation branch: `test/simulation-assurance-campaign-1`
- Implementation base: `main-v2` at
  `1b28d514882d407446dac3a366ecd35841a2a95a`
- Truth sources: `unit_test`, `deterministic_simulation`,
  `host_api_observation`

## Purpose

Increase software confidence in the accepted Simulation Sprint 1 and the
conditionally accepted Phase 3A and Phase 3B implementations through large,
bounded, reproducible scenario sets. The campaign discovers software defects
and preserves compact regression evidence. It never substitutes for Phase 2
physical validation.

## Included Scope

- property-based and metamorphic testing;
- deterministic bounded scenario generation and sharding;
- seed replay, stable scenario identifiers, bounded failure windows, and
  shrinking where practical;
- deterministic regression fixtures for confirmed defects;
- accelerated long-duration execution through the existing simulator;
- Phase 3A point-source and Phase 3B spread/irregular-layout invariants;
- bounded PR smoke, nightly, manual deep, and manual soak workflows.

Every generated scenario records a deterministic seed, stable scenario ID,
bounded configuration, `truth_source=deterministic_simulation`, reproducible
command, bounded report, and deterministic failure category. Failure artifacts
must remain compact; large campaign output is never committed.

## Required Levels

| Level | Required scale | Execution |
| --- | ---: | --- |
| PR smoke | 500--1,000 scenarios | Bounded pull-request workflow |
| Nightly | 10,000 scenarios | Deterministically sharded workflow |
| Manual deep | Configurable to 100,000 scenarios | Explicit workflow dispatch |
| Manual soak | Representative accelerated 24h, 7d, and 30d scenarios | Explicit workflow dispatch |

Initial evaluation must execute 1,000 smoke scenarios, 10,000 standard
scenarios, the same fixed 1,000 seeds three times, representative accelerated
24-hour scenarios, and all accepted legacy simulation fixtures.

## Required Properties

- no panic, deadlock, unbounded loop, or unbounded memory growth;
- finite output and bounded reports;
- zero warmed-up callback and renderer allocations;
- valid state transitions and allowed recovery terminal states;
- deterministic target-qualified checksums;
- explicit fallback and deterministic fault observability;
- no channel leakage;
- normalized renderer energy;
- spread zero remains compatible with Phase 3A;
- speaker-layout permutations produce equivalent permuted output;
- no physical terminology in generated evidence.

Required scenario dimensions cover sample rates, callback sizes, drift, jitter,
buffer fill, fault combinations, start/stop/recovery sequences, stereo/5.1/7.1,
canonical and irregular layouts, point and spread rendering, angular
wraparound, duplicate and near-duplicate geometry, and invalid or extreme
finite values.

## Dependency Matrix

| Gate | Decision |
| --- | --- |
| Software-only criteria | Generator, replay, shrinking, reports, invariants, workflows, tests, and documentation pass |
| Deterministic simulation criteria | Required levels and initial executions pass reproducibly with no unresolved defect |
| Host observation criteria | Runtime and memory remain bounded; results are not latency measurements |
| Hardware criteria for this campaign | None |
| Phase 2 required for implementation | No |
| Phase 2 required for campaign acceptance | No |
| Effect on Phase 2 | None; every physical gate remains open and required |
| Expected final classification | `ACCEPTED` after all campaign criteria pass |

## Protected Contracts

The campaign may not change Aurora-owned public traits, callback or buffer
ownership, allocation guarantees, bounded-memory guarantees, fault or state
semantics, device selection, truth-source terminology, accepted records, or
accepted tags. It must reuse `aurora-realtime-audio-sim`; creating a second
simulator is prohibited.

## Exclusions And Stop Boundary

No renderer feature, Phase 3C work, elevation, HRTF, Ambisonics, hardware
abstraction, physical acceptance, codec, HDMI, networking, wireless audio,
GUI, AI, or calibration is included. No campaign result is physical evidence.

Stop after the bounded implementation, required campaign executions, full
validation, resource review, formal evaluation, and creation and verification
of an unmerged implementation pull request against `main-v2`.
