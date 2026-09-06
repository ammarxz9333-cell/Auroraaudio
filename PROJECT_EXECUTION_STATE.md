# Aurora Project Execution State

## Current state

Aurora is in **active product implementation** with two coordinated execution lanes.

### Lane A — immersive renderer and product evidence

- Active program: `Immersive Audio Product Implementation 1`
- Completed prerequisite: issue `#43`, Checkpoint A, merged through PR `#57` (`44e3df84cb2082f079e83622f8e950cc66da1b8a`)
- Completed evaluation/performance foundation: issue `#44`, completed through PRs `#86`, `#87`, and final PR `#88` (`b1715f4315624de1862defb5691508b8992a470e`)
- Completed capability truth/reporting foundation: issue `#45`, completed through PR `#90` (`2c593e1d6b0b7e6732b35a7966ff4dd2e2d4ca52`)
- Active work item: issue `#38` — offline 3D loudspeaker rendering
- Active implementation slice: draft PR `#91`, `#38: add validated 3D VBAP renderer core`
- Next planned dependency after `#38`: issue `#46` — SOFA/HRIR data backend
- Default delivery unit: one reviewable implementation PR
- Governance mode: maintenance only

PR `#91` currently adds an experimental height-capable `Vbap3dRenderer` alongside the existing 2D implementation. Its reviewed scope includes validated non-degenerate loudspeaker triplets, a 3x3 VBAP solve, L2 spatial-energy normalization, LFE exclusion, deterministic outside-hull fallback, elevation routing, and canonical 5.1.2 tests. It is **not** issue `#38` completion yet, is still draft/open, and CI acceptance is still pending/failing on the current head. No production or physical capability is promoted by this work.

### Lane B — Galaxy S6 appliance and realtime-MCU enablement

- Integrated baseline: PR `#83`, merge commit `5aa97d10df9e8af28d5afea34c24b90bad6b47db`
- State: **host/software validated, physical acceptance still open**
- Adopted prototype host target: Samsung Galaxy S6 / Exynos 7420 / 3 GB RAM, with AuroraOS-S6 as a minimal ARM64 Linux appliance target rather than Android.
- UI target: reuse the Galaxy S6 display and touch hardware through a direct Linux UI path (LVGL/framebuffer/evdev class integration) once the physical kernel/display/touch bring-up gates are satisfied.
- Landed scope includes S6 appliance bootstrap, live immersive ingest, managed source routing, realtime post-processing boundaries, target-neutral realtime-MCU transport/capture foundations, build/runtime packaging, and physical bring-up contracts.
- This lane does **not** claim physical S6 boot, display/touch acceptance, eARC capture, USB timing, realtime-MCU target HAL, DAC/speaker output, thermal/latency performance, wireless operation, or live streaming-service JOC acceptance.
- Dedicated S6 maintenance, hardening, build, and physical-bring-up PRs may proceed without changing the renderer dependency order.

The S6 baseline is an implemented appliance integration layer plus an adopted prototype target, not evidence that the complete product is physically validated or that the immersive renderer roadmap is complete. In particular, sustained realtime Harletty + Omniphony 7.1.4 + Aurora DSP + USB operation on the Galaxy S6 remains an **open physical/performance validation gate** and must not be treated as proven.

## Current architecture direction

Aurora keeps ownership of the product graph and control plane rather than delegating the whole product to one external engine. The current direction is an AudioReach-style modular architecture with:

- Aurora-owned graph/module orchestration and capability registry;
- an immersive decoder stage that may use Harletty behind an Aurora-owned boundary where legally and technically applicable;
- Omniphony as a renderer integration target behind an Aurora-owned rendering boundary rather than as the product control plane;
- an Aurora calibration database and measurement/evidence layer;
- swappable hardware/audio endpoints so S6, realtime-MCU, Linux, and later hardware targets remain replaceable without rewriting the product graph.

Third-party engines must remain replaceable integrations behind Aurora-owned interfaces. Their presence does not upgrade Aurora capability status unless the corresponding code, tests, reproducible evidence, and acceptance gates exist.

## Sources of truth

Use these documents for different questions:

- Renderer/product dependency order: `docs/roadmaps/immersive-wireless-audio-execution-roadmap.md`
- S6 component evidence and promotion state: `platform/s6/COMPONENT_STATUS.md`
- S6 physical bring-up gates: `platform/s6/FLASH_GATES.md` and `docs/AURORA_EARC_REALTIME_MCU_PHYSICAL_BRINGUP.md`
- Capability truth: the typed capability registry exposed through `aurora-cli capabilities` and `aurora-cli capabilities --json`
- Generated capability documentation: the README capability table gated against registry truth in CI
- Repository-wide current execution summary: this file

Historical governance documents remain records of earlier decisions. They do not override these active sources of truth.

## Renderer/product dependency chain

Completed foundations:

- `#43A` — geometric binaural stabilization and honest classification, merged through PR `#57`.
- `#44` — unified renderer evaluation, stable evidence artifacts, Criterion regression policy, convolution benchmark coverage, and bounded-memory/performance documentation, completed through PR `#88`.
- `#45` — typed/versioned capability registry, shared presentation, generated README capability table, CI truth gate, and public CLI human/JSON capability output, completed through PR `#90`.

Current and upcoming sequence:

1. `#38` — implement and accept offline 3D loudspeaker rendering. Current first slice is draft PR `#91`.
2. `#46` — integrate the SOFA/HRIR data backend.
3. implement true offline Aurora HRTF, then realtime HRTF.
4. activate IAMF decoding as a separate out-of-process integration.
5. harden the CamillaDSP external runtime adapter.
6. build the deterministic network simulator.
7. implement packetized IP audio.
8. add the Linux receiver and optional PipeWire backend.
9. add multiroom behavior.
10. compare against Steam Audio and Snapcast as external references.
11. perform minimum physical system validation and measurement-driven hardware selection.

The landed S6 appliance baseline does not satisfy or skip any renderer acceptance gate above.

## Evidence foundation now merged

Issue `#44` established the software evidence foundation used by later renderer work. Accepted scope includes deterministic evaluation artifacts, versioned schemas, explicit correctness/discontinuity gates, Criterion benchmarks, an absolute regression policy, CI evidence artifacts, bounded-memory documentation, and deterministic convolution coverage.

Representative accepted software-only CI measurements from the final `#44` slice include approximately:

- renderer 12-channel path: `~0.23 us`;
- adaptive ASRC 12-channel path: `~60.5 us`;
- contiguous transport 12-channel / 256-frame round trip: `~11.3 us`;
- deterministic 128-tap convolution at 12-channel / 256-frame: `~1.11 ms`, about `20.9%` of a 48 kHz / 256-frame block budget.

These are GitHub-hosted software measurements, not Galaxy S6 measurements, acoustic measurements, end-to-end latency, or physical realtime acceptance.

Issue `#45` established capability truth as code rather than prose. Capability state is now typed, versioned, queryable, exposed through `aurora-cli capabilities` and `aurora-cli capabilities --json`, rendered into the README from the same registry, and checked in CI for documentation drift.

## S6 appliance evidence boundary

PR `#83` consolidated the reviewed S6 appliance stack into `main-v2`. Current evidence is intentionally split by class:

- **HOST-PASS**: deterministic host compilation/tests for protocol, source management, final source gating, live ingest, portable realtime-MCU transport/capture logic, and associated control/measurement boundaries.
- **BUILD-SCRIPT / STAGED**: reproducible S6 rootfs/kernel/image/package logic and staged external runtime components exist, but generated artifacts are not physically accepted yet.
- **HW-BLOCKED**: physical SM-G920F boot/display/touch/Wi-Fi, eARC carrier capture, USB host/HS-PHY behavior, target TDM/DMA, DAC/amplifier output, live JOC-to-7.1.4 acceptance, and sustained thermal/xrun validation.

No HOST-PASS or CI result may be described as physical measurement or production readiness.

## CI state and maintenance

After the `#44` / `#45` merges, CI maintenance was hardened on `main-v2` to reduce duplicate/stale work without weakening the validation policy. The latest maintenance commits before this execution-state refresh include:

- `dba74655ecf25eed13d2ab121dcdcda92d23c87d` — dependency-aware/cancel-stale CI hardening, bounded runtime, and bounded artifact retention;
- `53505c0316eea2067f97c30bae232aef42e73be6` — removal of duplicate push validation.

This is infrastructure maintenance only and does not promote any audio capability.

## Required contributor behavior

Contributors and coding agents must:

1. work on one reviewable implementation slice per PR;
2. produce code, deterministic tests, measurable artifacts, and reproducible commands where the change affects executable behavior;
3. run and report validation for the exact commit;
4. distinguish placeholder, experimental, accepted, host-validated, physically validated, and production-ready states;
5. keep simulated evidence separate from physical measurements;
6. keep third-party engines optional and behind Aurora-owned interfaces;
7. document dataset, patent, licensing, and redistribution boundaries;
8. avoid adding planning-only architecture unless a concrete implementation blocker requires it;
9. never combine HRTF, IAMF, networking, receiver, and multiroom work in one PR;
10. keep S6 appliance/hardware work separate from renderer checkpoint/evidence PRs unless an issue explicitly requires cross-lane integration;
11. never present CI success as proof that Galaxy S6, eARC, USB, realtime-MCU, DAC, thermal, or live immersive-streaming gates have passed.

## Integration policy

Aurora-owned components remain responsible for scene representation, graph/module orchestration, rendering policy, capability truth, evaluation, timing, transport, receiver behavior, calibration evidence, and product orchestration.

Preferred third-party roles:

- Harletty: optional immersive decoder integration behind an Aurora-owned boundary where applicable;
- Omniphony: renderer integration target behind an Aurora-owned boundary;
- `libmysofa`: optional SOFA/HRIR dataset backend;
- `iamf-tools` or `libiamf`: isolated open immersive decode process;
- CamillaDSP: optional external DSP backend;
- PipeWire: optional advanced Linux audio backend;
- Steam Audio: external HRTF and room-simulation comparison;
- Snapcast: external multiroom synchronization comparison.

Cavern, truehdd, and Resonance Audio are not active first-release dependencies.

## Capability honesty

The accepted `GeometricBinaural` Checkpoint A baseline uses geometric ITD, geometric ILD, and per-ear geometric distance weighting followed by power normalization. It is not a true HRTF renderer and has no HRIR data, convolution, pinna cues, or validated elevation cues. It must not be described as Dolby Atmos-like, elevation-capable, or front/back accurate without supporting evidence.

The draft `Vbap3dRenderer` work in PR `#91` is a software implementation slice with deterministic tests. Until issue `#38` acceptance is complete, it must not be described as a completed production 3D renderer.

Likewise, the landed S6 appliance stack and adopted Galaxy S6 target must not be described as flash-ready, plug-and-play, production-ready, physically validated, or live streaming Atmos/JOC validated while the corresponding hardware and performance gates remain open.

A capability is complete only when its required code, tests, artifacts, reproducible commands, limitations, licensing information, and acceptance evidence exist.
