# Aurora Project Execution State

## Current state

Aurora is in **active product implementation**.

- Active program: `Immersive Audio Product Implementation 1`
- Active work item: issue `#43`, Checkpoint A
- Next work items: issues `#44`, `#48`, `#45`, then `#38`
- Default development mode: implementation PRs
- Governance mode: maintenance only

## Source of truth

The single authoritative execution sequence is:

`docs/roadmaps/immersive-wireless-audio-execution-roadmap.md`

Historical governance documents remain records of earlier decisions. They do not override this file or select the next implementation task.

## Current dependency chain

1. `#43A` — stabilize and honestly classify geometric binaural.
2. `#44` — add the unified evaluation, benchmark, artifact, and memory-evidence runner.
3. `#48` — add Symphonia-based offline audio ingestion for WAV, FLAC, and Ogg/Vorbis.
4. `#45` — add the capability registry and CLI.
5. `#38` — implement offline 3D loudspeaker rendering.
6. `#46` — integrate the SOFA/HRIR data backend under the Aurora FFI policy.
7. implement true offline Aurora HRTF, then realtime HRTF.
8. activate IAMF decoding as a separate out-of-process integration.
9. harden the CamillaDSP external runtime adapter.
10. build the deterministic network simulator.
11. implement packetized IP audio using Tokio outside realtime boundaries.
12. add the Linux receiver and optional PipeWire backend.
13. add multiroom behavior.
14. compare against Steam Audio and Snapcast as external references.
15. perform minimum physical validation and select hardware from measurements.

Issue `#48` may proceed alongside `#45` after `#44` is accepted, but each must use a separate PR.

## Required contributor behavior

Contributors and coding agents must:

1. work on one reviewable implementation slice per PR;
2. produce code, deterministic tests, measurable artifacts, and reproducible commands;
3. run and report validation for the exact commit;
4. distinguish placeholder, experimental, accepted, and production-ready states;
5. keep simulated evidence separate from physical measurements;
6. keep third-party engines optional and behind Aurora-owned interfaces;
7. document dataset, patent, codec-feature, FFI, and redistribution boundaries;
8. keep unsafe/native code inside small reviewed adapter crates;
9. keep decoding, async runtimes, locks, filesystem access, and heavy logging outside audio callbacks;
10. avoid adding planning-only architecture unless a concrete implementation blocker requires it;
11. never combine HRTF, IAMF, networking, receiver, and multiroom work in one PR.

## Rust foundation policy

- `Symphonia`: preferred offline and streaming-file ingestion backend, behind Aurora-owned PCM types; no decoding inside realtime callbacks.
- `CPAL`: retained as the cross-platform realtime audio backend.
- `Tokio`: control plane, discovery, socket orchestration, reconnect logic, and background services only.
- `mio`: benchmark candidate only if Tokio fails measured latency or scheduling requirements.
- `Criterion`: required microbenchmark foundation with machine-readable regression evidence.
- `Bytehound` or equivalent: documented external Linux memory-profiling workflow.
- `Bencher`: optional only after Aurora's benchmark artifact schema stabilizes.
- `tracing`: structured telemetry, with formatting and output outside realtime code.

## Integration policy

Aurora-owned components remain responsible for scene representation, PCM contracts, rendering policy, evaluation, timing, transport, receiver behavior, and product orchestration.

Preferred third-party roles:

- `libmysofa`: optional SOFA/HRIR dataset backend through narrow reviewed FFI;
- `iamf-tools` or `libiamf`: isolated open immersive decode process;
- CamillaDSP: optional external DSP backend;
- PipeWire: optional advanced Linux audio backend;
- Steam Audio: external HRTF and room-simulation comparison;
- Snapcast: external multiroom synchronization comparison.

Cavern, truehdd, Resonance Audio, Rodio, and PortAudio are not active first-release dependencies.

## Capability honesty

The landed geometric binaural implementation is not true HRTF and must not be described as Dolby Atmos-like, elevation-capable, or front/back accurate without supporting evidence.

A capability is complete only when code, tests, artifacts, reproducible commands, limitations, licensing information, and CI evidence exist.