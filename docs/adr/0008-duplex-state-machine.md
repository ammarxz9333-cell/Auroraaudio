# ADR 0008: Explicit Duplex Device State Machine

- Status: Accepted
- Date: 2026-07-16

Use `Stopped`, `Starting`, `Running`, `Degraded`, `Faulted`, `Recovering`, and
`Stopping` as Aurora-owned control states. Callbacks publish numeric faults only;
the control thread stops both streams and optionally reopens the exact selectors.

Automatic fallback to another device is prohibited. CPAL does not expose the
Windows endpoint GUID, so name-derived selectors can change or collide. Exact
selectors win, fuzzy matches must be unique, and numeric indices remain temporary.

