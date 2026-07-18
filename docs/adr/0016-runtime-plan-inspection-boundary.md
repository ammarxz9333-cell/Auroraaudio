# ADR 0016: Read-Only Runtime Plan Inspection Boundary

- Status: Accepted governance; implementation in progress
- Date: 2026-07-18
- Milestone: Runtime Plan Inspection 1
- Authorization state: `AUTHORIZED`
- Execution state: `IN_PROGRESS`
- Evaluation classification: none
- Checkpoint A: `COMPLETE`
- Checkpoint B: `COMPLETE`
- Checkpoint C: active next scope; implementation not started
- Checkpoint D: `NOT_STARTED`

## Context

Runtime Assembly Contracts 1 is accepted and owns deterministic immutable
`PreparedRuntimePlan` and `PreparedSetupPlan` values. Those values deliberately
have no approved consumer. Configuration serialization, diagnostics snapshots,
and runtime assembly have separate accepted ownership boundaries, and none is
an authorization to expose prepared plans as persisted wire formats.

Operators and reviewers need a deterministic way to inspect what Aurora
prepared without probing a host, constructing runtime resources, or confusing
requested intent with negotiated or observed state. Adding serialization to
the accepted plan types would turn their Rust representation into an accidental
compatibility contract. Reusing diagnostics snapshots directly would mix
generic runtime diagnostics with a plan-specific schema and would require
producer wiring that Diagnostics & Telemetry Framework 1 explicitly deferred.

No current implementation milestone is authorized. Phase 2 is blocked by
missing hardware; Phase 3A and Phase 3B are closed and conditionally accepted
pending physical gates; all current software-only milestones are closed.

## Decision

Authorize, only after this governance amendment merges normally, one
software-only milestone named **Runtime Plan Inspection 1**.

The milestone may create a separate `aurora-runtime-inspection` control-plane
crate. It may read accepted public accessors on `PreparedRuntimePlan` and
`PreparedSetupPlan` and translate them into a versioned, bounded,
Aurora-owned inspection model. The inspection model is a projection, not the
prepared plan itself and not a runtime resource.

The crate may provide deterministic JSON and deterministic human-readable
formatting for its own inspection model. It must not add Serde traits or any
other serialization contract to `aurora-runtime-assembly` types. It must not
deserialize or reconstruct prepared plans. It must not define a plan hash,
fingerprint, signature, cache key, or cryptographic identity.

Inspection is control-thread-only. It may allocate within published bounds and
may use the existing workspace Serde and JSON dependencies privately. No
third-party type may cross a public Aurora API.

## Ownership And Dependency Boundary

The permitted direct dependency direction is:

```text
aurora-runtime-inspection --> aurora-runtime-assembly
```

Existing workspace Serde dependencies may be used privately for the
inspection schema. No existing crate may depend back on
`aurora-runtime-inspection` during this milestone. In particular, no direct
dependency from the inspection crate to configuration, diagnostics, renderer,
DSP, engine, backend, simulator, CPAL, audio I/O, scene, or CLI crates is
authorized.

The accepted runtime-assembly crate and its direct dependency boundary remain
unchanged. Inspection must use only its existing public read-only API. A
missing accessor or required protected-contract change is a stop condition,
not permission to modify runtime assembly.

## Inspection Semantics

The versioned inspection model may report only facts already present in a
prepared plan:

- plan schema version and inspection schema version;
- requested audio-format and callback intent;
- renderer selection intent and validated horizontal spread intent;
- canonical routing, output order, active/inactive speaker state, and layout
  descriptors;
- unresolved device-selection intent, redacted by default;
- explicit DSP `None` or schema-deferred intent;
- plan-known capacities and explicit deferred capacities;
- canonical setup stages and dependencies;
- deterministic conformance findings about the inspection projection itself.

Every field must preserve the distinction between requested, prepared,
deferred, negotiated, observed, simulated, and measured state. Inspection may
not infer endpoint availability, negotiated formats, runtime readiness,
successful setup, physical routing, latency, or hardware behavior.

`SetupPlanComplete` remains descriptive completeness only. An inspection
report must not rename it to or present it as runtime readiness.

## Determinism And Bounds

Equal prepared plans and equal explicit inspection options must produce
byte-identical JSON and identical human-readable output on the same schema
version. Ordering must derive from the accepted canonical vectors and fixed
setup dependency order. Output must not depend on hash-map iteration, locale,
wall clock, environment, filesystem, hostname, process ID, random state, or
host APIs.

The implementation must publish finite bounds for retained findings, strings,
routes, speakers, stages, dependencies, and serialized output bytes. It must
reject overflow and oversized output with structured inspection errors. It may
not silently truncate semantic records. Redaction may replace sensitive text
with deterministic category markers but must not claim that omitted data was
absent from the prepared plan.

## Redaction And Truth

Default inspection output must redact requested device names and stable IDs,
channel and speaker labels, and other operator-provided identifiers. An
explicit control-thread option may permit unredacted local output, but tests
must prove that the default is redacted and deterministic.

Inspection evidence uses `unit_test` and `host_api_observation` under existing
truth-source policy. Deterministic output is not a device observation,
simulation result, runtime fact, or physical measurement. This milestone may
not add or change diagnostics truth-source variants or wire a diagnostics
producer.

## Forbidden Behavior

This decision does not authorize:

- changes to accepted runtime-assembly, configuration, diagnostics, renderer,
  DSP, engine, backend, simulator, audio I/O, or CLI public contracts;
- renderer, DSP, backend, stream, callback, or engine construction or
  execution;
- device discovery, selection, resolution, format negotiation, or host access;
- plan deserialization, plan mutation, hot reload, persistence, cache loading,
  fingerprints, hashing, signatures, or compatibility claims beyond the
  inspection schema;
- filesystem or environment access in the library;
- a CLI command, GUI, service, control API, network endpoint, or database;
- Phase 2 work, Phase 3C, elevation, HRTF, Ambisonics, calibration, HDMI,
  networking, wireless audio, codecs, AI, or physical claims.

## Consequences

- Prepared plans gain a reviewable consumer without changing their ownership
  or representation.
- Inspection output has an explicit versioned compatibility boundary separate
  from configuration and prepared-plan schemas.
- Sensitive requested identifiers are redacted by default.
- Runtime construction and integration remain separately governable future
  work.
- Phase 2 remains open and unchanged; no hardware gate is satisfied.

## Implementation Authorization

This governance pull request contains no implementation. After it merges
normally into `main-v2`, a new branch may implement only the checkpoints in
`docs/planning/runtime-plan-inspection-1.md`. Branch creation does not itself
move execution state to `IN_PROGRESS`; the first implementation commit does.

## Stop Boundary

Stop after the isolated inspection crate, versioned bounded projection,
redaction, deterministic JSON and human formatting, focused tests,
documentation, validation, and an unmerged implementation pull request.

Stop immediately if implementation needs a protected-contract change, reverse
dependency, runtime construction, host access, unbounded output, plan
serialization, diagnostics producer wiring, CLI integration, or any forbidden
scope above.
