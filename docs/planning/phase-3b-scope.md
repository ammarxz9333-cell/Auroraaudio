# Phase 3B -- Deterministic Horizontal Source Spread and Irregular Layout Support

## Milestone Record

- `execution_state`: `IN_PROGRESS`
- `evaluation_classification`: `none`
- planned branch: `phase-3b-horizontal-spread`
- required base: `main-v2` after this governance amendment merges
- predecessor: Phase 3A, `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`

Branch creation alone did not start implementation. The first authorized Phase
3B implementation commit moved this record to `IN_PROGRESS`.

## Purpose And Included Scope

Phase 3B extends `crates/aurora-renderer-vbap` with deterministic software-only
support for horizontal source spread and irregular horizontal speaker layouts.
It must reuse the accepted Phase 3A implementation.

The normalized spread domain is finite and inclusive: `0.0..=1.0`. Spread zero
must use the accepted point-source path and match Phase 3A within documented
floating-point tolerance. Intermediate spread adds nearby eligible speakers
using one documented deterministic law. Spread one is the widest supported
horizontal distribution and remains power-normalized and finite.

Supported layouts include uneven spacing, asymmetry, missing nominal positions,
arbitrary valid ordering, canonical stereo/5.1/7.1, sparse layouts, and bounded
dense layouts with more than eight horizontal speakers. Applying a permutation
to a non-degenerate input layout must apply only the corresponding permutation
to output channels. Duplicate-angle behavior and all geometric tie-breaking
must be explicit and deterministic.

## Explicit Exclusions

- elevation, 3D or triplet VBAP;
- HRTF, binaural rendering, Ambisonics, or HOA;
- room simulation, reflections, reverberation, or acoustic correction;
- new distance attenuation, Doppler, head tracking, or listener orientation;
- hardware integration, calibration, speaker identification, or layout
  measurement;
- Phase 2 physical validation, Phase 3C, or any later milestone.

The milestone may not modify the Aurora-owned `Renderer` trait, Basic renderer
behavior, CLI/live defaults, callback ownership, allocation and bounded-memory
guarantees, state or fault semantics, device selection, or truth-source
semantics. A required change to one of these contracts stops the milestone and
requires a separately approved ADR or amendment.

## Dependency Matrix

| Evidence category | Required gates |
| --- | --- |
| Software-only | Point-source compatibility, deterministic spread, irregular-layout correctness, power normalization, finite output, stable tie-breaking, layout-order independence, caller-owned bounded memory, zero warmed-up allocations, public API documentation, no protected-contract change, deterministic fixtures, unit and integration tests |
| Deterministic simulation | Repeated fixture reproducibility, canonical and irregular scenarios, `-pi/+pi` source sweeps, complete spread sweeps, deterministic checksums where suitable |
| Host observation | Representative offline benchmarks and allocation audit, reported only as `host_api_observation` |
| Physical hardware | Audible spread on identifiable speakers, physical 5.1/7.1 routing, endpoint behavior, real-path level consistency, and hardware stability |
| Phase 2 dependency | Not required for implementation or software evaluation; required before final physical acceptance |
| Phase 3A dependency | Mandatory; reuse its crate, preserve spread-zero point output, validation, normalization, and finite fallback |

If all non-hardware gates pass while hardware remains unavailable, the expected
classification is `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`. It may not be
`ACCEPTED` while any physical gate above remains open.

## Truth Sources

Phase 3B permits `unit_test`, `deterministic_simulation`, and
`host_api_observation`. `virtual_audio_backend` is not required unless an
existing virtual-backend path becomes directly relevant; no simulator may be
duplicated. No result is `physical_measurement` without physical capture or
independently identifiable real output paths. Benchmark timing is not latency.

## Acceptance Criteria

1. Spread zero is compatible with the accepted Phase 3A point-source path for
   stereo, 5.1, 7.1, and irregular non-degenerate layouts.
2. The documented spread law widens deterministic speaker participation and
   preserves unit power across `0.0..=1.0`.
3. Canonical, uneven, asymmetric, sparse, dense, reversed, duplicate-angle,
   near-duplicate, wraparound, exact-hit, midpoint, single-speaker, and
   two-speaker cases have explicit tested behavior.
4. Invalid spread and non-finite configuration use structured setup/control
   errors. Invalid callback-time state uses the documented finite fallback.
5. Extreme finite values do not produce NaN or infinity in gains, metadata, or
   fixture-rendered samples.
6. Output and scratch storage remain caller-owned, bounded by configuration,
   and allocation-free for at least 1,000 warmed-up renders.
7. Public APIs and the mathematical law are documented with tolerances,
   participation, tie-breaking, and capacity behavior.
8. Focused point/spread benchmarks cover 5.1, 7.1, maximum irregular spread,
   and a dense horizontal layout at 48 kHz and 256 frames. Results use
   `host_api_observation` and disclose environment limits and point-path delta.
9. Formatting, Clippy, all-feature tests, strict Rustdoc, Actionlint, workspace
   benchmarks, and three repeated deterministic fixture runs pass.
10. Review confirms no protected contract, default, Phase 2, Phase 3A accepted
    record, simulator, or later-milestone change.

## Execution And Evaluation

- Before implementation: `NOT_STARTED`, classification `none`.
- First authorized implementation commit: `IN_PROGRESS`, classification `none`.
- Authorized implementation stop boundary: `READY_FOR_EVALUATION` with a
  mandatory provisional classification.
- After criterion-by-criterion evaluation: `CLOSED` with the evidence-supported
  terminal classification.

The final evaluation must distinguish software, deterministic, host, and
physical criteria. It must create `docs/acceptance/phase-3b.md`, identify exact
commits and commands, and state that no physical measurement occurred.

## Stop Boundary

Stop after the algorithm document, bounded implementation, required tests and
fixtures, allocation audit, focused and workspace benchmarks, full validation,
independent diff review, evaluation record, and creation of an unmerged Phase
3B pull request.

Stop immediately without architecture expansion if the work requires a
protected-contract change, cannot remain bounded and allocation-free, cannot be
deterministic, cannot preserve Phase 3A point-source behavior, or requires
physical evidence for a software claim.
