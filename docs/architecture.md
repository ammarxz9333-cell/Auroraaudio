# Architecture

Aurora is a format-independent spatial-audio system with implemented offline
rendering, basic DSP, backend-neutral real-time contracts, deterministic
simulation, and accepted diagnostics and configuration control planes. Physical
hardware validation remains incomplete. Proprietary codec decoding and network
speaker transport are out of scope.

## Workspace Layout

- `aurora-core`: Shared data model for formats, vectors, speakers, listeners, objects, audio blocks, and scenes.
- `aurora-renderer-api`: Stable renderer trait and renderer-facing errors.
- `aurora-renderer-basic`: Deterministic basic geometric modes, including the
  `GeometricBinaural` two-channel baseline with geometric ITD, geometric ILD,
  and per-ear geometric distance weighting followed by power normalization. It
  is not an HRTF renderer and uses no HRIR data, convolution, pinna cues, or
  elevation cues.
- `aurora-renderer-vbap`: Optional deterministic horizontal-plane VBAP renderer implementing the existing renderer boundary.
- `aurora-dsp-api`: Aurora-owned DSP processing boundary.
- `aurora-dsp-basic`: Implemented basic offline DSP, including fractional per-channel delay.
- `aurora-audio-io`: Implemented offline PCM/float WAV reading and multichannel WAVE_FORMAT_EXTENSIBLE output.
- `aurora-realtime-audio-api`: Backend-neutral local real-time audio traits.
- `aurora-realtime-audio-cpal`: First local audio backend, isolated behind Aurora-owned API types.
- `aurora-realtime-audio-sim`: Deterministic virtual backend and accelerated validation environment.
- `aurora-diagnostics`: Hardware-independent structured events, bounded control-thread logs, atomic callback metrics, snapshots, and reports.
- `aurora-config`: Immutable versioned configuration intent, deterministic presets, bounded migration, and redacted snapshots.
- `aurora-runtime-assembly`: Immutable runtime preparation contracts plus deterministic Checkpoint B derivation from validated configuration; no runtime construction or integration.
- `aurora-runtime-inspection`: leaf crate containing a bounded, versioned, redacted-by-default projection plus deterministic JSON/text formatting of inspection-owned facts.
- `aurora-runtime-materialization`: proposed near-leaf owner for bounded passive runtime-resource requirement contracts; it does not exist until separately reviewed Checkpoint A implementation.
- `aurora-realtime-engine`: Real-time block pipeline preserving renderer, channel-role, geometric-delay, and DSP boundaries.
- `aurora-measurement`: Synthetic-measurement scope scaffold; implemented synthetic latency and routing evidence lives in the simulator and real-time engine, and no accepted physical measurement capability exists.
- `aurora-scene`: JSON scene loading, validation, and trajectory sampling.
- `aurora-cli`: Developer CLI for offline simulation and inspection.

## Data Flow

```text
Scene + AudioFormat
        |
        v
Renderer API -> caller-owned gains/scratch -> renderer implementation
        |
        v
DSP API -> DSP implementation
        |
        v
Audio IO
```

Real-time output replaces offline Audio IO with a backend callback:

```text
Audio backend callback -> real-time engine -> renderer -> DSP -> output backend
```

Milestone 0B adds offline mono WAV input, block-based rendering, and multichannel WAV output. The visualizer remains intentionally deferred.

Milestone 0C adds explicit channel roles, canonical standard layout ordering, WAVE_FORMAT_EXTENSIBLE channel masks, and optional offline per-channel geometric delay processing.

Milestone 0D adds adapter crates for CamillaDSP, IAMF/libiamf, truehdd, and Cavern. These are boundary crates only: no third-party source is copied into Aurora, no adapter is required by `aurora-core`, and all real third-party integration points remain disabled by default behind adapter-specific Cargo features.

The IAMF adapter is a non-production placeholder for a preferred open decoder
path. The truehdd adapter is experimental, offline-only, and non-production.
The Cavern adapter is disabled by default and policy-limited pending license
review. None of these boundaries proves codec or renderer availability.

Milestone 0F adds a local real-time audio path using Aurora-owned traits and a CPAL-backed local backend. No HDMI/eARC, network audio, wireless speaker transport, GUI, or proprietary codec integration is included.

## Renderer Boundary

Renderers implement the `Renderer` trait from `aurora-renderer-api`. Configuration allocates fixed layout, object-history, output, and scratch capacity. Steady-state `render_gains` borrows compact numeric objects and writes flattened object-major, speaker-minor results into caller-owned buffers. Speaker identity is represented by configured index on the processing path. See ADR 0004.

Codec decoding is not part of the renderer boundary.

## DSP Boundary

DSP engines implement a separate trait and operate after rendering. The CamillaDSP adapter remains isolated as an external process controlled by generated configuration and offline WAV file input/output.

## WAV Channel Masks

Aurora keeps `hound` for WAV reading. `hound` can write WAVE_FORMAT_EXTENSIBLE, but its writer derives the channel mask from the channel count rather than from semantic channel roles. That is insufficient for standard surround masks such as 5.1, 7.1, and 5.1.2, so `aurora-audio-io` writes a minimal WAVE_FORMAT_EXTENSIBLE float header itself for rendered outputs.

Stereo, 5.1, and 7.1 use the standard Windows speaker-position bits. Aurora also writes a role-derived 5.1.2 mask using FL, FR, FC, LFE, SL, SR, TFL, and TFR bits, but WAVE_FORMAT_EXTENSIBLE does not fully describe object-based or up-firing speaker intent. Aurora therefore treats 5.1.2 WAV masks as channel-position metadata only; room geometry and up-firing semantics remain in the scene fixture.

## Real-Time Audio

The real-time callback exclusively owns the preallocated block engine and publishes numeric metrics through atomics. Control-thread status printing, device enumeration, configuration rebuilds, and speaker-identification confirmation stay outside the callback. The callback contains no Aurora logging, blocking locks, filesystem/process access, or steady-state allocation. See `docs/realtime-audio.md`, `docs/threading-model.md`, `docs/realtime-allocation-audit.md`, and `docs/device-support.md`.

The hardware-independent duplex boundary uses a fixed-capacity contiguous SPSC
frame ring between independently scheduled input and output callbacks. Drift
observability and the Aurora-owned `DriftCompensator` strategy live in
`aurora-realtime-engine`; CPAL types remain confined to the backend crate. See
`docs/backend-timing.md`, `docs/duplex-audio.md`, and ADR 0005.

The live duplex path places an Aurora-owned adaptive-resampler contract between
the selected SPSC ring and `RealTimeEngine`. Rubato is private implementation
detail. A PI controller adjusts one multichannel-coherent ratio; CPAL input and
output streams remain independent. Device lifecycle is controlled by the state
machine in ADR 0008.

Simulation Sprint 1 adds an independent virtual backend with integer-tick input
and output clock domains. It exercises format negotiation, callback scheduling,
ring-fill drift control, deterministic recovery, virtual-loopback truth, and
canonical routing without importing simulator types into shared domain APIs.

Diagnostics & Telemetry Framework 1 adds a separate control-plane crate. It
does not depend on a renderer, DSP implementation, device backend, engine, or
CLI. Protected audio APIs remain unchanged. Callback-reachable producers may
update only fixed `AtomicU64` counters; event construction and all output remain
control-thread responsibilities. See `docs/diagnostics.md` and ADR 0012.

Configuration & Preset System 1 adds another independent control-plane crate.
Raw configuration is inert until wrapped by `ValidatedConfiguration`;
serialization, preset materialization, migration, and redaction remain outside
audio callbacks. Device values are selection intent only and never discovery or
negotiation evidence. See `docs/configuration.md` and ADR 0013.

Runtime Assembly Contracts 1 Checkpoint B adds deterministic, fallible
derivation from `&ValidatedConfiguration` to the immutable Checkpoint A model.
The crate still depends directly only on `aurora-config` and `aurora-core`.
Derivation preserves normalized routing and speaker order, inactive state,
unresolved device-selection intent, explicit no-DSP state, and validated
renderer intent. Speaker azimuth becomes a dimensionless horizontal unit
direction; it is not a room coordinate, distance, or physical measurement.
The crate does not resolve devices, construct renderers or DSP, allocate runtime
storage, connect an engine, or execute callbacks. Setup-derived capacities
remain explicitly deferred. See ADR 0014 and
`docs/planning/runtime-assembly-contracts-1.md`.

ADR 0015 governs the merged immutable setup-planning description derived from
`PreparedRuntimePlan`. Checkpoint C describes canonical setup stages, acyclic
dependencies, unresolved device intent, requested format intent, and
renderer/DSP/backend setup intent inside `aurora-runtime-assembly`.
`SetupPlanComplete` means only that the description is complete; it is not
runtime, host, or physical readiness. Direct dependencies remain only
`aurora-config` and `aurora-core`; construction and integration remain
unauthorized. Checkpoint D's contract evidence and documentation are complete
and merged through PR `#26` at
`93f36464cac429bf7e25b257e894aab64a72e2d4`. Checkpoint E's separate final
architectural review accepted Runtime Assembly Contracts 1 on 2026-07-18. This
software-only result accepts immutable descriptive contracts and deterministic
derivation only. It does not assert runtime readiness, execute or construct a
renderer, DSP, backend, stream, callback, or engine, validate hardware, make a
physical claim, or authorize Phase 2, Phase 3C, or a later milestone. See
`docs/acceptance/runtime-assembly-contracts-1.md`.

Diagnostics & Telemetry Framework 1 and Configuration & Preset System 1 are
merged, accepted software-only control planes. Phase 2 remains open and
incomplete. Phase 3A and Phase 3B remain
`CONDITIONALLY_ACCEPTED_PENDING_HARDWARE`; no Phase 3C milestone has started.

ADR 0016 authorizes **Runtime Plan Inspection 1** as a software-only
control-plane boundary after its governance merge. A separate
`aurora-runtime-inspection` leaf crate may read the accepted public accessors on
prepared runtime and setup plans and produce its own versioned, bounded,
redacted-by-default inspection projection. The only permitted Aurora crate
dependency is `aurora-runtime-inspection --> aurora-runtime-assembly`.

The projection is not plan serialization, runtime readiness, diagnostics
producer wiring, or runtime construction. It adds no reverse dependency and no
renderer, DSP, engine, backend, simulator, CPAL, CLI, filesystem, environment,
host, or hardware dependency. Checkpoints A, B, and C are `COMPLETE`.
Checkpoint C's deterministic bounded JSON/text formatting and inspection-owned
conformance evidence merged through PR `#32` at
`c3bb059396185eed5155e1147eca5f35c8c15ac7`. Checkpoint D's final validation and
architectural evaluation accepted the software-only milestone on 2026-07-18.
No runtime, hardware, readiness, physical, or latency claim was accepted. See
`docs/runtime-plan-inspection.md` and
`docs/acceptance/runtime-plan-inspection-1.md`.

ADR 0017 proposes **Runtime Materialization Contracts 1** as the next
software-only control-plane boundary. After the governance change merges, a
new `aurora-runtime-materialization` crate may depend directly only on
`aurora-runtime-assembly` and describe aggregate future resource
responsibilities, capability requirements, explicit deferred requirements,
and canonical materialization-planning dependencies.

Materialization planning remains passive data derivation. It does not extend
accepted setup planning, change Runtime Plan Inspection 1, construct a
renderer, DSP processor, backend, stream, callback, or engine, access a host or
device, or claim runtime readiness. Checkpoints A-D remain `NOT_STARTED` in
this governance change. See ADR 0017 and
`docs/planning/runtime-materialization-contracts-1.md`.


## Shared cinema output processing

`aurora-dsp-basic::output::SpeakerPostProcessor` owns the 48 kHz canonical
7.1.4 interleaved output path. `aurora-s6-postprocess` uses this same unit after
Rubato ASRC; `aurora-self-test` exercises it without opening a device. There is
no second output-DSP crate or duplicate S6 filter implementation in this branch.

Order: per-channel LR4 crossover (80 Hz bed / 100 Hz height defaults), LFE
120 Hz low-pass plus bass redirection, 20 Hz sub high-pass, per-speaker PEQ /
trim / polarity / delay, lip-sync delay, smoothed master gain and linked peak
limiting. LFE trim defaults to 0 dB to avoid doubling an upstream playback gain.

`SpeakerCalibration` is a strict schema-1, 48 kHz configuration in Aurora
canonical order, with up to eight PEQ bands and 100 ms delay per channel.
Construction/configuration allocate; processing, reset and control setters do
not. Invalid partial interleaved blocks are silenced and return `OutputShapeError`.
Nonfinite input is sanitized before filters. Lip-sync changes crossfade over
240 frames; mute applies after the delay, so queued audio cannot defer mute.

Set `AURORA_SPEAKER_CALIBRATION` to a validated JSON path before launching the
S6 postprocessor. The flat example is `config/speaker-calibration-flat-v1.json`.
These are manually supplied corrections, not an automatic acoustic measurement.

WAVE_FORMAT_EXTENSIBLE export permutes samples into ascending speaker-mask order;
Aurora runtime and JSON trajectories retain canonical order. Specifically,
7.1.4 back channels precede side channels in the file but not in the runtime.

### Source DSP control and allocation audits

The source gate delivers validated absolute lip-sync frames through a dedicated
APC0 Unix datagram endpoint, separate from the AUR0 audio stream. HDMI uses
`AURORA_DSP_CONTROL_SOCKET`; the optional local adapter has its own endpoint.
The supported range is 0–24000 frames at 48 kHz. Active-source control is
replayed after DSP restart. The legacy broker pipe remains available only when
`AURORA_CONTROL_FD` is explicitly supplied and names a valid descriptor.

Tests share the dev-only `aurora-test-alloc` allocator. Windows uses a native
thread ID and atomic counters because Rust 1.78 GNU TLS initialization itself
allocates. Measurement setup is serialized outside the measured closure;
allocator callbacks never acquire that mutex. Other targets use thread-local
counters. Panic cleanup and positive allocation/reallocation controls protect
against false zero-allocation results.
