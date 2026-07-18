# Runtime Plan Inspection 1

## Authorization Record

- Milestone: Runtime Plan Inspection 1
- Milestone kind: software-only control-plane inspection
- `authorization_state`: `AUTHORIZED`
- `execution_state`: `IN_PROGRESS`
- `evaluation_classification`: none
- Governance branch: `governance/runtime-plan-inspection-1`
- Governance merge: PR `#29`,
  `02deb93374b162d11b18b4010452254f3ecd1c18`
- Checkpoint A merge: PR `#30`,
  `b847344c8a64e2b605aead3b6cef8979f39b9916`
- Implementation branch: `feature/runtime-plan-inspection-1-checkpoint-b`
- Required base: `main-v2` after the governance pull request merges normally
- Active checkpoint: B
- Completed checkpoints: A
- Checkpoints C and D: `NOT_STARTED`
- Expected terminal classification: `ACCEPTED`
- Phase 2 required for implementation: no
- Phase 2 required for acceptance: no
- Hardware acceptance criteria: none

The governance amendment and ADR 0016 merged normally. Checkpoint A merged
through PR `#30`; Checkpoint B is the only active implementation scope.
Checkpoints C and D remain `NOT_STARTED` until separately reviewed.

## Milestone Inventory And Selection

The governance review found no currently authorized implementation milestone:

| Milestone | Execution state | Evaluation | Remaining boundary |
| --- | --- | --- | --- |
| Simulation Sprint 1 | `CLOSED` | `ACCEPTED` | Frozen; physical validation remains outside it |
| Phase 2 physical hardware validation | open/incomplete | `BLOCKED_BY_HARDWARE` | Input, loopback, and multichannel endpoints required |
| Phase 3A | `CLOSED` | `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE` | Physical routing, endpoint, stability, and audible gates |
| Phase 3B | `CLOSED` | `CONDITIONALLY_ACCEPTED_PENDING_HARDWARE` | Physical spread, routing, level, endpoint, and stability gates |
| Simulation Assurance Campaign 1 | `CLOSED` | `ACCEPTED` | No further product implementation authorized |
| Diagnostics & Telemetry Framework 1 | `CLOSED` | `ACCEPTED` | Producer wiring requires new governance |
| Configuration & Preset System 1 | `CLOSED` | `ACCEPTED` | Runtime consumption and hot reload remain deferred |
| Runtime Assembly Contracts 1 | `CLOSED` | `ACCEPTED` | Consumers and inspection require new governance |

Runtime Plan Inspection 1 is selected because the accepted runtime-assembly
record explicitly defers plan inspection to later governance. It is
hardware-independent, consumes existing immutable public contracts, changes no
audio behavior, and does not overlap plan derivation, configuration
serialization, diagnostics snapshots, or runtime construction.

## Objective

Provide a deterministic, bounded, read-only inspection projection for accepted
`PreparedRuntimePlan` and `PreparedSetupPlan` values so developers and future
control-plane tools can review prepared intent without constructing or
executing a runtime subsystem.

## Motivation

Aurora can validate configuration and derive prepared runtime/setup plans, but
the plans intentionally have no approved consumer. Review currently depends on
Rust debug structure or bespoke test assertions. A dedicated inspection
boundary makes requested intent, deferred capacities, canonical routing, and
setup dependencies reviewable while preventing the prepared Rust model from
becoming a persisted wire contract.

The milestone also creates a safe place for default redaction and explicit
requested-versus-negotiated terminology. It does not solve runtime assembly,
device negotiation, or diagnostics producer integration.

## Included Scope

- a separate `aurora-runtime-inspection` crate;
- an immutable Aurora-owned inspection schema with an explicit version;
- deterministic projection from borrowed prepared runtime and setup plans;
- bounded structured inspection errors and conformance findings;
- redacted-by-default requested identifiers;
- deterministic JSON and deterministic human-readable formatting for the
  inspection schema only;
- public API documentation, focused tests, fixtures if required, and CI
  validation;
- architecture and inspection-format documentation.

## Dependency Matrix

| Dependency or gate | Decision |
| --- | --- |
| Runtime Assembly Contracts 1 | Required and accepted; consume existing public read-only APIs only |
| Configuration & Preset System 1 | Indirect through runtime assembly only; no direct dependency or raw configuration input |
| Diagnostics Framework 1 | No direct dependency; no snapshot/report schema reuse or producer wiring |
| Phase 3A / Phase 3B | Vocabulary already projected by runtime assembly; no renderer dependency or behavior change |
| Simulation Sprint / Assurance Campaign | No dependency; no simulator execution or duplicate harness |
| Existing Serde workspace dependencies | Private implementation detail for the new inspection schema only |
| Software-only gates | Schema, projection, terminology, redaction, determinism, bounds, errors, docs, and tests |
| Deterministic simulation gates | None; the milestone does not execute simulation |
| Host-observation gates | Build, CI, and optional setup-thread benchmark observations only |
| Hardware gates | None |
| Phase 2 required for implementation | No |
| Phase 2 required for acceptance | No |
| Effect on Phase 2 | None; Phase 2 remains open and incomplete |

Permitted direct crate dependency:

```text
aurora-runtime-inspection --> aurora-runtime-assembly
```

No reverse dependency and no direct dependency on configuration, diagnostics,
renderer, DSP, engine, backend, simulator, CPAL, CLI, scene, or audio I/O is
authorized.

## Public Contract Boundary

The milestone may add public types owned by the new inspection crate. It may
not modify any accepted public type or trait. Public inspection APIs must accept
borrowed prepared plans and explicit immutable options and return owned
inspection values or structured errors.

No Serde implementation may be added to runtime-assembly types. Serialization
belongs only to inspection-owned values. No third-party type may appear in a
public signature.

## Versioned Inspection Content

Schema 1 may include:

- inspection and source plan schema versions;
- requested format and callback intent;
- renderer intent and horizontal spread intent;
- canonical routes, outputs, speakers, active state, roles, and normalized
  horizontal geometry;
- unresolved requested device intent with default redaction;
- DSP absent/deferred state;
- known and deferred capacities;
- setup stages, canonical order, and dependency edges;
- bounded conformance findings generated solely from represented plan values.

Every field name and rustdoc must preserve requested/prepared/deferred
semantics. The schema must contain no negotiated, observed, simulated,
measured, ready, active-runtime, latency, endpoint-health, or physical field.

## Bounds

The implementation must publish and enforce limits for:

- speakers, routes, outputs, stages, and dependencies, no larger than accepted
  upstream bounds;
- retained conformance findings;
- copied string bytes and individual string length;
- serialized inspection bytes;
- nesting depth and collection counts.

Checked arithmetic is required. Oversized projection or output returns a typed
error. Semantic entries may not be silently truncated. Setup/control-thread
allocation within these bounds is permitted; callback use is prohibited.

## Redaction Policy

Default output redacts device selectors, stable IDs, friendly names, speaker
labels, channel labels, and operator provenance using deterministic category
markers. Explicit unredacted output may be available only through an
inspection option supplied by the control-thread caller.

Redaction must preserve structure and presence. It must not turn a present
selector into an absent selector or alter counts, ordering, roles, active
state, or dependencies.

## Out Of Scope

- modifying accepted runtime-assembly, configuration, diagnostics, renderer,
  DSP, backend, engine, simulator, scene, audio I/O, or CLI contracts;
- constructing or executing renderers, DSP, backends, streams, callbacks, or
  engines;
- device discovery, resolution, capability probing, format negotiation, or
  hardware access;
- serializing/deserializing prepared plans, mutating plans, reconstructing
  plans from inspection, persistence, hot reload, caches, fingerprints,
  hashing, signing, or cryptographic claims;
- diagnostics snapshot/report producer wiring or truth-source changes;
- CLI commands, filesystem output, environment reads, GUI, service, control
  API, database, networking, HDMI, wireless audio, codecs, AI, or calibration;
- Phase 2 changes, Phase 3C, elevation, triplet VBAP, HRTF, or Ambisonics;
- physical, runtime-readiness, negotiated-format, or latency claims.

## Checkpoint Breakdown

### Checkpoint A: isolated inspection contracts

- scaffold the new crate with only the authorized dependency direction;
- define versioned immutable inspection values, options, bounds, and typed
  errors;
- document requested/prepared/deferred semantics;
- add no projection, formatting, CLI, or integration.

### Checkpoint B: deterministic projection and redaction

- project borrowed runtime and setup plans through existing public accessors;
- preserve canonical ordering and explicit deferred values;
- implement deterministic default redaction and explicit local unredacted
  option;
- validate bounds and return typed errors without truncation.

### Checkpoint C: inspection formatting and conformance evidence

- serialize inspection-owned schema values to deterministic JSON;
- format deterministic human-readable output;
- add bounded conformance findings about represented plan relationships;
- add exact-output, redaction, maximum-bound, failure, and repeated-run tests;
- document schema compatibility and limitations.

### Checkpoint D: validation and final evaluation

- run all required local and remote checks;
- audit dependencies, protected contracts, prohibited scope, and public docs;
- perform criterion-by-criterion evaluation;
- stop with an unmerged implementation pull request and no later work.

Each checkpoint requires separate review before the next begins. Checkpoint A
must not silently include Checkpoint B behavior.

## Acceptance Criteria

1. The new crate depends directly only on `aurora-runtime-assembly`; existing
   workspace Serde crates remain private implementation dependencies.
2. No accepted public type, trait, behavior, schema, record, or tag changes.
3. Equal prepared plans and equal options produce byte-identical JSON and
   identical human output for inspection schema 1.
4. Canonical route, output, speaker, stage, and dependency order is preserved.
5. Requested, prepared, deferred, and absent values remain distinguishable;
   no negotiated, observed, ready, measured, or physical state is inferred.
6. Default output deterministically redacts all specified operator identifiers
   while preserving structure and presence.
7. Maximum accepted upstream plan sizes project within published bounds;
   overflow and oversized output return typed errors without panic or silent
   truncation.
8. Inspection cannot deserialize or reconstruct a plan and defines no hash,
   fingerprint, signature, or cache identity.
9. Production code uses no unsafe Rust and no filesystem, environment, host,
   process, thread, device, clock, or randomness source.
10. Tests cover both plan kinds, every renderer intent, canonical and custom
    layouts, inactive speakers, unresolved selectors, DSP absent/deferred,
    known/deferred capacity, redaction, deterministic repeated output,
    maximum bounds, invalid projections, and structured errors.
11. Strict Rustdoc documents every public API and the inspection schema
    compatibility boundary.
12. Formatting, all-target/all-feature Clippy, workspace tests, strict Rustdoc,
    Rust 1.78, Actionlint, dependency scans, and repeated deterministic contract
    tests pass.
13. Review confirms no runtime execution, protected-contract change, Phase 2
    change, Phase 3C work, physical claim, or out-of-scope integration.

## Validation Strategy

Future implementation validation must include:

```text
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
actionlint
cargo +1.78.0 check --workspace --all-targets --all-features
```

Inspection contract and exact-output tests must run three consecutive times.
A dependency scan must prove the permitted direct edge and absence of reverse
edges. A prohibited-scope scan must cover filesystem, environment, host,
process, thread, clock, randomness, runtime construction, and forbidden crate
imports. Workspace benchmarks must run as regression evidence; inspection-
specific benchmarks are required only if implementation review identifies a
meaningful setup-time or memory risk. Any timing is `host_api_observation`, not
latency.

## Risks And Mitigations

| Risk | Mitigation |
| --- | --- |
| Inspection becomes accidental prepared-plan wire format | Separate versioned projection; no Serde on plan types; no reconstruction |
| Requested intent is presented as negotiated or ready | Restricted vocabulary, schema tests, and explicit forbidden fields |
| Sensitive selectors or labels leak | Deterministic redaction by default and exact redaction tests |
| Duplicate diagnostics or configuration ownership | No direct dependencies, no producer wiring, and plan-only input |
| Dependency cycle | Leaf crate and automated direct/reverse dependency scans |
| Unbounded report growth | Published collection/string/byte bounds and typed rejection |
| Schema changes silently alter output | Explicit inspection schema version and exact fixture review |
| Future consumers treat output as physical evidence | Truth-source restrictions and no observed/measured fields |

## Stop Boundary

Stop after Checkpoint D evaluation and creation of an unmerged implementation
pull request. Do not merge automatically and do not create an accepted tag
before formal acceptance.

Stop immediately if any checkpoint requires modifying an accepted contract,
adding a reverse dependency, accessing host or physical state, executing or
constructing runtime components, serializing prepared plans, adding a CLI or
diagnostics producer, exceeding fixed bounds, or entering Phase 2, Phase 3C,
or another milestone.
