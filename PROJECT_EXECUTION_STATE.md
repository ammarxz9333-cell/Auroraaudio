# Aurora Project Execution State

## Current state

Aurora is in **active product implementation**.

- Active program: `Immersive Audio Product Implementation 1`
- Active work item: issue `#43`, Checkpoint A
- Next work items: issues `#44`, `#45`, then `#38`
- Default development mode: implementation PRs
- Governance mode: maintenance only

## Source of truth

The single authoritative execution sequence is:

`docs/roadmaps/immersive-wireless-audio-execution-roadmap.md`

Historical governance documents remain records of earlier decisions. They do not override this file or select the next implementation task.

## Current dependency chain

1. `#43A` — stabilize and honestly classify geometric binaural.
2. `#44` — add the unified renderer evaluation and artifact runner.
3. `#45` — add the capability registry and CLI.
4. `#38` — implement offline 3D loudspeaker rendering.
5. `#46` — integrate the SOFA/HRIR data backend.
6. implement true offline Aurora HRTF, then realtime HRTF.
7. activate IAMF decoding as a separate out-of-process integration.
8. harden the CamillaDSP external runtime adapter.
9. build the deterministic network simulator.
10. implement packetized IP audio.
11. add the Linux receiver and optional PipeWire backend.
12. add multiroom behavior.
13. compare against Steam Audio and Snapcast as external references.
14. perform minimum physical validation and select hardware from measurements.

## Required contributor behavior

Contributors and coding agents must:

1. work on one reviewable implementation slice per PR;
2. produce code, deterministic tests, measurable artifacts, and reproducible commands;
3. run and report validation for the exact commit;
4. distinguish placeholder, experimental, accepted, and production-ready states;
5. keep simulated evidence separate from physical measurements;
6. keep third-party engines optional and behind Aurora-owned interfaces;
7. document dataset, patent, and redistribution boundaries;
8. avoid adding planning-only architecture unless a concrete implementation blocker requires it;
9. never combine HRTF, IAMF, networking, receiver, and multiroom work in one PR.

## Integration policy

Aurora-owned components remain responsible for scene representation, rendering policy, evaluation, timing, transport, receiver behavior, and product orchestration.

Preferred third-party roles:

- `libmysofa`: optional SOFA/HRIR dataset backend;
- `iamf-tools` or `libiamf`: isolated open immersive decode process;
- CamillaDSP: optional external DSP backend;
- PipeWire: optional advanced Linux audio backend;
- Steam Audio: external HRTF and room-simulation comparison;
- Snapcast: external multiroom synchronization comparison.

Cavern, truehdd, and Resonance Audio are not active first-release dependencies.

## Capability honesty

The landed `GeometricBinaural` implementation uses geometric ITD, geometric
ILD, and simple distance attenuation. It is not a true HRTF renderer and has no
HRIR data, convolution, pinna cues, or elevation cues. It must not be described
as Dolby Atmos-like, elevation-capable, or front/back accurate without
supporting evidence.

A capability is complete only when code, tests, artifacts, reproducible commands, limitations, licensing information, and CI evidence exist.
