# ADR 0015: Deterministic Runtime Setup Planning Boundary

- Status: Proposed; authorized after governance merge
- Date: 2026-07-18
- Milestone: Runtime Assembly Contracts 1
- Execution state: `NOT_STARTED`
- Evaluation classification: `NOT_EVALUATED`

## Context

ADR 0014 established the immutable `PreparedRuntimePlan` boundary and its
deterministic derivation from `ValidatedConfiguration`. Checkpoints A and B
implemented that decision without constructing runtime resources.

The milestone still needs a hardware-independent description of how later
setup work must be ordered and related. The original Checkpoint C covered only
evidence and documentation, so it does not authorize a second descriptive
layer, setup stages, or dependency contracts. Those contracts require an
explicit extension before implementation.

## Decision

Extend Runtime Assembly Contracts 1 with an immutable Aurora-owned setup plan
derived only from `PreparedRuntimePlan`:

```text
ValidatedConfiguration
        |
        v
PreparedRuntimePlan
        |
        v
PreparedSetupPlan
```

`PreparedSetupPlan` is a conceptual name for descriptive setup intent. Exact
Rust names remain subject to implementation review. The plan contains no live
resource and never claims that setup ran or that a device, backend, renderer,
DSP processor, stream, callback, or engine exists.

## Ownership Boundary

The setup-planning contracts remain in `aurora-runtime-assembly`. Public values
are Aurora-owned, immutable, deterministic, hardware-independent, and free of
third-party types. Direct dependencies remain exactly:

```text
aurora-runtime-assembly --> aurora-config --> aurora-diagnostics (existing)
aurora-runtime-assembly --> aurora-core
```

No reverse dependency is authorized. There is no direct dependency on
renderer, DSP, backend, real-time, CPAL, simulator, diagnostics, or CLI crates.
A separate crate is not justified for this bounded extension.

## Permitted Contracts

Checkpoint C may introduce contracts equivalent in purpose to:

- an aggregate prepared setup plan;
- a canonical setup-stage descriptor;
- explicit setup dependencies;
- unresolved input and output device-selection intent;
- requested audio-format setup intent;
- renderer setup intent;
- DSP setup intent;
- backend setup intent.

These names are descriptive, not mandatory implementation spelling. Values may
copy accepted intent and bounded counts from `PreparedRuntimePlan` or derive
structural relationships from those values. They may not contain executable
behavior or observational state.

## Setup-Stage Model

The canonical descriptive order is:

1. device-selection intent;
2. requested-format planning;
3. renderer-preparation intent;
4. DSP-preparation intent;
5. backend-preparation intent;
6. `SetupPlanComplete`.

No stage performs work. `SetupPlanComplete` means only that the immutable
description contains every required stage in canonical order. It is preferred
over `RuntimeReady` because it does not imply that a runtime exists, operates,
or has passed a host or physical readiness check. Stages make no physical,
host, timing, latency, negotiation, or operational-readiness claim.

## Dependency Model

Dependencies are explicit, acyclic, and canonically ordered. At minimum:

- renderer preparation depends on prepared topology/layout, canonical routing,
  and requested audio-format intent;
- DSP preparation depends on requested audio-format intent and the accepted DSP
  schema state;
- backend preparation depends on unresolved device-selection intent and the
  requested-format setup description;
- `SetupPlanComplete` depends on all prior setup intents.

The graph is descriptive. It contains no closures, callbacks, function
pointers, handles, trait objects, runtime objects, or host-discovered edges.
Dependency validation may not execute or probe anything.

## Device Semantics

Device setup data remains unresolved requested intent. It may preserve the
requested input selector, requested output selector, and explicit absence where
accepted. It contains no discovered OS identity, CPAL value, handle,
availability or health claim, negotiated channels or format, endpoint
observation, or physical evidence. A selector is never described as resolved.

## Format Semantics

The setup plan may copy requested format values already present in
`PreparedRuntimePlan`. Names such as `RequestedAudioFormatSetup` or
`PlannedAudioFormat` are preferred over `ConfirmedAudioFormat`, because no
backend confirmation occurs.

Requested values are not negotiated values. The plan makes no claim of backend
acceptance, device support, actual sample rate, actual channel count, actual
callback size, actual latency, or successful negotiation.

## Renderer Setup Semantics

Renderer setup intent may preserve renderer family, validated spread intent,
required prepared topology/layout, and canonical routing dependencies. It may
not contain renderer instances or implementation types, VBAP matrices, lookup
or gain tables, scratch or history buffers, implementation-specific
allocation, physical calibration, or speaker measurements.

No renderer implementation dependency or factory is authorized. Future object
construction requires separate governance.

## DSP Setup Semantics

The current schema permits only an explicit state equivalent to `None` or
`DeferredByCurrentSchema`. Setup planning must preserve that accepted state and
must not synthesize a graph, processor chain, filters, delays, equalization,
calibration, or execution plan. No DSP implementation dependency is
authorized.

## Backend Setup Semantics

Backend setup intent may describe that later integration requires unresolved
device intent, requested format intent, and canonical dependencies. An already
accepted configuration value may be preserved as requested backend intent, but
the setup plan does not select a host implementation.

The plan may not instantiate CPAL, a simulator backend, backend objects,
streams, callbacks, ring buffers, ASRC, or host APIs. Backend identity remains
deferred when accepted configuration does not determine requested intent.

## Determinism Requirements

Equal `PreparedRuntimePlan` values must produce equal setup plans and equal
typed errors. Stage and dependency order is canonical and independent of map,
host, clock, random, process, or filesystem state. No timestamps, UUIDs,
hashes, fingerprints, hostnames, or observations are permitted.

## Capacity And Allocation Policy

ADR 0014 capacity rules remain binding. Setup planning may carry values already
known by `PreparedRuntimePlan` and bounded structural counts derived from them.
It may not invent maximum objects, renderer scratch or history sizes, delay or
ASRC storage, backend ring sizes, or negotiated buffer sizes.

Ordinary ownership allocations for immutable setup-plan values are setup-thread
work and remain bounded by existing validated configuration limits. No large
implementation allocation or callback allocation is authorized.

## Error Policy

Valid `PreparedRuntimePlan` values should normally yield a valid setup plan.
Structured setup-planning errors are reserved for checked representational
limits and defensive invariant protection, such as a missing required prepared
component, inconsistent dependency, cycle, invalid canonical order, or an
impossible mismatch among prepared fields.

Errors never panic, reuse callback-fault terminology, or claim device,
negotiation, host, or physical failure. Artificial failure modes are not
authorized.

## Forbidden Behavior

This decision does not authorize renderer or DSP construction, a renderer
factory, engine wiring, backend selection or construction, device discovery,
format negotiation, stream creation, CPAL, simulator or diagnostics producer
integration, CLI work, serialization, hashing, fingerprints, maximum-object
synthesis, thread or process creation, callbacks, real-time execution, physical
measurement, latency claims, calibration, networking, wireless audio, codecs,
Dolby/DTS behavior, Phase 2 changes, Phase 3C, or tags.

## Options Considered

### A. Keep Checkpoint C as evidence only

Rejected. It leaves setup ordering and dependency ownership undefined before
the milestone's final evidence review.

### B. Extend `aurora-runtime-assembly`

Selected. Setup planning is a bounded descriptive continuation of the
Aurora-owned prepared plan and needs no new dependency direction.

### C. Add a separate setup-planning crate

Rejected for this milestone. It creates another boundary without independent
ownership or dependency needs. A future proposal may revisit this only with a
specific consumer and rationale.

### D. Construct runtime resources directly

Rejected. Construction would cross into renderer, DSP, backend, engine, and
hardware integration and requires separate governance.

## Consequences

- Checkpoints A and B remain governed by ADR 0014 and unchanged.
- Checkpoint C may describe deterministic setup intent after this amendment
  merges, but may not execute it.
- The former evidence checkpoint moves intact to Checkpoint D.
- The former stop boundary moves to Checkpoint E.
- Configuration stays inert, dependency direction stays acyclic, and product
  audio and real-time behavior stay unchanged.
- Phase 2 remains open and Phase 3C remains unauthorized.

## Implementation Authorization

No Checkpoint C implementation is authorized in this governance pull request.
After this amendment merges, a new implementation branch and pull request may
implement only the contracts and deterministic setup-plan derivation described
here. Checkpoint D cannot begin until Checkpoint C is merged and reviewed.

## Stop Boundary

Checkpoint C must stop after immutable setup-planning contracts, deterministic
derivation, defensive setup-level validation, focused tests, and public API
documentation. It may not construct or integrate any runtime subsystem.

Checkpoint E is the final architectural evaluation boundary for Runtime
Assembly Contracts 1. No CLI, renderer factory, runtime engine, backend,
simulator, diagnostics producer, Phase 2, Phase 3C, or later milestone work is
authorized by this ADR.
