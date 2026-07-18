# Runtime Assembly Contracts 1: Checkpoint D Evidence

## Record

- Milestone: Runtime Assembly Contracts 1
- Authorization state: `AUTHORIZED`
- Execution state: `IN_PROGRESS`
- Evaluation classification: `NOT_EVALUATED`
- Completed checkpoints: A, B, C
- Active checkpoint: D
- Checkpoint E: `NOT_STARTED`
- Evidence date: `2026-07-18`
- Branch: `docs/runtime-assembly-contracts-1-checkpoint-d`

This is evidence for a later architectural review. It is not an acceptance
record and makes no runtime, host, hardware, latency, or physical claim.

## Checkpoint History

| Change | Pull request | Reviewed head | Merge commit |
| --- | ---: | --- | --- |
| Governance and ADR 0014 | #20 | `fdee7c2f3148402747bc6b7da8047d35951d3222` | `e2f3b315889f49340398d8503aac7123567f602b` |
| Checkpoint A contracts | #21 | `3f7110d7a0c23bbf945b1e2331ce9ce3b578fe19` | `dbf4aedf2c49108d4e20273249ad4c58d67d780a` |
| Checkpoint B derivation | #23 | `687f64c526a9671316a6bdc713301d63c45cb8a0` | `2a45322e05476801d77705cd26f5be5479eeac35` |
| ADR 0015 authorization | #24 | `51f803f2c0079e640c0af1a62e357686e371ebbe` | `766bf50f482ba12d2b701581d19453f866fba612` |
| Checkpoint C setup plan | #25 | `fc4d3fccf5adbee9638b7d214cd8baa6aacc1ecf` | `dbf06a8d2f613086cccf3e6b43e8c7f714e00671` |

ADRs [0014](../adr/0014-runtime-assembly-boundary.md) and
[0015](../adr/0015-deterministic-runtime-setup-planning.md) remain historical
decisions and are unchanged by Checkpoint D.

## Public Contract Evidence

The `aurora-runtime-assembly` public boundary contains:

- immutable runtime-plan contracts and `prepare_runtime_plan`;
- immutable setup-plan, stage, and dependency contracts and
  `prepare_setup_plan`;
- unresolved requested device intent and requested, unnegotiated format intent;
- passive renderer/topology, current-schema DSP, and requested backend intent;
- structured runtime-preparation and setup-planning errors and invariants.

The crate-level and item rustdoc state the ownership, deterministic-input,
requested-versus-negotiated, unresolved-device, non-execution, non-readiness,
and typed-error guarantees. `SetupPlanComplete` means only that all six
descriptive stages and nine required edges are present in canonical order.

## Contract Tests

The public-API integration suite at
`crates/aurora-runtime-assembly/tests/contracts.rs` verifies:

- semantically equal validated inputs produce equal runtime and setup plans;
- canonical stage order is stable and ends in `SetupPlanComplete`;
- the nine-edge dependency graph is complete and acyclic;
- requested device, format, and backend values remain requested intent;
- absent selectors and DSP configuration remain absent;
- inconsistent public runtime components return a typed invariant error;
- supported fixtures prepare without panic or external state.

Private constructor tests additionally cover duplicate and reordered stages,
cycles, missing and reordered dependencies, format/topology mismatch, and
backend/device mismatch with typed `SetupPlanningError` values.

## Dependency Boundary

`aurora-runtime-assembly` direct dependencies remain exactly:

```text
aurora-config
aurora-core
```

`cargo tree -p aurora-runtime-assembly --depth 1` confirms this boundary.
No dependency, feature, Cargo manifest, or lockfile changed in Checkpoint D.

The prohibited direct dependency set remains the one in the
[milestone specification](../planning/runtime-assembly-contracts-1.md): no
renderer or DSP API/implementation, real-time engine or backend, CPAL,
simulator, diagnostics, or CLI dependency.

## CI And Validation

Existing CI runs the complete workspace tests and warning-free rustdoc on Linux
stable and Windows stable, plus locked Rust 1.78 workspace checks and tests.
The public contract suite is included automatically; no workflow expansion or
external service is required.

Required local commands:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo +1.78.0 check --workspace --all-targets --all-features --locked
cargo +1.78.0 test --workspace --all-features --locked
actionlint
git diff --check
```

Local results on Windows:

- formatting: PASS;
- all-target, all-feature Clippy with warnings denied: PASS;
- workspace tests: 235 passed, 0 failed, 5 explicitly ignored hardware tests;
- runtime-assembly tests: 47 unit and 6 public contract tests passed;
- warning-free workspace rustdoc: PASS;
- locked Rust 1.78 workspace check and tests: PASS with the same test counts;
- Actionlint: PASS;
- relative links: PASS across 65 tracked Markdown files plus this evidence file;
- dependency-boundary and prohibited-import scans: PASS;
- `git diff --check`: PASS.

The exact Checkpoint D commit is recorded in the Draft PR. Remote Linux,
Windows, MSRV, and Simulation Assurance PR Smoke results are pending until that
PR runs; they are evidence inputs, not acceptance. Existing CI targets Linux
stable, Windows stable, and Rust 1.78. The local results above are build and
unit-test evidence only and do not observe a device or physical path.

## Scope Audit

Checkpoint D constructs and executes no runtime subsystem. It adds no renderer
or DSP construction, factories, backend selection or construction, CPAL,
device discovery, negotiation, stream, callback, real-time execution,
simulator or diagnostics producer integration, CLI, serialization, hashing,
fingerprint, networking, calibration, maximum-object synthesis, Phase 2 work,
Phase 3C work, physical measurement, or latency claim.

Production unsafe code remains forbidden by the crate root. The fixed setup
stage and dependency graph remains bounded. No product or callback behavior is
changed.

## Remaining Evidence

- required Draft PR checks on Linux stable, Windows stable, and Rust 1.78;
- Simulation Assurance PR Smoke result;
- Checkpoint E's separate criterion-by-criterion architectural review.

Runtime Assembly Contracts 1 remains `IN_PROGRESS` and `NOT_EVALUATED`.
Checkpoint E has not started, and no accepted tag or release is authorized.
