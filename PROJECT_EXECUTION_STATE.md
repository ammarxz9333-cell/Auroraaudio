# Aurora Project Execution State

## Current state

Aurora is in **active product implementation** with two coordinated execution lanes.

### Lane A — immersive renderer and product evidence

- Active program: `Immersive Audio Product Implementation 1`
- Completed prerequisite: issue `#43`, Checkpoint A, merged through PR `#57` (`44e3df84cb2082f079e83622f8e950cc66da1b8a`)
- Active work item: issue `#44` — unified renderer evaluation and artifact runner
- Next work items: issue `#45`, then issue `#38`
- Default delivery unit: one reviewable implementation PR
- Governance mode: maintenance only

### Lane B — Galaxy S6 appliance and realtime-MCU enablement

- Integrated baseline: PR `#83`, merge commit `5aa97d10df9e8af28d5afea34c24b90bad6b47db`
- State: **host/software validated, physical acceptance still open**
- Landed scope includes S6 appliance bootstrap, live immersive ingest, managed source routing, realtime post-processing boundaries, target-neutral realtime-MCU transport/capture foundations, build/runtime packaging, and physical bring-up contracts.
- This lane does **not** claim physical S6 boot, eARC capture, USB timing, realtime-MCU target HAL, DAC/speaker output, thermal/latency performance, wireless operation, or live streaming-service JOC acceptance.
- Dedicated S6 maintenance, hardening, build, and physical-bring-up PRs may proceed without changing the renderer dependency order.

The S6 baseline is an implemented appliance integration layer, not evidence that the complete product is physically validated or that the immersive renderer roadmap is complete.

## Sources of truth

Use these documents for different questions:

- Renderer/product dependency order: `docs/roadmaps/immersive-wireless-audio-execution-roadmap.md`
- S6 component evidence and promotion state: `platform/s6/COMPONENT_STATUS.md`
- S6 physical bring-up gates: `platform/s6/FLASH_GATES.md` and `docs/AURORA_EARC_REALTIME_MCU_PHYSICAL_BRINGUP.md`
- Repository-wide current execution summary: this file

Historical governance documents remain records of earlier decisions. They do not override these active sources of truth.

## Renderer/product dependency chain

Completed prerequisite:

- `#43A` — geometric binaural stabilization and honest classification, merged through PR `#57`.

Current and upcoming sequence:

1. `#44` — add the unified renderer evaluation and artifact runner.
2. `#45` — add the capability registry and CLI.
3. `#38` — implement offline 3D loudspeaker rendering.
4. `#46` — integrate the SOFA/HRIR data backend.
5. implement true offline Aurora HRTF, then realtime HRTF.
6. activate IAMF decoding as a separate out-of-process integration.
7. harden the CamillaDSP external runtime adapter.
8. build the deterministic network simulator.
9. implement packetized IP audio.
10. add the Linux receiver and optional PipeWire backend.
11. add multiroom behavior.
12. compare against Steam Audio and Snapcast as external references.
13. perform minimum physical system validation and measurement-driven hardware selection.

The landed S6 appliance baseline does not satisfy or skip any renderer acceptance gate above.

## S6 appliance evidence boundary

PR `#83` consolidated the reviewed S6 appliance stack into `main-v2`. Current evidence is intentionally split by class:

- **HOST-PASS**: deterministic host compilation/tests for protocol, source management, final source gating, live ingest, portable realtime-MCU transport/capture logic, and associated control/measurement boundaries.
- **BUILD-SCRIPT / STAGED**: reproducible S6 rootfs/kernel/image/package logic and staged external runtime components exist, but generated artifacts are not physically accepted yet.
- **HW-BLOCKED**: physical SM-G920F boot/display/touch/Wi-Fi, eARC carrier capture, USB host/HS-PHY behavior, target TDM/DMA, DAC/amplifier output, live JOC-to-7.1.4 acceptance, and sustained thermal/xrun validation.

No HOST-PASS or CI result may be described as physical measurement or production readiness.

## Required contributor behavior

Contributors and coding agents must:

1. work on one reviewable implementation slice per PR;
2. produce code, deterministic tests, measurable artifacts, and reproducible commands where the change affects executable behavior;
3. run and report validation for the exact commit;
4. distinguish placeholder, experimental, accepted, host-validated, physically validated, and production-ready states;
5. keep simulated evidence separate from physical measurements;
6. keep third-party engines optional and behind Aurora-owned interfaces;
7. document dataset, patent, and redistribution boundaries;
8. avoid adding planning-only architecture unless a concrete implementation blocker requires it;
9. never combine HRTF, IAMF, networking, receiver, and multiroom work in one PR;
10. keep S6 appliance/hardware work separate from renderer checkpoint/evidence PRs unless an issue explicitly requires cross-lane integration.

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

The accepted `GeometricBinaural` Checkpoint A baseline uses geometric ITD, geometric
ILD, and per-ear geometric distance weighting followed by power normalization.
It is not a true HRTF renderer and has no HRIR data, convolution, pinna cues, or
elevation cues. It must not be described
as Dolby Atmos-like, elevation-capable, or front/back accurate without
supporting evidence.

Likewise, the landed S6 appliance stack must not be described as flash-ready, plug-and-play, production-ready, physically validated, or live streaming Atmos/JOC validated while the corresponding hardware gates remain open.

A capability is complete only when its required code, tests, artifacts, reproducible commands, limitations, licensing information, and acceptance evidence exist.
