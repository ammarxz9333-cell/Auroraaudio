# Aurora Component Inventory

This inventory classifies repository components by current product role. It is a structural control document, not a claim of product readiness.

## Status vocabulary

- **active-required**: built by the workspace and required by current accepted software paths.
- **active-incomplete**: built by the workspace and intentionally incomplete, but still part of the current architecture.
- **preserved-experimental**: source retained for possible future use; excluded from normal workspace builds and not a product capability.
- **planned**: required by the roadmap but not implemented.
- **hardware-gated**: implementation or validation requires physical hardware evidence.

## Domain and configuration

| Component | Status | Current responsibility | Prohibited interpretation |
| --- | --- | --- | --- |
| `aurora-core` | active-required | shared domain types and channel semantics | not application orchestration |
| `aurora-scene` | active-required | validated scene loading and trajectories | not runtime discovery |
| `aurora-config` | active-required | immutable validated configuration intent | not proof of device availability |

## Rendering

| Component | Status | Current responsibility | Known gap |
| --- | --- | --- | --- |
| `aurora-renderer-api` | active-required | Aurora-owned renderer boundary | no renderer implementation |
| `aurora-renderer-basic` | active-required | basic loudspeaker modes and geometric binaural baseline | geometric binaural is not HRTF; moving delay remains blockwise |
| `aurora-renderer-vbap` | active-incomplete | horizontal-plane VBAP implementation | not the complete 3D loudspeaker vertical slice |
| `aurora-renderer-cavern` | preserved-experimental | retained disabled adapter placeholder | not built, selected, or product-ready |
| SOFA/HRIR backend | planned | validated user-supplied HRIR ingestion | not implemented |
| Aurora HRTF renderer | planned | true binaural convolution and transition handling | not implemented |

## Decoding and ingestion

| Component | Status | Current responsibility | Known gap |
| --- | --- | --- | --- |
| `aurora-decoder-api` | active-incomplete | Aurora-owned decoder boundary | no production immersive decoder |
| `aurora-decoder-iamf` | active-incomplete | isolated open IAMF adapter candidate | not production-ready |
| `aurora-decoder-truehdd` | preserved-experimental | retained disabled offline placeholder | not built and not first-release scope |
| general media ingestion | planned | safe file ingestion behind PCM contracts | not implemented |

## DSP and audio I/O

| Component | Status | Current responsibility | Known gap |
| --- | --- | --- | --- |
| `aurora-dsp-api` | active-required | Aurora-owned DSP boundary | no algorithms by itself |
| `aurora-dsp-basic` | active-required | basic gain/filter/delay processing | dynamic-delay transition work remains |
| `aurora-dsp-camilladsp` | active-incomplete | optional external offline CamillaDSP adapter | runtime hardening and deployment review remain |
| `aurora-audio-io` | active-required | offline WAV I/O and semantic channel masks | not broad media ingestion |

## Realtime and simulation

| Component | Status | Current responsibility | Known gap |
| --- | --- | --- | --- |
| `aurora-realtime-audio-api` | active-required | backend-neutral realtime contracts | no backend behavior |
| `aurora-realtime-audio-cpal` | active-required | local CPAL backend | physical endpoint coverage incomplete |
| `aurora-realtime-audio-sim` | active-required | deterministic virtual audio backend | simulation is not physical evidence |
| `aurora-realtime-engine` | active-required | realtime block processing and duplex foundations | no complete network receiver pipeline |
| `aurora-simulation-assurance` | active-required | deterministic campaigns and reports | no room-physics replacement for hardware tests |
| synchronized packet transport | planned | bounded network audio transport | not implemented |
| Linux receiver node | planned | endpoint playback, recovery, and capability reporting | not implemented |
| multiroom orchestration | planned | grouping and synchronization policy | not implemented |

## Control plane and evidence

| Component | Status | Current responsibility | Known gap |
| --- | --- | --- | --- |
| `aurora-diagnostics` | active-required | bounded diagnostics and callback-safe counters | not product UI |
| `aurora-runtime-assembly` | active-incomplete | immutable runtime preparation descriptions | does not construct a running system |
| `aurora-runtime-inspection` | active-incomplete | bounded inspection projection | not readiness or materialization |
| `aurora-evaluation` | active-required | deterministic renderer evidence plus separated host observations | not physical audio-quality proof |
| `aurora-measurement` | active-incomplete | measurement boundary/scaffold | physical measurement remains hardware-gated |
| capability registry | planned | honest implemented/simulated/hardware-blocked reporting | not implemented |

## Application composition

| Component | Status | Current responsibility | Known gap |
| --- | --- | --- | --- |
| `aurora-cli` | active-required | developer-facing composition, commands, artifact placement | monolithic `main.rs` is under behavior-preserving decomposition |
| integrated product runtime | planned | configure, materialize, start, observe, stop | no complete vertical slice yet |
| installer/service/UI | planned | end-user deployment and operation | not implemented |

## Deletion policy

No component classified as `preserved-experimental`, `active-incomplete`, or `hardware-gated` may be deleted merely because it is not currently exercised. Deletion requires all of the following:

1. no active manifest, feature, code, test, fixture, workflow, or documentation dependency;
2. no unique contract or design knowledge needed by an active roadmap item;
3. a reviewed replacement or an explicit decision that the capability is out of scope;
4. full workspace, MSRV, cross-platform, documentation, and relevant behavior-equivalence validation;
5. an isolated PR whose purpose is the deletion.

Until those conditions are met, inactive source is preserved and clearly excluded rather than silently removed.
