# Aurora architecture

Aurora is a hardware-agnostic immersive-audio software stack. The architecture is organized around stable software boundaries rather than a particular appliance.

## Layers

### 1. Core model

`aurora-core` defines channel roles, layouts, capability truth, and shared low-level types. `aurora-scene` defines listener, speaker, source, and trajectory state.

These crates must stay free of device- or operating-system-specific assumptions.

### 2. Decoder boundary

`aurora-decoder-api` defines the contract for turning an encoded or structured input into Aurora-consumable channel/object information. Decoder implementations may be in-process or external, but proprietary or legally sensitive behavior is not reimplemented in the core.

A decoder mode must state whether it provides real object metadata, channel PCM, or a synthetic upmix. Those modes are never interchangeable by silent fallback.

### 3. Rendering

`aurora-renderer-api` owns the renderer contract. Implementations include the basic renderer, VBAP work, and isolated external adapters. Renderers consume Aurora scene/audio data and write into caller-owned buffers.

### 4. DSP

`aurora-dsp-api` and `aurora-dsp-basic` provide post-render processing such as gain/delay/calibration/output shaping. External DSP engines remain adapters.

### 5. Realtime engine

The realtime crates own scheduling, bounded queues, device state, asynchronous resampling, drift control, latency accounting, and fault recovery. They are transport-independent and do not assume USB, eARC, TDM, or any other physical link.

### 6. Audio I/O

`aurora-audio-io` and the realtime audio API provide generic host-side input/output abstractions. CPAL is the current portable implementation; simulation backends provide deterministic test devices.

### 7. Runtime/configuration/diagnostics

`aurora-config`, `aurora-runtime-assembly`, `aurora-runtime-inspection`, and `aurora-diagnostics` turn configuration into a deterministic runtime plan and observable state.

### 8. Validation

`validation/` contains end-to-end software gates. These gates may use pinned external projects and real encoded fixtures, but they do not define or require a physical Aurora device.

## Data-flow principle

```text
input adapter -> decoder boundary -> Aurora scene/audio model -> renderer -> DSP -> generic audio output
```

Realtime timing and diagnostics wrap the data path without owning a particular hardware transport.

## Integration rule

A future hardware implementation may provide an input/output adapter, but it cannot become the canonical Aurora architecture. Replacing that adapter must not require changes to the core model, renderer API, DSP API, or realtime engine.
