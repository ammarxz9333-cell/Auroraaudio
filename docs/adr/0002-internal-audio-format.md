# ADR 0002: Internal Audio Format

## Decision

Represent internal PCM audio as planar `f32` blocks with explicit frame count, presentation timestamp, and discontinuity flag.

## Rationale

Planar `f32` samples are straightforward for DSP, renderer math, offline analysis, and future SIMD optimization.

