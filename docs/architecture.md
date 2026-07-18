# Aurora Architecture

Aurora is a Rust spatial-audio platform organized around explicit realtime boundaries, Aurora-owned interfaces, deterministic offline evaluation, and hardware-gated physical claims.

This document describes the current active architecture. Historical milestones, superseded adapters, and acceptance records belong in `docs/acceptance/`, `docs/adr/`, and Git history; they must not redefine the active product structure.

## Architectural principles

1. Audio callbacks perform bounded numeric work only.
2. Renderer, DSP, backend, configuration, evaluation, and diagnostics responsibilities remain separated.
3. Control-plane code may allocate, serialize, inspect, and perform filesystem or process work; callback code may not.
4. Simulation evidence, host observations, and physical measurements are distinct evidence classes.
5. A crate exists only when it owns a durable production responsibility. Temporary milestones do not justify permanent layers.
6. The active product path takes precedence over superseded experiments and inactive adapters.

## Active workspace layers

### Domain

- `aurora-core`: shared numeric and audio-domain values.
- `aurora-scene`: scene loading, validation, and trajectory sampling.
- `aurora-config`: immutable versioned product intent and preset materialization.

Domain crates must not depend on renderer implementations, DSP implementations, backends, CLI, evaluation, or diagnostics.

### Rendering

- `aurora-renderer-api`: Aurora-owned renderer contract.
- `aurora-renderer-basic`: basic deterministic renderers, including `GeometricBinaural`.
- `aurora-renderer-vbap`: horizontal VBAP implementation.

`GeometricBinaural` is a geometric ITD/ILD baseline. It is not HRTF and provides no HRIR convolution, pinna model, reliable elevation cues, or individualized listener model.

Renderer implementations may depend on domain and renderer-interface crates. They must not depend on the CLI, backend implementations, evaluation, diagnostics formatting, or filesystem code.

### DSP

- `aurora-dsp-api`: Aurora-owned DSP contract.
- `aurora-dsp-basic`: in-process DSP primitives, including fractional delay.
- `aurora-dsp-camilladsp`: isolated control-thread adapter for optional offline external processing.

DSP processing follows rendering. External-process control is never callback work.

### Audio I/O and realtime

- `aurora-audio-io`: offline WAV ingestion and multichannel WAV output.
- `aurora-realtime-audio-api`: backend-neutral stream and device contracts.
- `aurora-realtime-audio-cpal`: CPAL backend adapter.
- `aurora-realtime-audio-sim`: deterministic virtual audio backend.
- `aurora-realtime-engine`: preallocated realtime block pipeline and drift-control integration.

CPAL and other backend-native types remain inside backend adapter crates. The realtime engine consumes Aurora-owned contracts only.

### Control plane and evidence

- `aurora-diagnostics`: bounded control-thread diagnostics and atomic callback metrics.
- `aurora-runtime-assembly`: deterministic preparation descriptions derived from validated configuration.
- `aurora-runtime-inspection`: read-only projection of prepared descriptions.
- `aurora-evaluation`: offline renderer evidence and host-observation collection.
- `aurora-simulation-assurance`: deterministic stress and replay campaigns.
- `aurora-measurement`: reserved physical-measurement boundary; no physical capability is claimed until hardware gates pass.

These crates must not be mistaken for an operating runtime. Preparation and inspection descriptions are data; they do not construct streams, renderers, DSP processors, or devices.

### Application

- `aurora-cli`: developer-facing application composition and commands.

The CLI may depend on implementation crates. No production crate may depend on the CLI.

## Canonical data flows

### Offline rendering

```text
validated scene + decoded mono PCM
        -> configured renderer
        -> caller-owned gains and scratch
        -> DSP processing
        -> multichannel PCM/WAV
```

### Realtime output

```text
backend callback
        -> preallocated realtime engine
        -> renderer
        -> DSP
        -> backend output buffer
```

### Evaluation

```text
canonical fixture + scene + PCM
        -> configured renderer
        -> deterministic evidence
        -> optional host observations
        -> bounded artifacts
```

Host timing never changes deterministic correctness status. Missing required evidence cannot be reported as `pass`.

## Callback contract

Callback-reachable code must not perform:

- heap allocation or reallocation;
- filesystem or process access;
- logging, formatting, JSON, or string construction;
- blocking locks or sleeps;
- device enumeration or configuration rebuilding;
- unbounded loops or collections.

Control-thread configuration creates all renderer, DSP, ring, scratch, and block capacities before stream start. Runtime state changes require a separately reviewed bounded handoff design.

## Dependency direction

The intended direction is:

```text
core / scene / config
        -> APIs
        -> implementations
        -> realtime engine or offline orchestration
        -> CLI

evaluation and diagnostics consume public boundaries;
production processing crates never depend back on them.
```

Reverse dependencies from domain or processing crates into CLI, evaluation, report formatting, or backend-native crates are architectural defects.

## Active versus legacy code

The active workspace excludes the superseded `aurora-renderer-cavern` and `aurora-decoder-truehdd` experiments. Their directories may remain temporarily for historical extraction, but they are not built, tested, advertised, or eligible for product integration. They should be deleted once any still-useful design notes are migrated to neutral documentation.

The IAMF crate is an isolated placeholder boundary until separately authorized decoder integration exists. It must not advertise production decoding capability.

## Known structural gaps

The following are intentional product gaps, not completed capabilities:

- block-continuous moving-source delay interpolation;
- true SOFA/HRIR-backed HRTF rendering;
- complete offline 3D loudspeaker vertical slice;
- construction from prepared runtime descriptions into real objects;
- packetized network audio and receiver synchronization;
- physical multichannel routing and latency validation.

New work should close these gaps rather than add another passive governance or inspection layer.

## Change policy

A structural refactor is justified when it removes duplicated ownership, reverse dependencies, inactive product paths, or monolithic application responsibilities. Renaming or moving stable code only for appearance is not sufficient.

Each structural change must:

1. state the ownership problem;
2. preserve or deliberately version public behavior;
3. add dependency or contract tests where applicable;
4. pass Linux, Windows, and MSRV validation;
5. avoid combining unrelated DSP behavior changes with file movement.
