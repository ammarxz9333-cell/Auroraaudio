# Project Structure Cleanup

## Purpose

This document governs Aurora's structural cleanup before additional product milestones. The cleanup must improve ownership and maintainability without deleting required behavior, weakening accepted contracts, or silently changing command-line, renderer, DSP, realtime, simulation, or evidence semantics.

## Non-Destructive Refactor Rule

No production, experimental, or planning component may be deleted merely because it is not currently active.

A component can be deleted only after all of the following are recorded in the same reviewable change:

1. no active Cargo dependency references it;
2. no feature flag, build script, test, benchmark, fixture, workflow, or example requires it;
3. no active roadmap checkpoint names it as an implementation dependency;
4. any reusable design information has been moved to an implementation-neutral document;
5. a replacement exists when the component owns unique required behavior;
6. migration instructions exist for downstream callers when a public API changes;
7. Linux stable, Windows stable, Rust 1.78 MSRV, rustdoc, and workspace tests pass;
8. the pull request explicitly lists the deleted paths and why future product work does not require them.

Until every condition is satisfied, inactive source is preserved and explicitly excluded from the active workspace.

## Current Branch Stack

```text
main-v2
  -> PR #58: Issue #44 unified evaluation framework
      -> PR #59: project structure cleanup
```

PR #59 is stacked so evaluation corrections remain independently reviewable. It must not be merged before its base is accepted or the branch is cleanly rebased onto the accepted base.

## Active Workspace Definition

The root `Cargo.toml` is the machine-readable authority for active compilation units:

- `workspace.members` are active components;
- `workspace.exclude` entries are preserved but inactive components;
- `[workspace.metadata.aurora]` records preservation policy.

Being excluded does not mean abandoned or safe to delete. Reactivation requires a dedicated design and licensing review followed by normal CI.

## Preserved Legacy Components

### `aurora-renderer-cavern`

Status: preserved, excluded, not product-active.

Reason:

- placeholder only;
- no linked or vendored Cavern implementation;
- unresolved licensing and redistribution posture;
- not required for the first-release architecture.

Preservation rationale:

- retains the previously defined Aurora renderer-boundary experiment;
- allows future comparison if licensing is resolved;
- deletion is unnecessary for workspace cleanliness.

### `aurora-decoder-truehdd`

Status: preserved, excluded, not product-active.

Reason:

- placeholder only;
- no linked or vendored decoder;
- high legal and commercial uncertainty;
- open IAMF ingestion is the preferred active direction.

Preservation rationale:

- retains historical adapter-boundary work;
- prevents accidental loss of prior interface experiments;
- can be re-evaluated independently without affecting the active build.

## Target Repository Shape

```text
crates/
  domain and contracts/
    aurora-core
    aurora-scene
    aurora-config
    aurora-renderer-api
    aurora-dsp-api
    aurora-decoder-api
    aurora-realtime-audio-api

  implementations/
    aurora-renderer-basic
    aurora-renderer-vbap
    aurora-dsp-basic
    aurora-dsp-camilladsp
    aurora-decoder-iamf
    aurora-realtime-audio-cpal
    aurora-realtime-audio-sim

  orchestration/
    aurora-realtime-engine
    aurora-runtime-assembly
    aurora-runtime-inspection
    aurora-audio-io

  evidence and assurance/
    aurora-evaluation
    aurora-diagnostics
    aurora-simulation-assurance
    aurora-measurement

  application/
    aurora-cli
```

The physical directory names will not be moved merely to match this conceptual grouping. Moving crate directories creates broad path churn and merge risk without improving dependency ownership. The grouping is architectural, while crate names and paths remain stable unless a later review demonstrates a concrete benefit.

## Dependency Direction

Allowed direction:

```text
domain data
  -> Aurora-owned contracts
      -> implementations
          -> orchestration
              -> application composition
```

Evidence crates may consume public contracts and outputs, but production processing crates must not depend on evidence formatting or the CLI.

Forbidden directions include:

- renderer or DSP crates depending on `aurora-cli`;
- realtime callback code depending on filesystem or JSON artifact writers;
- core/domain crates depending on CPAL, CamillaDSP, simulator, or host-specific types;
- production processing crates depending on `aurora-evaluation`;
- control-plane description crates claiming runtime construction or hardware readiness.

## CLI Decomposition Plan

The current `aurora-cli/src/main.rs` is too large and mixes argument parsing, orchestration, artifact formatting, host interaction, and feature-gated implementations. It will be decomposed without changing public command syntax.

Target shape:

```text
crates/aurora-cli/src/
  main.rs                 # parse and dispatch only
  args.rs                 # clap model and value enums
  commands/
    mod.rs
    render.rs
    evaluate.rs
    process.rs
    realtime.rs
    duplex.rs
    measurement.rs
    simulation.rs
    devices.rs
    doctor.rs
    identify.rs
  artifacts/
    mod.rs
    evaluation.rs
  support/
    mod.rs
    git.rs
    paths.rs
```

### Migration order

1. add characterization tests for CLI parsing and command names;
2. move pure value enums and conversions to `args.rs`;
3. move Git provenance and path quoting helpers to `support/`;
4. move evaluation artifact structs and JSON writing to `artifacts/evaluation.rs`;
5. move the complete evaluation command as one unit;
6. move offline rendering and gains;
7. move CamillaDSP process orchestration;
8. move device and realtime output commands;
9. retain existing `realtime_commands.rs` and `simulation_commands.rs` behavior while wrapping them in command modules;
10. reduce `main.rs` to parse plus dispatch;
11. compare `--help` output and representative command output against pre-refactor snapshots.

No function is removed from the old location until the moved function compiles and its behavior is covered.

## Behavioral Compatibility Matrix

The following behavior must remain unchanged during CLI decomposition:

| Surface | Required compatibility |
| --- | --- |
| command names | byte-identical spelling and aliases |
| option names | unchanged long names and defaults |
| renderer alias | `binaural` remains accepted for `geometric-binaural` |
| exit status | success/failure behavior preserved |
| stdout keys | existing machine-readable key names preserved |
| artifact names | existing filenames preserved unless schema migration explicitly documents a rename |
| feature-disabled errors | same semantic errors remain available |
| realtime callback | no new allocation, logging, lock, filesystem, or process access |
| evaluation | no renderer or DSP mathematics change |

## Required Validation Per Refactor Commit

Minimum local/CI gates:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo +1.78.0 check --workspace --all-targets --all-features --locked
cargo +1.78.0 test --workspace --all-features --locked
```

Additional structural checks:

- root workspace does not include preserved legacy crates;
- preserved legacy files remain present;
- no active crate depends on excluded crate paths;
- CLI help snapshots remain equivalent;
- evaluation artifacts retain their documented schema behavior;
- hardware-only tests remain explicitly gated rather than deleted.

## Current Cleanup Checkpoints

### S0 — Safety baseline

Status: in progress.

- preserve legacy source;
- exclude inactive placeholders explicitly;
- document deletion gates;
- rewrite architecture around current ownership.

### S1 — CLI characterization

Status: not started.

- add parser tests;
- capture command and option surface;
- record representative stdout/artifact behavior.

### S2 — CLI support extraction

Status: not started.

- extract support and artifact helpers;
- no command behavior changes.

### S3 — Command decomposition

Status: not started.

- extract evaluation, rendering, process, realtime, duplex, simulation, and measurement commands in small commits.

### S4 — Documentation consolidation

Status: not started.

- eliminate duplicate active execution claims;
- retain historical ADR and acceptance records as history;
- make one current execution reference authoritative.

### S5 — Dependency and inactive-component audit

Status: not started.

For every crate, record one status:

- active and required;
- active but intentionally incomplete;
- preserved experimental;
- planned but not implemented;
- candidate for replacement;
- candidate for deletion after full gates.

### S6 — Final structural review

Status: not started.

- full CI and benchmarks;
- independent review;
- no merge while stacked base is unaccepted;
- no claim that structural cleanup completes product functionality.

## Stop Conditions

Stop the refactor and do not continue deleting or moving code when any of these occurs:

- a dependency or feature reference is ambiguous;
- a test disappears instead of being migrated;
- CLI output compatibility cannot be demonstrated;
- the change modifies renderer, DSP, callback, synchronization, or acoustic behavior;
- the change requires simultaneous edits across unrelated milestones;
- the branch cannot pass MSRV or cross-platform CI;
- an excluded component is discovered to own unique behavior required by an active roadmap checkpoint.

In those cases, preserve the old component, document the blocker, and isolate the required product change in a separate implementation PR.
