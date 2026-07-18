# Runtime Materialization Contracts 1

## Authorization Record

- Milestone: Runtime Materialization Contracts 1
- Milestone kind: software-only control-plane contracts
- `authorization_state`: `AUTHORIZED_AFTER_GOVERNANCE_MERGE`
- `milestone_status`: `PROPOSED`
- `execution_state`: `NOT_STARTED`
- `evaluation_classification`: `NOT_EVALUATED`
- Governance branch: `governance/runtime-materialization-contracts-1`
- Required base: `main-v2` after this governance pull request merges normally
- Active checkpoint: none
- Checkpoint A: `NOT_STARTED`
- Checkpoint B: `NOT_STARTED`
- Checkpoint C: `NOT_STARTED`
- Checkpoint D: `NOT_STARTED`
- Expected terminal classification: `ACCEPTED`
- Phase 2 required for implementation: no
- Phase 2 required for acceptance: no
- Hardware acceptance criteria: none

No implementation checkpoint is active before this governance change merges.
The first implementation commit after a reviewed merge moves execution to
`IN_PROGRESS`; branch creation alone does not.

Governance basis: [ADR 0017](../adr/0017-runtime-materialization-contracts-boundary.md),
the accepted [runtime assembly](../acceptance/runtime-assembly-contracts-1.md)
and [runtime inspection](../acceptance/runtime-plan-inspection-1.md) records,
and the repository master reference.

## Repository Audit Decision

Runtime Materialization Contracts 1 is the correct next software-only
architectural milestone. The audit found no accepted equivalent contract and
no current crate that owns this responsibility:

- `aurora-runtime-assembly` owns immutable prepared runtime/setup intent and
  canonical descriptive setup stages, but stops before concrete resource
  requirements or construction;
- `aurora-runtime-inspection` owns read-only inspection of accepted prepared
  plans and stops before runtime construction or new plan ownership;
- `aurora-config` owns inert configuration and preset materialization, which is
  unrelated to runtime-resource materialization;
- `aurora-realtime-engine` and backend crates own executable runtime behavior;
- `aurora-realtime-audio-sim` owns simulated execution, not construction
  policy; and
- `aurora-diagnostics` owns diagnostics values, not resource planning.

A new near-leaf crate can consume accepted assembly contracts without a reverse
dependency or cycle. Its output is a passive requirement description, not a
constructed resource set. The milestone can be implemented, tested, and
accepted without a device, host, simulator, or physical hardware.

## Objective

Define deterministic, bounded, immutable Aurora-owned contracts that translate
borrowed `PreparedRuntimePlan` and `PreparedSetupPlan` values into a passive
description of what a future separately governed constructor would require.

The result describes materialization-planned resource kinds, stages,
dependencies, capability requirements, and explicitly deferred requirements.
It does not allocate implementation resources, resolve devices, construct
components, start a runtime, or prove readiness.

## Motivation

Accepted prepared plans define what Aurora intends to set up, but a future
constructor still needs a reviewed contract for resource responsibilities,
dependency order, required capabilities, and unavailable concrete capacities.
Defining that vocabulary before construction keeps hidden defaults and
implementation-specific handles out of accepted plans, preserves semantic
honesty, and gives later construction work a bounded testable input boundary.

## Architectural Context

The accepted control-plane flow currently ends at prepared plans and their
read-only inspection. This milestone may add one passive successor:

```text
PreparedRuntimePlan + PreparedSetupPlan
        |
        v
RuntimeMaterializationRequest
        |
        v
RuntimeMaterializationPlan
```

The lower plan is still data. No arrow to a renderer, DSP processor, backend,
stream, callback, or engine is authorized.

## Ownership

Create one future crate:

```text
aurora-runtime-materialization
```

It owns materialization-planning values, limits, validation, errors, and, only
in Checkpoint C, its own inspection projection and formatters. It does not own
prepared plans or executable runtime resources.

Permitted Aurora production dependency direction:

```text
aurora-runtime-materialization --> aurora-runtime-assembly
```

No reverse dependency is authorized. `aurora-runtime-inspection` remains
unchanged and does not depend on the new crate during this milestone. The new
crate must not depend directly on configuration, core, diagnostics, renderer,
DSP, engine, backend, simulator, CPAL, CLI, scene, or audio-I/O crates.

Checkpoint C may use existing workspace `serde` and `serde_json` privately for
a materialization-owned inspection schema. No third-party type may cross a
public API. This governance pull request adds no dependency.

## Dependency Matrix

| Dependency or gate | Decision |
| --- | --- |
| Runtime Assembly Contracts 1 | Required and `ACCEPTED`; use existing borrowed public accessors only |
| Runtime Plan Inspection 1 | `ACCEPTED` and unchanged; no dependency in either direction during this milestone |
| Configuration & Preset System 1 | Indirect only through runtime assembly; preset materialization remains separate ownership |
| Diagnostics Framework 1 | No dependency or producer integration |
| Real-time engine and audio API | No dependency; executable resources and callbacks remain outside scope |
| Simulator and assurance campaign | No dependency; fixtures may be reused only by a future governed consumer |
| Renderer and DSP crates | No dependency; only accepted prepared intent may be described |
| Existing Serde workspace dependencies | Private Checkpoint C detail for materialization-owned inspection values only |
| Software-only gates | Ownership, derivation, ordering, bounds, errors, semantics, docs, and tests |
| Deterministic simulation gates | None |
| Host-observation gates | Build and CI results only; no device observation |
| Hardware gates | None |
| Phase 2 required for implementation | No |
| Phase 2 required for acceptance | No |
| Effect on Phase 2 | None; Phase 2 remains open and incomplete |

## Public Contract Candidates

Checkpoint review may refine spelling but not responsibility. Candidate
Aurora-owned contracts are:

- `RuntimeMaterializationRequest`: a read-only request over one paired
  `PreparedRuntimePlan` and `PreparedSetupPlan`, plus explicit bounded planning
  options if later justified;
- `RuntimeMaterializationPlan`: immutable materialization-planned result;
- `RuntimeResourceDescriptor`: aggregate description of one future resource
  responsibility, never a handle or object;
- `RuntimeResourceKind`: closed materialization-planning categories;
- `MaterializationStage`: canonical passive planning stage;
- `MaterializationDependency`: directed dependency between known descriptors;
- `MaterializationCapabilityRequirement`: requirement that a future
  constructor would need to satisfy, not an observed capability;
- `DeferredMaterializationRequirement`: requirement whose concrete value is
  unavailable from accepted prepared plans;
- `MaterializationError`: structured planning and invariant failures; and
- `plan_runtime_materialization`: deterministic Checkpoint B entry point
  returning `Result<RuntimeMaterializationPlan, MaterializationError>`.

The returned plan is the planning result. No public type named or documented as
a constructed, active, ready, negotiated, or observed runtime result is
authorized.

## Semantic Vocabulary

| Term | Meaning in this milestone |
| --- | --- |
| `requested` | User intent preserved by accepted configuration/prepared plans |
| `prepared` | Deterministically normalized intent already owned by runtime assembly |
| `materialization-planned` | Passive requirements derived from paired prepared plans |
| `deferred` | Requirement is known but its concrete value is unavailable from accepted inputs |
| `constructed` | Future resource creation actually completed; unavailable and forbidden as a fact here |
| `active` | A running resource or runtime state; unavailable and forbidden as a fact here |

`materialization-planned` is not a synonym for prepared, constructed, active,
negotiated, observed, simulated, measured, supported, healthy, or ready.
`MaterializationPlanComplete` may mean only that the bounded description is
complete.

## Resource And Stage Model

Resource descriptors are aggregate planning records, not one record per sample,
frame, route, or speaker. They may represent only responsibilities already
implied by accepted setup intent: requested device-selection data, requested
format data, topology/routing storage requirements, renderer state, DSP state,
backend state, and bounded shared storage requirements.

Canonical stage order follows the six accepted setup stages:

1. device-intent requirements;
2. requested-format requirements;
3. renderer requirements;
4. DSP requirements;
5. backend requirements;
6. `MaterializationPlanComplete`.

Dependencies are explicit, acyclic, and ordered by canonical stage, then
resource-kind discriminant, then canonical source index. The accepted
`PreparedSetupPlan` dependency order is authoritative. Derivation may add only
resource-level edges implied by represented requirements; it may not invent a
different setup workflow.

## Deterministic Derivation Rules

- Input is a borrowed, matching `PreparedRuntimePlan` and
  `PreparedSetupPlan` pair.
- Derivation uses only accepted public accessors and explicit options.
- Equal paired inputs and equal options produce equal typed plans or equal
  typed errors.
- Canonical vectors and fixed setup dependency order are preserved.
- No map iteration, platform default, locale, wall clock, randomness,
  environment, filesystem, process, hostname, or host API affects output.
- Pair mismatch returns a structured error; one source is never selected
  silently.
- Missing concrete capacities remain deferred; no zero, guessed default, or
  host-probed value replaces them.
- Multiplication, addition, conversion, indexing, and cumulative bounds use
  checked arithmetic.

## Published Finite Bounds

Checkpoint A must publish constants no larger than these schema-1 limits:

| Bound | Maximum |
| --- | ---: |
| Aggregate resource descriptors | 32 |
| Materialization stages | 6 |
| Materialization dependencies | 64 |
| Capability requirements | 64 |
| Deferred requirements | 32 |
| One copied string | 256 UTF-8 bytes |
| Total copied strings | 32768 UTF-8 bytes |
| Materialization inspection JSON | 262144 bytes |
| Materialization inspection text | 262144 bytes |
| Serialized collection entries | 256 |
| Inspection nesting depth | 8 |

If implementation review proves a limit insufficient, work stops for a
governance amendment. Semantic entries are rejected rather than truncated.
Ordinary bounded setup-thread allocation is permitted; callback use is
forbidden.

## Structured Errors

`MaterializationError` must distinguish at least:

- source-plan mismatch;
- unsupported prepared intent;
- invalid canonical ordering;
- invalid or cyclic dependency;
- limit exceeded with actual and maximum values;
- checked arithmetic overflow with the failed operation;
- missing required prepared fact;
- contradictory deferred requirement; and
- materialization-owned inspection output too large.

Errors describe planning failures only. They are not callback faults, device
errors, construction failures, endpoint-health reports, or physical evidence.
No panic, silent fallback, or silent truncation is allowed.

## Inspection And Conformance Evidence

Checkpoint C is justified because materialization plans need reviewable exact
evidence without changing the accepted Runtime Plan Inspection 1 crate.

Checkpoint C may define a versioned materialization-owned inspection projection
and deterministic bounded JSON/text formatters. It must not add Serde to
`RuntimeMaterializationPlan`, `PreparedRuntimePlan`, or `PreparedSetupPlan` and
must not make any of them persisted wire formats. It may emit fixed bounded
conformance findings derived solely from represented values.

Test fixtures are contract evidence. They may be reusable by a future simulator
test, but this milestone adds no simulator dependency or simulated execution.

## Read-Only Inspection Compatibility

Materialization contracts expose immutable read-only accessors suitable for a
future separately governed inspection consumer. This milestone does not change
or depend on `aurora-runtime-inspection`, and it creates no reverse dependency.
Checkpoint C's local inspection projection exists only to provide bounded
conformance evidence for materialization-owned values; it does not absorb or
supersede Runtime Plan Inspection 1.

## Out Of Scope And Prohibited Capabilities

This milestone does not authorize:

- real device discovery, real device opening, resolution, capability probing,
  or selection;
- format negotiation or endpoint provisioning;
- CPAL, host APIs, streams, callbacks, real-time execution, clocks, timing, or
  latency measurement;
- renderer, DSP, backend, transport, ASRC, engine, thread, or process
  construction or execution;
- filesystem or environment access in production code;
- diagnostics producer integration, CLI, GUI, service, database, or control
  API integration;
- networking, multiroom, wireless, HDMI/eARC, fleet, OTA, or mobile work;
- physical acoustic simulation, HRTF, Ambisonics, elevation, calibration, room
  modeling, or design exploration;
- Phase 2 changes, Phase 3C, physical validation, or physical claims;
- prepared-plan mutation, serialization, deserialization, reconstruction,
  hashing, fingerprints, signatures, cache identity, or persistence;
- changes to accepted public contracts, records, or tags; and
- unsafe production Rust.

## Checkpoint Workflow

### Checkpoint A: isolated crate and marker contracts

- scaffold `aurora-runtime-materialization` with only the authorized dependency;
- establish module and ownership boundaries;
- define public enums and immutable marker/value contracts;
- publish finite constants and structured marker errors;
- add compile and public-contract tests;
- add no derivation, inspection formatting, or materialization execution.

### Checkpoint B: deterministic materialization-plan derivation

- derive materialization-owned values from borrowed paired prepared plans;
- preserve canonical resource, stage, and dependency ordering;
- enforce finite validation and checked arithmetic;
- return structured errors without panic, fallback, or truncation;
- add deterministic, maximum-bound, mismatch, deferred-state, and fixture tests;
- construct no resource.

### Checkpoint C: deterministic inspection and conformance evidence

- add a materialization-owned versioned inspection projection if still
  justified at checkpoint review;
- add bounded deterministic compact JSON and human-readable text;
- add exact-output, escaping, maximum-output, and three-run determinism tests;
- serialize inspection-owned values only;
- add no prepared-plan serialization or runtime execution.

### Checkpoint D: final validation and architectural evaluation

- run complete local and remote validation;
- audit dependency direction, accepted/protected contracts, public API,
  determinism, bounds, semantic honesty, and prohibited scope;
- decide every acceptance criterion as `PASS`, `FAIL`, or justified
  `NOT_APPLICABLE`;
- issue exactly one terminal decision: `ACCEPTED` or `REJECTED`;
- stop without starting another milestone.

Each checkpoint requires separate review and merge before the next starts.
This governance pull request implements none of them.

## Acceptance Criteria

1. The new crate's only direct Aurora production dependency is
   `aurora-runtime-assembly`, with no reverse dependency or cycle.
2. No accepted public type, trait, schema, behavior, record, or tag changes.
3. Equal paired prepared plans and equal options produce equal materialization
   plans or equal structured errors.
4. Resource, stage, requirement, deferred-requirement, and dependency order is
   canonical and documented.
5. Every collection and output is bounded by published finite constants.
6. Checked arithmetic covers all size, count, index, conversion, and cumulative
   accounting operations.
7. Limit and invariant violations return structured errors with no panic,
   fallback, silent truncation, or partial success.
8. Prepared plans remain borrowed, immutable, and unserialized; no plan can be
   reconstructed from materialization output.
9. Requested, prepared, materialization-planned, deferred, constructed, and
   active semantics remain distinct.
10. Deferred capacities remain explicit and no implementation-specific value
    is guessed or host-probed.
11. No renderer, DSP, backend, stream, callback, engine, thread, process, or
    other runtime resource is constructed or executed.
12. Production code has no host, device, filesystem, environment, network,
    clock, randomness, or hardware access and uses no unsafe Rust.
13. Public APIs use Aurora-owned or standard Rust types and have warning-free
    strict rustdoc plus focused public-contract tests.
14. Deterministic fixtures and exact inspection outputs, if Checkpoint C remains
    justified, repeat byte-identically three times within all bounds.
15. Full formatting, all-target/all-feature Clippy, workspace tests, strict
    rustdoc, Rust 1.78, Actionlint, dependency scans, and remote checks pass.
16. Review confirms no runtime-readiness, endpoint-health, negotiated,
    observed, simulated, measured, latency, hardware, or physical claim.
17. Review confirms no Phase 2 or Phase 3C work and no Physical Acoustic
    Simulator 1 implementation.

## Validation Strategy

Every implementation checkpoint must run the checks applicable to its scope;
Checkpoint D must run all of:

```text
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo +1.78.0 check --workspace --all-targets --all-features
actionlint
```

Checkpoint C exact-output and deterministic contract tests must run three
consecutive times. Automated scans must prove the direct and reverse dependency
boundary, no accepted-crate modification, no prepared-plan serialization, no
unsafe production code, no prohibited imports/capabilities, and no dishonest
semantic vocabulary. Benchmarks are required only if implementation review
identifies a material setup-time or memory risk; timing is
`host_api_observation`, never latency or physical evidence.

## Compatibility Limitations

- Schema 1 describes requirements known from accepted prepared contracts; it
  is not a constructor protocol or stable wire format.
- Materialization-owned inspection output, if added, has its own schema version
  and cannot reconstruct any plan.
- New upstream prepared-plan variants or required capabilities may require a
  separately reviewed materialization schema revision.
- Deferred requirements remain unresolved until a future construction
  milestone is governed.
- Simulator-testable fixtures prove contract determinism only, not runtime or
  simulated device behavior.

## Risks And Mitigations

| Risk | Mitigation |
| --- | --- |
| Duplicates setup planning | Consume accepted stages/dependencies and describe only resource requirements |
| Becomes hidden runtime constructor | No implementation dependencies, handles, factories, callbacks, or execution APIs |
| Requested intent is presented as readiness | Restricted vocabulary and semantic-honesty tests |
| Deferred values gain fabricated defaults | Explicit deferred types and structured errors |
| Dependency cycle through inspection or engine | One permitted Aurora edge and reverse-dependency scans |
| Unbounded descriptor/report growth | Published collection/string/output limits and checked arithmetic |
| Plan becomes accidental wire format | No Serde on plan types; optional separate inspection projection only |
| Confusion with preset materialization | ADR and rustdoc separate preset expansion from runtime-resource planning |
| Future consumer treats evidence as physical | No observed/measured fields and no hardware acceptance gate |

## Stop Boundary

Stop after Checkpoint D's criterion-by-criterion architectural evaluation and
terminal decision. Do not merge automatically, create an accepted tag, or
start another milestone without separate authorization.

Stop immediately if implementation requires an accepted-contract change,
reverse dependency, concrete resource construction, renderer/DSP/backend or
engine dependency, host/device access, unbounded storage, prepared-plan
serialization, runtime execution, diagnostics/CLI integration, Phase 2,
Phase 3C, Physical Acoustic Simulator 1, or a physical/readiness/latency claim.
