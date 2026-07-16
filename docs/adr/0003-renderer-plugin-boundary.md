# ADR 0003: Renderer Plugin Boundary

## Decision

Depend on `aurora-renderer-api` traits rather than concrete renderer implementations.

## Rationale

This keeps basic geometric rendering, future VBAP/Ambisonics/IAMF renderers, and possible process-isolated adapters replaceable without changing scene, DSP, audio IO, or CLI code.

