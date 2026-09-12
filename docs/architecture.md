# Aurora architecture

Aurora is a hardware-agnostic immersive-audio software stack. The architecture is organized around stable software boundaries rather than a particular appliance.

## Layers

### 1. Core model

`aurora-core` defines channel roles, layouts, capability truth, and shared low-level types. `aurora-scene` defines listener, speaker, source, and trajectory state.

These crates must stay free of device-, provider-, plugin-, or operating-system-specific assumptions.

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

### 8. Application plugin boundary

`aurora-plugin-api` defines the stable application-plugin contract. Media libraries, Spotify/YouTube integrations, metadata/lyrics/artwork providers, URL resolvers, automation, and control surfaces run out of process behind a future Aurora Plugin Host.

Application plugins never execute on the realtime callback and never receive direct access to renderer internals, DSP internals, amplifier/MCU control, or raw realtime memory. They interact through versioned control/source APIs and explicit permissions.

The complete extension and upgrade policy is defined in `docs/plugin-architecture.md`.

### 9. Validation

`validation/` contains end-to-end software gates. These gates may use pinned external projects and real encoded fixtures, but they do not define or require a physical Aurora device.

## Data-flow principle

```text
application plugin -> Plugin Host -> source/control boundary
                                      |
input adapter -> decoder boundary -> Aurora scene/audio model -> renderer -> DSP -> generic audio output
```

Realtime timing and diagnostics wrap the audio data path without owning a particular hardware transport. Plugin IPC remains outside the realtime callback.

## Replaceability rule

Every major subsystem must depend on a narrow Aurora-owned contract instead of another implementation's internals. Upgrading one subsystem must not require unrelated layers to change unless the contract itself is intentionally versioned.

Examples:

- a media-service plugin can be replaced without rebuilding the renderer/DSP/realtime engine;
- a decoder backend can be replaced behind `aurora-decoder-api`;
- a renderer can be replaced behind `aurora-renderer-api`;
- a DSP implementation can be replaced behind `aurora-dsp-api`;
- a host, MCU, DAC, eARC receiver, or operating-system backend can be replaced through hardware/audio adapters;
- breaking contract changes require an explicit version transition and migration path, never an implicit repository-wide rewrite.

## Integration rule

A future hardware implementation or provider integration may provide an adapter/plugin, but it cannot become the canonical Aurora architecture. Replacing that adapter/plugin must not require changes to the core model, renderer API, DSP API, realtime engine, or unrelated plugins.
