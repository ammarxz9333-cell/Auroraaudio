# ADR 0012: Control-Thread Diagnostics With Atomic Callback Metrics

- Status: Accepted for Diagnostics & Telemetry Framework 1
- Date: 2026-07-17

## Decision

Keep structured diagnostic events, retained logs, serialization, snapshots,
reports, formatting, locking, and output entirely on control threads. Code that
may be called from an audio callback may only update a fixed set of numeric
atomics and may expose a control-thread snapshot operation.

Use logical timestamps for deterministic evidence and explicitly supplied
monotonic nanoseconds for host observations. Use ordered collections for every
serialized map or set. Bound retained events and failure evidence at
construction time.

## Consequences

- Audio callbacks do not allocate, block, format, log, or perform I/O.
- Existing callback, renderer, DSP, state, fault, and device contracts remain
  unchanged.
- Structured payloads can allocate on the control thread.
- Wall-clock timestamps are intentionally excluded from deterministic output.
- Integration points must translate existing numeric snapshots on the control
  thread rather than importing diagnostics types into protected APIs.
