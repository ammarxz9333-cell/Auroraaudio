# ADR 0014: Runtime Assembly Boundary

- Status: Proposed; authorized after governance merge
- Date: 2026-07-17
- Milestone: Runtime Assembly Contracts 1
- Execution state: `NOT_STARTED`
- Evaluation classification: `NOT_EVALUATED`

## Context

Aurora has an accepted immutable `ValidatedConfiguration`, renderer and DSP
contracts, and separate real-time engines/backends. It lacks an owned boundary
that converts validated intent into a deterministic preparation plan without
probing hardware or constructing a running engine.

Putting this policy in configuration would make configuration active. Putting
it in the real-time engine would couple reusable control-plane derivation to
callback concerns. Depending on renderer/DSP implementations would leak
construction choices and risk dependency cycles.

## Decision

A later implementation will create the separate Aurora-owned crate
`aurora-runtime-assembly`. It will provide an immutable prepared plan and a
deterministic setup-time builder whose only configuration input is
`&ValidatedConfiguration`.

The permitted direct dependency graph is:

```text
aurora-runtime-assembly --> aurora-config --> aurora-diagnostics (existing)
aurora-runtime-assembly --> aurora-core
```

No existing crate may depend back during this milestone. There is no direct
assembly dependency on diagnostics, renderer/DSP APIs or implementations,
real-time engine, backend API, CPAL, simulator, or CLI.

The plan may describe only accepted renderer intent, validated canonical
routing, explicit no-DSP state under the current schema, requested device
intent, bounded available capacities, explicit deferred capacities, and
non-observational metadata. It exposes only Aurora-owned values.

Renderer/DSP object construction, engine wiring, device discovery and
negotiation, stream opening, threads, processes, callbacks, and physical
measurement are outside the boundary. Prepared device values are requests, not
negotiated or observed facts.

All allocation and errors are setup-thread work. Future callback-facing state
must be separately preallocated and fixed-capacity. Setup errors never cross
into steady-state callback processing and must not reuse callback-fault or
physical-measurement terminology.

Typed deterministic equality is required. Serialization and plan fingerprinting
are deferred because no approved consumer or versioned canonical plan schema
exists. No hashing dependency or new diagnostics truth-source variant is
authorized.

## Options considered

### A. `aurora-runtime-plan`

Rejected. The name suggests passive storage and understates validated
derivation and invariant checking.

### B. `aurora-runtime-assembly`

Selected. It gives setup policy a reusable control-plane owner while keeping
configuration inert and the real-time engine focused.

### C. Module inside `aurora-config`

Rejected. It would introduce runtime policy into the accepted configuration
boundary and make future dependency growth harder to contain.

### D. Module inside `aurora-realtime-engine`

Rejected. It would couple hardware-independent preparation to engine concerns
and prevent clean future CLI or simulated-backend reuse.

## Consequences

- Configuration remains immutable and inert.
- The real-time engine, callbacks, renderer/DSP contracts, and device state
  machine remain unchanged.
- A small independent crate can be tested without CPAL or hardware.
- Current schema gaps, including maximum object count and DSP intent, remain
  explicit deferred values rather than hidden defaults.
- Any consumer integration or schema extension needs separate authorization.
- Phase 2 remains open and Phase 3C remains unauthorized.

## Implementation stop boundary

After this ADR and its specification merge, a new implementation branch may
implement only the isolated plan crate, deterministic builder, structured
errors, tests, and documentation. It may not add CLI, renderer factory,
engine/backend wiring, diagnostics producers, or later-milestone work.
