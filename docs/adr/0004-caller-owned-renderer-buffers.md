# ADR 0004: Caller-Owned Renderer Buffers

## Status

Accepted.

## Context

The original `Renderer` trait stored owned objects and returned
`Vec<Vec<SpeakerGain>>`. The basic renderer also keyed smoothing history by
owned object and speaker strings. Every real-time block therefore allocated,
cloned identifiers, and performed hash-table work on the audio thread.

## Decision

Renderer configuration now declares a fixed maximum object count. A configured
renderer reports `RendererScratchSize`; the caller allocates `RendererScratch`
and a flattened `SpeakerGain` buffer before processing starts. `render_gains`
takes borrowed `RenderObject` values and writes object-major, speaker-minor
results into those buffers.

`SpeakerGain` identifies a speaker by configured index rather than an owned
identifier. The renderer keeps fixed index-based smoothing history. Buffer shape
errors use structured numeric fields and do not construct callback-time strings.

## Consequences

- Steady-state rendering performs no heap allocation or container resizing.
- Callers must retain scratch and output storage for the renderer lifetime.
- Implementations remain replaceable behind the Aurora-owned trait.
- The change is intentionally API-breaking at this pre-1.0 stage.
- Offline callers are slightly more explicit but use the same tested path as the
  real-time engine.
