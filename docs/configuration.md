# Configuration And Presets

## Architecture

`aurora-config` is an independent control-plane crate. It depends only on the
existing Serde JSON infrastructure and the accepted `aurora-diagnostics`
taxonomy. It has no renderer implementation, DSP, backend, engine, CLI, file
I/O, or callback dependency and uses `#![forbid(unsafe_code)]`.

Raw `AuroraConfiguration` values are inert intent. Consumers construct
`ValidatedConfiguration`, which validates and normalizes an owned value and
then exposes immutable borrows. The crate never probes a device or claims that
a requested format was negotiated.

## Schema Model

Production schema 1 is named `aurora.configuration`, has
`schema_version=1`, and requires `minimum_reader_version=1`. Unknown fields and
unknown enum values are rejected except unknown renderer names, which are
retained long enough to return the structured `unsupported_renderer` error.

The root configuration contains:

- engine identifier, operating mode, startup/shutdown intent, and recovery
  policy reference;
- requested audio format and explicit reject/allow-list fallback policy;
- optional input/output device intent with an explicit ambiguity policy;
- named input/output channels, routes, and explicitly inactive outputs;
- stereo, 5.1, 7.1, or irregular horizontal speaker layout intent;
- Basic, Phase 3A point-source VBAP, or Phase 3B horizontal-spread selection;
- bounded buffering policy requiring preallocated runtime storage;
- diagnostics policy using accepted severity and truth-source vocabulary;
- optional deterministic simulation profile and replay metadata.

Speaker elevation is reserved metadata only. `elevation_rendering=true` is
rejected and this milestone does not activate elevation behavior.

## Canonicalization

Canonical JSON is compact UTF-8 JSON emitted by Serde after validation and
normalization. Struct field order is declaration order. Inputs, outputs,
routes, inactive outputs, speakers, preset IDs, and tags use explicit stable
ordering. Ordered sets represent diagnostic truth sources. Hash-map iteration,
wall-clock values, random IDs, local paths, and platform separators are absent.

`schema.generated_by` is provenance, not semantic configuration. It is removed
from canonical JSON and semantic equality. Preset `extends` order is preserved
because it defines overlay precedence. Floating-point fields must be finite;
Serde supplies one stable shortest JSON representation for accepted values.

Indented diagnostic output applies the same normalization and omission rules.
Three repeated contract executions compare canonical bytes exactly.

## Validation Phases

Validation runs before normalization:

1. serialized byte and schema bounds;
2. required and bounded UTF-8 strings;
3. sample rate, channel count, callback frames, and fallback policy;
4. unambiguous device-selection intent without first-match fallback;
5. unique channel IDs, known routes, unique assignments, and explicit inactive
   outputs;
6. unique speakers, finite horizontal geometry, and complete standard roles;
7. renderer/layout compatibility and normalized spread;
8. ordered buffer fill bounds and mandatory preallocation intent;
9. bounded diagnostics and deterministic simulation values.

The validator rejects NaN/infinity, unsupported versions, duplicate IDs,
missing active routes, unknown inactive outputs, unsupported renderer names,
active reserved elevation, and every published bound violation.

## Error Taxonomy

`ConfigError` always contains a stable `ErrorCode`, dot-separated field path,
stable `ErrorCategory`, bounded detail, and optional bounded remediation. It
does not include source JSON, credentials, selectors, or other user values.

Categories are schema, validation, device intent, routing, renderer, preset,
migration, and bounds. Codes distinguish parse/version failures, numeric and
string validation, duplicate IDs, routing, ambiguity, renderer support, buffer
bounds, preset conflict/cycle/depth/reference, migration ambiguity/data loss,
and serialized-size rejection.

## Preset Model

Preset collections contain at most 128 presets. Each preset has a stable ID,
display name, schema version, declared type, immutable typed payload, optional
description, sorted tags, and ordered references.

Supported payloads are full configuration, renderer, speaker layout, routing,
diagnostics, and simulation. Materialization walks references depth-first and
left-to-right, then applies the current payload. The latest payload therefore
has explicit precedence. A full configuration base must appear before an
overlay. Direct references of the same payload type conflict rather than
silently choosing one.

Cycles, missing references, depth greater than 8, inconsistent declared types,
and conflicts return structured errors. There is no scripting, expression
language, arbitrary merge, or runtime mutation.

## Migration Policy

Migration is explicit, offline, deterministic, and bounded. Framework 1
provides the reviewable fixture migration from source version 0 to destination
version 1. It updates schema metadata and converts deprecated
`renderer.spread_percent` in `0..=100` to normalized `renderer.spread` in
`0.0..=1.0`.

The result records sorted changed-field paths and a bounded deprecation warning,
then validates against schema 1. Missing, ambiguous, out-of-range, unknown, or
oversized input fails. Unknown data is not silently discarded because the
destination schema denies unknown fields.

## Redaction

`RedactedConfiguration` is created only from a validated configuration on the
control thread and records an accepted diagnostics `TruthSource`. Generated-by
metadata, device friendly names, channel labels, speaker labels, and simulation
replay IDs are omitted or replaced. Strict mode also replaces stable device IDs.

There are no token, network credential, or secret fields in schema 1. Every
string and serialized object is bounded, so arbitrary large values cannot enter
a snapshot. Redaction does not wire a producer into a protected callback.

## Published Bounds

| Resource | Limit |
| --- | ---: |
| serialized configuration or collection | 256 KiB |
| ordinary string | 256 UTF-8 bytes |
| description | 1,024 UTF-8 bytes |
| speakers | 32 |
| routes | 64 |
| channels per direction | 32 |
| presets per collection | 128 |
| tags per preset | 16 |
| composition depth | 8 |
| fallback sample rates | 8 |
| retained migration diagnostics | 32 |

No recursion, collection, error aggregation, or retained payload is unbounded.

## Examples

Validated fixture examples are under `fixtures/config/`. Canonical stereo,
5.1, 7.1, irregular, Phase 3A, Phase 3B, invalid, and migration fixtures are
checked by the focused contract suite.

```rust
use aurora_config::ValidatedConfiguration;

let bytes = include_bytes!("../fixtures/config/stereo-basic-v1.json");
let configuration = ValidatedConfiguration::from_json(bytes)?;
let canonical = configuration.canonical_json()?;
# Ok::<(), aurora_config::ConfigError>(())
```

## Security Considerations

- parse size is checked before JSON deserialization;
- all collections, text, composition, and migration evidence are bounded;
- first-device and silent format fallback are rejected;
- errors do not echo secrets or selectors;
- redacted output removes local device and user-facing labels;
- no file, process, device, network, environment, or wall-clock access exists;
- production code contains no unsafe Rust.

## Truth Sources

Milestone evidence uses `unit_test`, `deterministic_serialization`,
`schema_validation`, and `host_api_observation`. The first three are evaluation
labels, not additions to the runtime diagnostics enum; runtime diagnostic
records map them to `unit_test`. Benchmark timings are host observations, not
audio latency or physical measurements.

## Non-Goals

No runtime hot reload, CLI command, automatic reconfiguration, UI, database,
network/cloud control, hardware probing, physical acceptance, renderer/DSP
algorithm, callback wiring, Phase 3C, elevation rendering, HRTF, Ambisonics,
room correction, codec, or Dolby feature is included.
