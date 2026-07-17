# ADR 0013: Versioned Configuration And Bounded Presets

- Status: Accepted for Configuration & Preset System 1
- Date: 2026-07-17

## Decision

Place Aurora configuration intent in an independent `aurora-config` control-
plane crate. Keep raw deserialized models inert and require an owned
`ValidatedConfiguration` wrapper before consumption. Normalize ordered
collections only after validation and serialize canonical JSON with existing
Serde infrastructure.

Treat `generated_by` as non-semantic provenance. Preserve preset reference
order as explicit precedence, forbid cycles, cap composition depth, and reject
direct same-type conflicts. Restrict migration to explicit version pairs with
changed-field records, bounded warnings, destination validation, and no silent
field loss.

Use the accepted diagnostics enums for diagnostics policy and redacted snapshot
truth sources. Keep serialization, validation, materialization, migration, and
redaction outside protected callbacks.

## Consequences

- semantically identical validated inputs produce byte-identical canonical JSON;
- callers cannot mistake configuration intent for device discovery or format
  negotiation evidence;
- schema evolution requires an explicit version and migration path;
- preset behavior is reviewable and bounded rather than scriptable;
- existing renderer, DSP, backend, state, fault, device, and callback contracts
  remain unchanged;
- schema 1 does not support runtime hot reload or partial live mutation.
