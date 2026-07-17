# Aurora Validation Lab

## Status

- `authorization_state`: `PROPOSED`
- `execution_state`: `NOT_STARTED`
- `evaluation_classification`: `NOT_EVALUATED`
- scope: documentation and architecture only

## Purpose

The Aurora Validation Lab is the project-wide automated verification layer for signal processing, rendering, synchronization, transport, endpoint behavior, acoustics, fault recovery, and long-duration stability.

It shall convert every relevant Aurora revision into repeatable engineering evidence before hardware purchase or physical deployment. Simulated evidence must remain explicitly separated from physical measurements.

## Required pipeline

Every campaign shall:

1. select or generate a versioned scenario matrix;
2. build the exact repository revision under test;
3. execute deterministic functional, stress, randomized, and soak suites;
4. collect metrics with evidence class, fidelity tier, units, thresholds, and uncertainty;
5. classify each scenario as `PASS`, `PASS_WITH_WARNINGS`, `FAIL`, `INCONCLUSIVE`, or `MODEL_NOT_APPLICABLE`;
6. compare results with an accepted baseline;
7. emit machine-readable and human-readable reports;
8. preserve replay commands, seeds, configurations, dataset hashes, and environment metadata;
9. generate bounded engineering recommendations without modifying production code;
10. fail closed when required evidence is absent or internally inconsistent.

## Coverage domains

The laboratory shall cover, where applicable:

- DSP correctness, channel order, gain, delay, polarity, filters, crossovers, clipping, and silence preservation;
- renderer continuity, spatial trajectories, layout robustness, and binaural preview proxies;
- ASRC stability, clock drift, synchronization error, jitter, buffer occupancy, underflow, and overflow;
- CPU budget, callback deadlines, memory growth, allocation behavior, and queue saturation;
- packet latency, jitter, loss, duplication, reordering, bursts, endpoint disappearance, and recovery;
- room geometry, propagation, reflections, reverberation approximations, directivity, and speaker profiles;
- configuration errors, corrupted model data, interrupted report generation, and recovery semantics;
- accelerated soak behavior and regression detection.

## Campaign classes

- **presubmit**: short deterministic gates for pull requests;
- **post-merge**: broader deterministic regression coverage;
- **scheduled**: large randomized and long-duration campaigns;
- **release candidate**: frozen scenario matrix and accepted thresholds;
- **physical-correlation**: later comparison between simulation and accepted hardware measurements.

## Non-negotiable boundaries

- The laboratory does not modify production code automatically.
- It does not alter accepted baselines or thresholds without explicit review.
- It does not label modeled output as measured hardware performance.
- It does not replace Phase 2 physical validation.
- It retains failed, skipped, unsupported, and inconclusive cases.

## Relationship to other documents

- Campaign and acoustic scope: `docs/planning/physical-acoustic-simulator-1.md`
- Fleet lifecycle: `docs/architecture/fleet-management.md`
- Endpoint health telemetry: `docs/architecture/health-monitoring.md`
- Safe deployment: `docs/architecture/ota-update-system.md`
- Automated configuration search: `docs/architecture/design-explorer.md`
- Simulation-to-hardware calibration: `docs/architecture/physical-correlation.md`
