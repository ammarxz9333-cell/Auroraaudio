# ADR 0017: Runtime Materialization Contracts Boundary

- Status: Proposed; authorized after governance merge
- Date: 2026-07-18
- Milestone: Runtime Materialization Contracts 1
- Authorization state: `AUTHORIZED_AFTER_GOVERNANCE_MERGE`
- Milestone status: `PROPOSED`
- Execution state: `NOT_STARTED`
- Evaluation classification: `NOT_EVALUATED`
- Checkpoints A-D: `NOT_STARTED`

The complete checkpoint specification, dependency matrix, limits, criteria,
and validation strategy are in the
[milestone plan](../planning/runtime-materialization-contracts-1.md).

## Context

Runtime Assembly Contracts 1 is accepted and owns immutable
`PreparedRuntimePlan` and `PreparedSetupPlan` values. Runtime Plan Inspection 1
is accepted and owns bounded read-only inspection of those plans. Neither
milestone describes the aggregate resource requirements that a future runtime
constructor would need, and both explicitly stop before construction.

No existing crate owns that intermediate responsibility. Configuration is
inert and its preset "materialization" means deterministic preset expansion,
not runtime-resource planning. The real-time engine and audio backends own
executable behavior and would couple a hardware-independent contract to host,
stream, callback, and timing concerns. Diagnostics and simulation have separate
accepted ownership.

A direct jump from prepared plans to construction would leave resource kinds,
dependency order, capability requirements, deferred capacities, bounds, and
error semantics implicit. It would also risk presenting requested intent as
negotiated capability or runtime readiness.

## Decision

Authorize, only after this governance amendment merges normally, one
software-only milestone named **Runtime Materialization Contracts 1**.

The milestone may create `aurora-runtime-materialization`, a near-leaf
control-plane crate that derives immutable materialization-planned requirement
values from borrowed, matching prepared runtime and setup plans.

The plan may describe aggregate future resource responsibilities, canonical
materialization stages and dependencies, capability requirements, explicit
deferred requirements, finite capacities, and structured planning errors. It
contains no resource handle, executable object, factory, closure, callback,
thread, process, stream, backend, renderer, DSP processor, or engine.

`MaterializationPlanComplete` means only that the passive bounded requirement
description is complete. It does not mean construction ran, a device exists, a
format was negotiated, a runtime is ready, or any host or physical behavior was
observed.

## Separation From Runtime Assembly

Runtime assembly owns preparation and descriptive setup intent. It answers:
"what normalized runtime/setup intent was prepared?"

Runtime materialization contracts answer a narrower successor question:
"what bounded resource responsibilities and unresolved requirements would a
future constructor need to satisfy?"

The new crate consumes accepted stage and dependency order. It may not replace,
duplicate, reorder, or modify `PreparedSetupPlan`, and it may not move
materialization contracts into `aurora-runtime-assembly`. The accepted assembly
crate remains immutable and unchanged.

## Separation From Runtime Construction

Materialization planning is data derivation. Runtime construction would create
implementation resources, resolve devices, negotiate formats, allocate
callback-facing storage, instantiate renderers/DSP/backends, open streams, and
connect an engine. None of those operations is authorized.

A future constructor requires a separate ADR and milestone. This decision does
not provide an execution trait, factory interface, plugin loader, backend
selector, or runtime startup API.

## Ownership And Dependency Direction

The sole permitted Aurora production edge is:

```text
aurora-runtime-materialization --> aurora-runtime-assembly
```

No existing crate may depend back during this milestone. In particular,
`aurora-runtime-inspection` remains unchanged and does not gain a dependency on
materialization. There is no direct dependency on configuration, core,
diagnostics, renderer, DSP, real-time engine, audio backend API, CPAL,
simulator, CLI, scene, or audio I/O.

Checkpoint C may use existing workspace Serde/JSON crates privately for a
separate materialization-owned inspection projection. It may not serialize
materialization plans directly or add serialization to either accepted
prepared-plan type. This governance change adds no dependency.

## Semantic Distinctions

- `requested`: user intent retained by accepted upstream contracts;
- `prepared`: normalized passive intent in accepted prepared plans;
- `materialization-planned`: deterministic resource requirements derived from
  prepared intent;
- `deferred`: a required concrete value is unavailable from accepted inputs;
- `constructed`: a future constructor created a resource; unavailable here;
- `active`: a constructed runtime is running; unavailable here.

Constructed and active state cannot be represented as successful facts by this
milestone. Requested and prepared values cannot be relabeled as negotiated,
supported, observed, measured, ready, healthy, or physical.

## Determinism, Ordering, And Bounds

Equal paired prepared plans and equal options must produce equal typed plans or
equal structured errors. Derivation uses only public read-only accessors,
accepted canonical vectors, and fixed setup dependency order. It cannot depend
on hash iteration, locale, platform defaults, clock, randomness, environment,
filesystem, process, hostname, host API, or device state.

Resource descriptors are aggregate and bounded. Checkpoint A must publish the
schema-1 maxima defined by the milestone plan: 32 resources, 6 stages, 64
dependencies, 64 capability requirements, 32 deferred requirements, 256 bytes
per string, and 32768 total string bytes. Checkpoint C output is limited to
262144 bytes per format, 256 collection entries, and nesting depth 8.

Every size, count, index, conversion, and cumulative operation uses checked
arithmetic. Oversized or contradictory input returns a structured error. No
semantic record is silently truncated and no hidden fallback is allowed.

## Schema And Compatibility Limitations

The materialization plan is an internal Rust control-plane contract, not a
constructor protocol, persistence schema, cache identity, compatibility
signature, or wire format. It cannot reconstruct a prepared plan.

Checkpoint C may create a separately versioned inspection projection for
materialization-owned values. That format carries only planning facts and has
no runtime-readiness or physical compatibility meaning. New upstream variants
or future concrete constructor requirements may require a later reviewed schema
revision.

## Rejected Runtime And Physical Claims

The plan cannot establish endpoint availability, supported formats, negotiated
channels, actual callback sizes, runtime allocation success, stream health,
engine state, latency, physical routing, audible behavior, or hardware
readiness. Build, test, and CI evidence is software evidence only.

Phase 2 remains open and incomplete. Phase 3C remains `NOT_STARTED`. This ADR
does not satisfy, weaken, or replace any physical gate.

## Forbidden Behavior

This decision does not authorize device discovery or real device opening,
CPAL, host access, format negotiation, endpoint provisioning,
renderer/DSP/backend/engine
construction or execution, streams, callbacks, threads, processes, clocks,
timing, filesystem/environment access, diagnostics producer wiring, CLI/GUI,
networking, multiroom, HDMI/eARC, wireless, fleet management, OTA, mobile work,
physical acoustic simulation, calibration, HRTF, Ambisonics, Phase 2, Phase 3C,
unsafe production Rust, prepared-plan mutation/serialization/reconstruction,
hashing, fingerprints, signatures, persistence, or physical/readiness/latency
claims.

## Options Considered

### A. Extend `aurora-runtime-assembly`

Rejected. Accepted assembly ownership ends at prepared and setup intent.
Resource-requirement planning has a distinct future consumer boundary and
should not reopen the accepted crate.

### B. Add `aurora-runtime-materialization`

Selected. A near-leaf crate preserves accepted contracts, keeps dependency
direction acyclic, and gives bounded requirement planning an explicit owner.

### C. Put contracts in `aurora-realtime-engine`

Rejected. It couples hardware-independent planning to executable callbacks,
renderer/DSP implementations, and engine behavior.

### D. Extend `aurora-runtime-inspection`

Rejected. Inspection is a read-only projection boundary, not an owner of new
runtime planning semantics. Its accepted dependency and schema stay unchanged.

### E. Construct resources directly

Rejected. Construction crosses renderer, DSP, backend, engine, host, and
real-time boundaries and requires separate governance.

## Consequences

- Accepted prepared plans remain immutable and unserialized.
- Resource-requirement planning gains one bounded owner without a dependency
  cycle.
- Construction, execution, host access, and physical validation remain
  explicitly deferred.
- A future constructor must consume these contracts through a separately
  reviewed boundary and cannot infer readiness from plan completeness.
- Phase 2 and Phase 3C remain unchanged.

## Checkpoint Authorization

The four checkpoints are independently reviewed:

- A: isolated crate and public marker/value contracts;
- B: deterministic materialization-plan derivation;
- C: deterministic materialization-owned inspection and conformance evidence;
- D: final validation and architectural evaluation.

This governance pull request implements none of them. After it merges, only
Checkpoint A may begin on a new branch. Checkpoints may not be combined or
started before the preceding checkpoint is reviewed and merged.

## Stop Boundary

Stop after Checkpoint D's terminal `ACCEPTED` or `REJECTED` decision. Stop
earlier if any work requires a protected-contract change, reverse dependency,
resource construction, execution, host/device access, unbounded storage,
prepared-plan serialization, Phase 2, Phase 3C, Physical Acoustic Simulator 1,
or a physical/readiness/latency claim.
