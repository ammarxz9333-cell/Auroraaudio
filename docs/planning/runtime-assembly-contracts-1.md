# Runtime Assembly Contracts 1

## Current implementation state

- `authorization_state`: `AUTHORIZED`
- `execution_state`: `IN_PROGRESS`
- `evaluation_classification`: `NOT_EVALUATED`
- `completed_checkpoint`: `A`
- `active_checkpoint`: `B`
- implementation branch: `feature/runtime-assembly-contracts-1-checkpoint-b`
- review state: Checkpoint B implementation prepared for Draft PR; open and unmerged

Checkpoint A's isolated immutable contract model is merged. Checkpoint B adds
one deterministic, fallible derivation entry point from
`&ValidatedConfiguration`, focused mapping and determinism tests, and no
runtime construction, hardware access, or subsystem integration. This does not
start Checkpoint C or classify the milestone as accepted.

## Governance record

- Milestone class: `SOFTWARE_ONLY_CONTROL_PLANE`
- `authorization_state`: `AUTHORIZED_AFTER_GOVERNANCE_MERGE`
- `execution_state`: `NOT_STARTED`
- `evaluation_classification`: `NOT_EVALUATED`
- Governance branch: `governance/runtime-assembly-contracts-1`
- Implementation branch: a new branch created only after governance merge
- Target final classification: `ACCEPTED`

This specification authorizes a future bounded control-plane implementation.
It is not an implementation record and authorizes no code before its merge.

## Purpose and flow

Runtime Assembly Contracts 1 converts an already validated configuration into
an immutable preparation plan:

```text
ValidatedConfiguration
        |
        v
PreparedRuntimePlan
        +-- PreparedRendererPlan
        +-- PreparedRoutingPlan
        +-- PreparedDspPlan
        +-- PreparedDeviceIntent
        +-- RuntimeCapacityPlan
        +-- RuntimePlanMetadata
```

The proposed Aurora-owned entry point is:

```text
prepare_runtime_plan(&ValidatedConfiguration)
    -> Result<PreparedRuntimePlan, RuntimePreparationError>
```

Exact Rust spelling may be refined in implementation review, but these
semantics are binding. The builder accepts only the validated view, returns
owned immutable values, exposes no third-party types, and performs no device,
process, thread, callback, or physical work.

## Contract boundaries

Construction is deterministic for semantically equal normalized validated
configurations. It is bounded by accepted configuration limits: 32 inputs, 32
outputs, 64 routing edges, and 32 speakers. Derived multiplication and
conversion use checked arithmetic and reject overflow.

The plan is control-plane data. All allocation occurs during setup. Future
callback-facing state must be separately constructed with fixed capacities.
Callbacks may not mutate the plan or perform serialization, string or map work,
formatting, logging, filesystem access, process work, or allocation. This
milestone does not claim that an implementation already exists or is
zero-allocation.

## Prepared plan content

The plan may carry only semantic intent available from
`ValidatedConfiguration`. Version 1 includes:

- configuration schema version;
- normalized sample-rate, sample-format, channel-count, and format-fallback
  intent;
- callback-frame intent;
- stable input and output identities;
- canonical output order, routing edges, and inactive outputs;
- layout kind, speaker identities, canonical roles, and normalized geometry;
- Basic, point-source VBAP, or horizontal-spread renderer selection intent;
- requested device selectors and ambiguity policy;
- known bounded counts and control-plane metadata.

The following are not known from validated configuration and must not appear as
facts in the prepared plan:

- negotiated backend format or actual endpoint availability;
- renderer implementation scratch size or physical channel availability;
- actual callback size, device latency, or live engine state;
- physical measurement or production ASRC behavior;
- a final maximum object count;
- a complete DSP execution plan.

These values remain unavailable in the current schema, setup-time derived, or
deferred to a separately authorized milestone. A future API that makes one of
them mandatory must reject its absence instead of silently choosing a value.

## Renderer preparation

| Validated renderer intent | Prepared description | Construction |
| --- | --- | --- |
| `Basic` | Basic inverse-distance | Deferred to later setup integration |
| `PointSourceVbap` | Point-source horizontal VBAP | Deferred |
| `HorizontalSpread { spread }` | Horizontal-spread VBAP with validated spread | Deferred |
| `Unsupported` or future reserved intent | Structured setup rejection | None |

The description contains only Aurora-owned scalar or enum values. It holds no
`Renderer`, implementation object, dynamic plugin, or implementation-crate
type. It does not change defaults or the protected `Renderer` trait. HRTF,
Ambisonics, elevation, 3D/triplet VBAP, Cavern, and dynamic plugin loading are
excluded.

The current configuration schema has no maximum-object field. Version 1 must
therefore mark maximum object count and renderer output-gain capacity as later
setup-time derivations and must not invent a default. Output-gain capacity will
eventually be a checked product of output count and an explicit object limit.
Renderer scratch is likewise an implementation-specific setup derivation after
renderer construction, not a fabricated number in this plan.

The known renderer output-gain width for one object equals the validated output
channel count. Total output-gain storage remains deferred because object count
is unavailable.

## Routing preparation

The routing plan copies only validated normalized intent:

- stable input and output identities;
- canonical output order from normalized roles and assignments;
- input-to-output edges in canonical `(output, input)` order;
- inactive outputs retained in stable order;
- bounded channel, speaker, and edge counts;
- no duplicate assignments or unknown references.

Validation guarantees the last rule; derivation rechecks internal lookup
invariants. The plan does not process samples or authorize new multichannel
behavior. Existing real-time mono downmix remains unchanged. Multichannel Input
& Routing Model 1 remains a later milestone.

## DSP preparation

The accepted configuration schema currently carries no DSP configuration.
Version 1 therefore prepares the explicit Aurora-owned state `None`. It may
reserve enum space for future internal-basic, external-adapter, and unsupported
intent, but must not accept or synthesize unavailable DSP intent.

A separately authorized future schema may distinguish internal basic DSP,
policy-limited external adapter intent, and reserved/unsupported intent. An
unavailable adapter must produce setup rejection without fallback. This
milestone adds no filter, coefficient, delay behavior, CamillaDSP type, or
realtime claim; it neither launches CamillaDSP nor changes adapter integration.
A future DSP Configuration milestone may therefore be required before a full
DSP execution plan can be assembled.

## Device intent

`PreparedDeviceIntent` preserves requested backend, direction, selector,
ambiguity policy, and requested format/buffering values as intent. Stable IDs
and friendly names are not evidence that a device exists.

The contract keeps these concepts distinct:

1. requested configuration;
2. prepared intent;
3. negotiated backend format;
4. observed host behavior;
5. physical measurement.

Only the first two exist here. Preparation does not enumerate devices, resolve
a friendly name against a host, silently select a first match, negotiate a
format, confirm channels, open a stream, or change the device state machine.

## Capacity plan

| Capacity | Rule |
| --- | --- |
| Input channels | Validated requested count, at most 32 |
| Output channels | Validated requested count, at most 32 |
| Callback frames | Validated maximum callback-frame intent |
| Routing edges | Exact validated count, at most 64 |
| Maximum objects | Deferred because no accepted configuration field exists |
| Renderer output gains | Deferred until maximum objects is explicit; checked product required |
| Renderer scratch | Deferred implementation-specific setup derivation |
| Delay buffers | Deferred; no DSP intent exists in the schema |
| Ring storage | Validated ring-capacity intent; checked channel/sample products later |

Absence of an accepted source is explicit, never represented by zero, an
undocumented default, or host probing. Future materialization rejects invalid
required capacities and arithmetic overflow before allocation.

The capacity contract has two levels:

- plan-known: input/output channel counts, routing-edge count, speaker count,
  callback-frame intent, and per-object renderer output-gain width;
- setup-derived: renderer scratch and history, implementation-specific
  temporary storage, delay-processor storage, ASRC storage, and backend ring
  capacities.

Setup-derived requirements are calculated only by a later integration layer
after the relevant implementation has been configured. The plan exposes no
concrete renderer scratch type or third-party capacity type.

## Setup error model

`RuntimePreparationError` will use structured categories with stable machine
codes and optional control-thread detail:

- unsupported renderer intent;
- unsupported reserved configuration;
- invalid derived capacity;
- arithmetic overflow;
- incompatible routing/layout intent;
- unsupported DSP intent;
- unavailable policy-limited adapter;
- internal invariant violation.

These are setup errors, not `RealTimeFault`, device-state events, runtime
recoveries, or physical failures. Owned descriptive strings are permitted only
because these errors never enter steady-state callback processing.

## Diagnostics metadata and truth

`RuntimePlanMetadata` may expose configuration schema version, renderer
selection, input/output counts, layout kind, routing edge count, DSP selection,
and requested-selector presence. It cannot claim negotiated format, accepted
device, `host_api_observation`, or `physical_measurement`.

No diagnostics dependency or producer wiring is authorized. Tests and future
control-plane translation use existing `unit_test` truth. "Deterministic
control-plane derivation" is descriptive provenance, not a new `TruthSource`.

## Determinism, serialization, and fingerprint

Stable equality is required. It follows normalized vectors and explicit enum
discriminants and cannot depend on `HashMap` iteration, host enumeration,
wall-clock time, randomness, filesystem order, or platform defaults.

Serde is intentionally excluded because no persistence or interchange consumer
is approved. Contract tests compare typed values. A plan fingerprint is also
deferred. A future fingerprint requires a separately reviewed versioned field
set and Aurora-owned algorithm, excludes nonsemantic provenance such as
`generated_by`, and makes no cryptographic claim. This PR adds no hash dependency.

## Crate placement decision

ADR 0014 selects **Option B**, a future `aurora-runtime-assembly` crate.

| Option | Decision |
| --- | --- |
| A: `aurora-runtime-plan` | Rejected: obscures validated derivation responsibility |
| B: `aurora-runtime-assembly` | Selected: independent setup/control-plane ownership |
| C: module in `aurora-config` | Rejected: configuration must remain inert |
| D: module in `aurora-realtime-engine` | Rejected: couples plan to engine/callback concerns |

Permitted direct dependencies:

```text
aurora-runtime-assembly --> aurora-config --> aurora-diagnostics (existing)
aurora-runtime-assembly --> aurora-core
```

There is no direct dependency on diagnostics, renderer or DSP APIs or
implementations, real-time engine, backend APIs, CPAL, simulator, or CLI. No
existing crate may depend back during this milestone. Later consumers require
separate authorization and cannot create a cycle.

The default prohibited direct-dependency set is `aurora-renderer-api`,
`aurora-renderer-basic`, `aurora-renderer-vbap`, `aurora-dsp-api`,
`aurora-dsp-basic`, `aurora-realtime-engine`, `aurora-realtime-audio-api`,
`aurora-realtime-audio-cpal`, `aurora-realtime-audio-sim`,
`aurora-diagnostics`, and `aurora-cli`.

## Dependency matrix

| Crate | Assembly dependency | Why | May depend back now | Boundary types | Modified later |
| --- | --- | --- | --- | --- | --- |
| `aurora-core` | Yes, direct | Canonical channel/layout vocabulary | No | Aurora values copied into owned plan | No |
| `aurora-config` | Yes, direct | Sole validated entry point | No | `ValidatedConfiguration` at input only | No |
| `aurora-renderer-api` | No | Plan describes intent, not objects | No | No | No |
| `aurora-renderer-basic` | No | Avoid implementation leakage | No | No | No |
| `aurora-renderer-vbap` | No | Avoid implementation leakage | No | No | No |
| `aurora-dsp-api` | No | No accepted DSP mapping | No | No | No |
| `aurora-dsp-basic` | No | No processor construction | No | No | No |
| `aurora-realtime-engine` | No | Keep callbacks/engine separate | No | No | No |
| `aurora-realtime-audio-api` | No | No stream/backend work | No | No | No |
| `aurora-realtime-audio-cpal` | No | No host dependency | No | No | No |
| `aurora-realtime-audio-sim` | No | Plan is not a simulator | No | No | No |
| `aurora-diagnostics` | No direct dependency | Metadata remains plan-owned | No | No diagnostics types | No |
| `aurora-cli` | No | Inspection command deferred | No | No | No |

## Implementation checkpoints

### Checkpoint A: isolated contracts

- scaffold `aurora-runtime-assembly`;
- add immutable plan model and structured setup errors;
- depend only on `aurora-config` and `aurora-core`;
- add no integration.

### Checkpoint B: deterministic derivation

- build from `&ValidatedConfiguration` only;
- map renderer, routing, device, and explicit no-DSP state;
- derive available bounded capacities and preserve deferred capacities;
- reject overflow and invariant violations.

### Checkpoint C: evidence and documentation

- add unit and contract tests;
- prove deterministic typed equality and canonical ordering;
- cover invalid cases and document public APIs;
- run Linux, Windows, MSRV, Clippy, tests, and rustdoc.

### Checkpoint D: stop

No CLI command is authorized. Plan inspection, engine construction, renderer
factory, diagnostics producer, and backend integration require new governance.

## Required implementation tests

- identical validated configuration produces an identical plan;
- canonical input/output and routing order is independent of source vector
  order;
- Basic maps to inverse-distance intent;
- point-source VBAP maps correctly;
- horizontal-spread VBAP preserves validated spread;
- reserved/unsupported renderer intent is rejected;
- standard and custom horizontal layouts map canonically;
- inactive outputs are retained;
- device selector remains intent only;
- capacity overflow is rejected;
- maximum configured channel/routing bounds are accepted without growth;
- missing future DSP detail remains explicit or is rejected without synthesis;
- missing maximum object capacity is never silently defaulted;
- no physical hardware, CPAL, or callback execution is required;
- the crate graph has no real-time engine, renderer implementation, DSP
  implementation, diagnostics, or CLI dependency;
- no third-party type crosses the public boundary;
- metadata and errors make no physical or host-observation claim.

Serde round trips are intentionally excluded. Benchmarks are not required:
construction is bounded setup work over at most 32 channels and 64 edges. A
benchmark needs evidence of meaningful performance risk.

## Hardware dependency matrix

| Gate | Decision |
| --- | --- |
| Software-only | Immutable contracts, deterministic mapping, bounds, errors, tests, docs |
| Deterministic simulation | None; simulator is unchanged |
| Host observation | Build and CI results only; no devices |
| Hardware-dependent | None |
| Phase 2 required for implementation | No |
| Phase 2 required for acceptance | No |
| Effect on Phase 2 | None; remains open and incomplete |

## Non-goals

Excluded: RealTimeEngine wiring, CPAL streams, device enumeration/negotiation,
physical validation or latency, callback changes, hot reload, runtime mutation,
GUI, networking, wireless audio, HDMI/eARC, codecs, Dolby/DTS, IAMF decoding,
room calibration, AI, multichannel input redesign, Phase 2 changes, Phase 3C,
and accepted-tag changes.

## Acceptance criteria

Implementation may be `ACCEPTED` only when architecture matches ADR 0014 with
no cycle; `ValidatedConfiguration` is the sole entry point; the plan is
immutable and deterministic; capacities are bounded and checked; errors are
structured; no device I/O, callback wiring, process/thread work, or product
audio behavior change exists; required tests/docs pass on Linux, Windows, and
MSRV 1.78; and no physical claim is made.

This pure software milestone has no physical gate. Acceptance does not alter
Phase 2 or authorize Phase 3C.

## Stop boundary

After governance merge, only Runtime Assembly Contracts 1 implementation is
authorized on a new branch and PR. The agent must not continue into renderer
factory integration, a virtual end-to-end harness, diagnostics producer wiring,
multichannel routing redesign, Phase 2, Phase 3C, or any later milestone.
