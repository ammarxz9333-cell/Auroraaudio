# Diagnostics And Telemetry

## Architecture

`aurora-diagnostics` is a hardware-independent control-plane crate. It has no
dependency on Aurora renderers, DSP, device backends, the real-time engine, or
the CLI. This keeps diagnostic schemas usable by future integrations without
changing protected audio contracts or processing semantics.

The framework has two deliberately separate paths:

1. `RealtimeMetricCounters` is fixed-size atomic state. Callback code may
   record numeric durations, occupancy, and counts. Recording does not allocate,
   lock, format, log, perform I/O, or retain audio.
2. Events, retained logs, JSON, human formatting, snapshots, and reports are
   assembled on control threads. `SharedDiagnosticLog` is for concurrent
   non-real-time producers only.

No wall-clock source is built into the framework. Producers supply either a
deterministic logical timestamp or a monotonic nanosecond observation. This
prevents ambient time from entering deterministic simulation evidence.

## Event Taxonomy

Every event contains `schema_version`, `timestamp`, `component`, `severity`,
`event_id`, a key-sorted structured `payload`, and exactly one `truth_source`.

| Event ID | Meaning |
| --- | --- |
| `startup` | Process or subsystem startup |
| `shutdown` | Clean or faulted shutdown |
| `device_discovery` | Device enumeration or explicit selection observation |
| `capability_report` | Device or software capability information |
| `routing_decision` | Explicit channel or graph routing decision |
| `renderer_selection` | Renderer selected from configuration |
| `state_transition` | Lifecycle state transition |
| `underrun` | Audio-path underrun observation |
| `overrun` | Audio-path overrun observation |
| `recovery` | Recovery attempt or outcome |
| `configuration_validation` | Configuration validation outcome |
| `simulation_execution` | Deterministic simulation progress or result |
| `benchmark_execution` | Benchmark execution or statistics |

Severities are ordered `trace`, `debug`, `info`, `warning`, `error`, and
`critical`. Truth sources are the canonical master-reference values:
`unit_test`, `deterministic_simulation`, `virtual_audio_backend`,
`host_api_observation`, and `physical_measurement`. Only evidence captured from
a documented physical signal path may use `physical_measurement`.
Validation requires `physical_measurement` records to include a non-empty
`physical_signal_path` structured field. The same field is rejected for every
nonphysical truth source, preventing a record from silently upgrading its
evidence classification. Unknown serialized enum values are rejected by Serde.

## Logging Policy

`DiagnosticLog` stores events oldest-first in a `VecDeque` with an explicit
non-zero capacity. It also applies a configurable minimum severity and an
explicit per-event retained-size ceiling. When full, the oldest event is
evicted. Filtering, eviction, and oversized rejection return a
`LogDisposition` and update separate counters; none is a hidden fallback.
Events that fail schema or truth-source validation are likewise rejected and
counted before filtering or retention.

The default per-event estimate ceiling is 16 KiB. Callers may configure a
different non-zero ceiling. The estimate includes string bytes and conservative
fixed overhead per event and payload field. Event count and accepted event size
therefore remain bounded by construction.

JSON Lines and human-readable output preserve event order. Payload maps use
`BTreeMap`; snapshot routing and feature sets use `BTreeSet`. Identical input
objects serialize identically. Output is returned to the caller as a string;
the framework never writes files or streams and never blocks an audio callback.

## Performance Metrics

Atomic metrics cover:

- callback count, total execution nanoseconds, and maximum nanoseconds;
- renderer count, total execution nanoseconds, and maximum nanoseconds;
- current and maximum queue occupancy;
- underruns, overruns, recoveries, and dropped frames;
- simulation frames and host-observed elapsed nanoseconds;
- benchmark sample count, total nanoseconds, and maximum nanoseconds.

`MetricSnapshot` copies these values on a control thread and provides derived
simulation throughput and mean benchmark duration. Timing values are supplied
observations. They are not audio latency and must not be described as physical
measurement without a captured physical loopback path.

## Snapshot Format

`DiagnosticSnapshot` contains:

- active configuration in a key-sorted map;
- selected renderer;
- a sorted routing graph;
- queue occupancy and declared capacity;
- fixed real-time storage plus bounded diagnostics memory capacities;
- enabled features;
- platform, version, and Git commit identity;
- a `MetricSnapshot`;
- exactly one truth source.

The memory summary describes declared and retained capacities; it is not an OS
resident-set measurement. `validate` rejects unsupported schema versions,
missing renderer/build identity, queue occupancy above capacity, and retained
diagnostic bytes above capacity. It also enforces truth-source evidence.

## Report Format

`DiagnosticReport` contains a stable failure category, root component,
reproduction operation and scenario, optional deterministic seed,
recommendation, truth source, and key-sorted context. Categories cover
configuration, device, routing, renderer, real-time transport, simulation,
benchmark, recovery, and internal failures.

`validate` enforces the schema and required text fields. A report whose truth
source is `deterministic_simulation` must include its seed. Reports never infer
or upgrade a truth source. Event validation likewise enforces the schema
version, component, payload keys, and truth-source evidence.

Framework 1 uses schema version `1`. Additive or incompatible schema changes
require an explicit version increment, compatibility tests, and documentation;
unknown versions and enum values are not accepted as version 1.

## Integration Policy

Framework 1 supplies schemas and safe publication primitives; it does not wire
new producers into protected callbacks or alter existing renderer, DSP, engine,
device, state, or fault APIs. Future control-plane integrations must translate
existing numeric snapshots outside callbacks. They must preserve bounded
storage, explicit loss accounting, deterministic order, and truth-source
honesty.

## Verification

The dedicated contract suite covers every event ID, deterministic JSON and
human output, filtering, bounded retention, oversized rejection, concurrent
control-thread producers, atomic concurrent metrics, snapshot validity, report
schema validation, and zero callback metric allocations.

```text
cargo test -p aurora-diagnostics --test contracts
cargo test -p aurora-diagnostics --test callback_safety
```

CI runs the contract suite as a named gate in addition to full workspace
formatting, Clippy, tests, and documentation checks.
