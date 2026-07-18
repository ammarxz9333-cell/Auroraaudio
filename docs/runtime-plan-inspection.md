# Runtime Plan Inspection

## Boundary

`aurora-runtime-inspection` is a software-only leaf crate that borrows accepted
`PreparedRuntimePlan` and `PreparedSetupPlan` accessors and creates a separate
inspection-owned schema. The prepared plans are never serialized, deserialized,
mutated, reconstructed, hashed, persisted, or treated as a wire format.

The only production Aurora dependency remains:

```text
aurora-runtime-inspection -> aurora-runtime-assembly
```

The crate performs no host access, runtime construction, diagnostics producer
wiring, CLI work, or filesystem/network I/O.

## Formatting API

Projection remains explicit and redacted by default:

```rust
let report = InspectionReport::project(&runtime_plan, &setup_plan,
    InspectionOptions::redacted())?;
let json = JsonFormatter::format(&report)?;
let text = TextFormatter::format(&report)?;
```

`JsonFormatter` emits compact JSON with schema field order followed by the
accepted canonical collection order. Serde is private implementation detail for
inspection-owned values only. `TextFormatter` emits fixed sections in this
order: requested, prepared, deferred, setup, and findings. It uses no ANSI
styling, localization, terminal-width wrapping, timestamps, or host values.

## Published Bounds

| Bound | Value |
| --- | ---: |
| JSON bytes | 262,144 |
| Text bytes | 262,144 |
| Nesting depth | 8 |
| Total serialized collection entries | 256 |
| Findings | 32 |
| Source string bytes | 256 per string; 32,768 cumulative |
| Channels / speakers | 32 each |
| Routes | 64 |
| Setup stages / dependencies | 6 / 9 |

Formatters validate fixed nesting, checked aggregate collection counts, finite
floats, and format-specific byte limits. A violation returns a structured
`InspectionError`; semantic entries are never truncated.

## Schema Compatibility

Inspection schema version 1 is a deterministic review format owned by the
inspection crate. Its compatibility boundary is independent of configuration
schemas and Rust prepared-plan representations. The format is not a cache key,
signature, fingerprint, persistence format, or promise that accepted prepared
plans can be reconstructed.

Enum vocabulary and float spelling are locked by exact-output tests. JSON uses
fixed struct field order and no maps. Text uses fixed labels and indentation.
Default redaction preserves field presence and relationships while replacing
device, channel, and speaker identifiers with category-specific markers.

## Evidence Limitation

The report represents requested intent, prepared control-plane facts, and
explicitly deferred setup values only. `SetupPlanComplete` means descriptive
plan completeness, not runtime readiness. Formatting adds no negotiated,
observed, simulated, measured, runtime-ready, latency, health, endpoint, or
physical evidence. Phase 2 remains open and incomplete, and Phase 3C is not
authorized.
