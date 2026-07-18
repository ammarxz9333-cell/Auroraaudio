# Runtime Assembly Contracts 1: Final Architectural Evaluation

## Record

- Milestone: Runtime Assembly Contracts 1
- Milestone class: software-only control plane
- Acceptance date: `2026-07-18`
- Milestone status: `COMPLETE`
- Execution state: `CLOSED`
- Evaluation classification: `ACCEPTED`
- Final decision: `ACCEPTED`
- Completed checkpoints: A, B, C, D, E
- Evaluated repository state:
  `b7bcf01168b7d578b6b031aaaa70df9f64548d40`
- Checkpoint D evidence:
  [runtime-assembly-contracts-1-checkpoint-d.md](../evidence/runtime-assembly-contracts-1-checkpoint-d.md)

This decision accepts only the immutable Aurora-owned control-plane contracts
and deterministic derivation defined by this milestone. It does not accept or
claim a running product or runtime subsystem.

## Scope Evaluated

The review covered `PreparedRuntimePlan`, `PreparedSetupPlan`, their component
contracts, runtime-plan derivation from `ValidatedConfiguration`, setup-plan
derivation from `PreparedRuntimePlan`, canonical stage/dependency semantics,
typed errors, public documentation, tests, dependencies, merged history, and CI
evidence.

No implementation change belongs to Checkpoint E. The evaluated implementation
is the source already merged through Checkpoint C, with Checkpoint D supplying
contract tests, documentation, and evidence.

## Governance Basis

- [ADR 0014](../adr/0014-runtime-assembly-boundary.md): immutable runtime-plan
  ownership, deterministic validated derivation, requested intent, capacity,
  error, and dependency boundaries;
- [ADR 0015](../adr/0015-deterministic-runtime-setup-planning.md): immutable
  setup-plan ownership, six canonical descriptive stages, bounded acyclic
  dependencies, non-execution, and non-readiness semantics;
- [milestone specification](../planning/runtime-assembly-contracts-1.md);
- [Checkpoint D evidence](../evidence/runtime-assembly-contracts-1-checkpoint-d.md);
- repository architecture, roadmap, and master-reference invariants.

The historical ADR decisions are unchanged by this review.

## Merged History

| Scope | PR | Reviewed head | Merge commit |
| --- | ---: | --- | --- |
| Initial milestone governance | #20 | `fdee7c2f3148402747bc6b7da8047d35951d3222` | `e2f3b315889f49340398d8503aac7123567f602b` |
| Checkpoint A | #21 | `3f7110d7a0c23bbf945b1e2331ce9ce3b578fe19` | `dbf4aedf2c49108d4e20273249ad4c58d67d780a` |
| Checkpoint B | #23 | `687f64c526a9671316a6bdc713301d63c45cb8a0` | `2a45322e05476801d77705cd26f5be5479eeac35` |
| ADR 0015 governance | #24 | `51f803f2c0079e640c0af1a62e357686e371ebbe` | `766bf50f482ba12d2b701581d19453f866fba612` |
| Checkpoint C | #25 | `fc4d3fccf5adbee9638b7d214cd8baa6aacc1ecf` | `dbf06a8d2f613086cccf3e6b43e8c7f714e00671` |
| Checkpoint D | #26 | `b2c08a4767c26de1bcd4b911a0d941a24317a1aa` | `93f36464cac429bf7e25b257e894aab64a72e2d4` |
| Checkpoint D status reconciliation | #27 | `b631ee2a765616d8761cd4ea98f84647c88ee283` | `b7bcf01168b7d578b6b031aaaa70df9f64548d40` |

Every listed PR is merged. No history or accepted tag was rewritten.

## Implementation Summary

The standalone `aurora-runtime-assembly` crate owns immutable passive values.
`prepare_runtime_plan` derives normalized runtime preparation intent from an
already validated configuration. `prepare_setup_plan` derives a descriptive
setup plan from that prepared runtime plan.

The setup model has six canonical stages and nine fixed required dependencies.
`SetupPlanComplete` means only that the immutable description is complete. It
does not mean setup executed, a runtime exists, a host accepted a format, a
device was resolved, or any readiness was observed.

## Validation Matrix

| Criterion | Result | Evidence |
| --- | --- | --- |
| Immutable Aurora-owned contracts | PASS | Private fields, owned values, read-only accessors, no third-party public types |
| Runtime-plan determinism | PASS | Unit and public contract tests for semantically equal validated input |
| Setup-plan determinism | PASS | Repeated typed equality tests from equal prepared plans |
| Canonical ordering | PASS | Normalized routing/layout tests and fixed six-stage order |
| Complete bounded dependency graph | PASS | Fixed nine-edge graph; completeness and acyclicity tests |
| Requested versus negotiated semantics | PASS | Types, rustdoc, and tests preserve requested format only |
| Unresolved device semantics | PASS | Selectors and backend families remain passive requested intent |
| Renderer intent only | PASS | Descriptor values only; no renderer API or implementation dependency |
| DSP intent only | PASS | `None` or schema-deferred state; no graph or processor construction |
| Backend intent only | PASS | Requested family only; no selection, handle, stream, or construction |
| Typed defensive errors | PASS | Structured preparation/setup errors and invariant tests |
| No external state dependency | PASS | No host, filesystem, environment, clock, random, process, or hardware capability import |
| No runtime execution | PASS | No engine, callback, stream, thread, process, or executable behavior |
| Public documentation | PASS | Warning-free rustdoc documents ownership and semantic boundaries |
| Dependency boundary | PASS | Direct dependencies exactly `aurora-config` and `aurora-core` |
| Prohibited scope | PASS | Source, dependency, and complete milestone-diff audit |
| Local evidence | PASS | 235 tests passed, 5 hardware tests ignored; format, Clippy, rustdoc, Rust 1.78, Actionlint, links, and scans passed |
| Remote Checkpoint D CI | PASS | CI run 83 and Simulation Assurance PR Smoke run 28 succeeded on reviewed head `b2c08a4...` |
| Remote status-closeout CI | PASS | CI run 85 and Simulation Assurance PR Smoke run 29 succeeded on reviewed head `b631ee2...` |

Remote CI jobs explicitly passed on Linux stable, Windows stable, and MSRV
1.78. The smoke runs are deterministic software evidence, not hardware or
physical evidence.

## Architectural Boundary Assessment

The crate remains a leaf control-plane owner. It depends directly only on
`aurora-config` and `aurora-core`; no existing crate depends back as part of
this milestone. There is no renderer, DSP, backend, real-time, CPAL, simulator,
diagnostics, or CLI dependency. Configuration stays inert, and no runtime
subsystem consumes these plans yet.

## Determinism Assessment

Derivation reads only explicit normalized inputs. Production source imports no
filesystem, environment, host API, wall clock, random generator, process API,
or global mutable state. Vectors retain normalized canonical order, setup stages
and edges use fixed arrays, and equal semantic inputs produce equal typed values
or typed errors.

## Ownership Assessment

All public contracts are Aurora-owned Rust values with private fields and no
third-party types. Cloning performs bounded setup-thread ownership allocation;
the plans contain no handle, closure, callback, trait object, stream, process,
thread, renderer, DSP processor, backend, or engine.

## Dependency Assessment

`cargo tree -p aurora-runtime-assembly --depth 1` reports exactly:

```text
aurora-runtime-assembly
|-- aurora-config
`-- aurora-core
```

The milestone added no external dependency. The workspace and lockfile changes
only register the authorized new crate and its two existing path dependencies.

## Prohibited-Scope Assessment

No renderer or DSP construction, factory, backend selection or construction,
engine wiring, CPAL, device discovery, format negotiation, stream, callback,
real-time execution, simulator integration, diagnostics producer, CLI,
serialization, hashing, fingerprint, networking, calibration, maximum-object
synthesis, Phase 2 implementation, Phase 3C work, physical measurement, or
latency claim was introduced.

## Known Limitations

- Plans are passive and have no authorized runtime consumer.
- Device selectors are unresolved and formats are requested, not negotiated.
- Maximum object count and implementation-specific capacities remain deferred.
- The current schema does not describe a complete DSP execution graph.
- The contracts do not prove runtime readiness, host support, hardware
  behavior, physical routing, audible behavior, stability, or latency.

## Deferred Work

Any renderer/DSP/backend construction, plan consumer, engine integration,
device resolution, format negotiation, CLI inspection, serialization,
fingerprinting, or schema extension requires separately approved governance.
This acceptance does not authorize any such work.

## Residual Risks

- A future consumer could misinterpret requested intent as negotiated state;
  the current type names, rustdoc, and tests reduce but cannot eliminate that
  integration risk.
- Future schema growth may require a reviewed contract version change and new
  invariant coverage.
- Runtime behavior remains wholly unvalidated because no runtime integration
  belongs to this milestone.

## Final Decision

`ACCEPTED`

No blocking finding remains. Runtime Assembly Contracts 1 is complete as a
software-only milestone. No runtime executed. No hardware validation occurred.
No physical claim is made. No renderer, DSP, backend, stream, callback, or
engine exists or is constructed by this milestone. The decision does not
authorize Phase 2 implementation, Phase 3C, or any later milestone.
