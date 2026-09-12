# Aurora Agent Reference — Living Handoff

> Read this file first. It is Aurora's canonical compact handoff for coding agents. Every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update it.

Last updated: **2026-09-12**

## Repository / branch policy

- Long-lived source of truth: `main-v2`.
- Use at most one focused short-lived implementation branch at a time; squash/merge green work, then remove stale branches.
- Never leave temporary validation workflows or patch scripts on `main-v2`.
- If the user says **“كمل” / “continue”**, continue the first unfinished item below; do not redo project discovery.

Latest merged architecture slice: PR #131, squash `0f37b6df1587d429587b74eac714309dc8299d34`.
Latest merged validation slice: PR #133, squash `1445f0f29b9144dd2a958d6b57ff59285e439dec`.
Completed trackers include #118, #119, #130, #132.

## Product / safety invariants

Aurora is a hardware-agnostic, primarily Rust immersive-audio stack. Target direction: realtime 7.1.4-class audio expandable toward 11.1.4, replaceable decoder/renderer/DSP/audio-I/O/transport/hardware adapters, authorized TV/eARC ingestion, and later private wireless rear/multi-room transport.

Non-negotiable rules:
- no allocation, locks, logging/formatting, filesystem/process/config/registry work in the steady-state realtime callback;
- prepare -> validate -> commit; failed candidates do not mutate active state;
- unknown/incompatible IDs, schemas, capabilities, generations or contracts fail closed;
- object decode, channel decode and synthetic upmix are distinct;
- configured/simulated/estimated latency is never called measured latency;
- application plugins stay outside realtime memory and route through Aurora-owned source/control boundaries;
- no DRM circumvention, Widevine bypass, protected-media extraction or device-identity spoofing;
- no Dolby certification/proprietary-streaming claim from software CI;
- GPL/incompatible references remain external processes/reference implementations.

## Merged software truth

- Hardware-agnostic Rust workspace with Linux/Windows/MSRV CI.
- Deterministic simulation assurance and sustained accelerated realtime-health soak.
- Generic prepared renderer + realtime delay/DSP boundary in `RealTimeEngine`; Basic and VBAP use the same materialization boundary.
- Root renderer and input/output backend selection use versioned `ComponentReference`; config schema v3 has explicit deterministic v0/v1/v2 migration.
- Renderer/backend registries fail closed before activation; prepared identities include implementation/version/contract identity; runtime inspection schema v3 reports prepared control-plane intent.
- Software JOC/IEC61937 path is validated with pinned external Harletty/Omniphony; OpenJOC provides an independent external differential/fail-closed lane.
- PR #133 added a fail-closed temporal JOC harness. The public Harletty `joc_atmos_1s.eac3` passes codec/JOC admission but must fail moving-object proof as `insufficient_temporal_diversity`.
- Temporal evidence is separated into codec/JOC continuity, timed OAMD/object-state diversity, 12-channel rendered temporal-energy diversity, and pacing/health. OpenJOC `render-joc` is self-consistency evidence, not an authored-position oracle.

PR #131 final gates: CI `34693844433`, simulation smoke `34693844411`, realtime soak `34693844412` — PASS.
PR #133 final gates: CI `34694981490`, Immersive JOC `34694981484` — PASS.

Historical hardware proof remains limited to:
`Lindy 38368 / SiI9437 eARC -> Raspberry Pi 5 I2S slave -> IEC61937 type 0x15 @ 192 kHz -> raw E-AC-3 -> FFmpeg 5.1 PCM`.
This proves only DD+/E-AC-3 5.1 extraction/decoding in that tested setup.

## Not yet proven / physical boundary

- positive reproducible moving-object E-AC-3 JOC carrier satisfying the temporal harness;
- authored object-position correctness through OpenJOC;
- continuous physical eARC -> E-AC-3 JOC -> Aurora -> physical 7.1.4;
- Netflix/other DRM-service Atmos compatibility through authorized Aurora input;
- physical USB/TDM/STM32/DAC/amplifier/speaker path;
- private wireless speaker-network measured latency/sync/reliability;
- acoustic parity with Samsung Q995-class systems, thermal/EMI/production readiness, Dolby certification.

A positive moving-object carrier remains desirable evidence but is **not allowed to block unrelated software completion**. Use only lawful/public or user-supplied authorized media with provenance/SHA; never weaken the temporal gate.

## Current architecture / map

Control plane:
`aurora-config -> aurora-runtime-assembly -> aurora-runtime-inspection -> aurora-runtime-materialization -> prepared components -> aurora-realtime-engine`

Media direction:
`Source Manager/input -> decoder API -> scene/audio -> renderer API -> DSP API -> generic realtime audio I/O`

Application integration:
`application plugin <-> aurora-plugin-host <-> Source Manager/control APIs`

Key paths:
- config/runtime: `crates/aurora-config/`, `aurora-runtime-assembly/`, `aurora-runtime-materialization/`, `aurora-runtime-inspection/`
- realtime: `crates/aurora-realtime-engine/`, `aurora-realtime-audio-*`
- renderer/DSP/decoder: `crates/aurora-renderer-*`, `aurora-dsp-*`, `aurora-decoder-*`
- source/plugin: `crates/aurora-source-runtime/`, `aurora-plugin-api/`, `aurora-plugin-host/`
- immersive evidence: `validation/immersive/`
- CI: `.github/workflows/ci.yml`, `immersive-joc-stack-ci.yml`, `simulation-assurance-pr.yml`, `realtime-health-soak-ci.yml`

## Current work / next actions

### #115 — Omniphony reference evaluation (ACTIVE, PR #134)
Branch: `validation/omniphony-eval-v1`.

Implemented on branch:
- `config/omniphony-evaluation-v1.json` pins stable v0.5.2 `f9a79721af64ad9c39042d4deded158b568fc598` and evaluation-only candidate `4903c893d25ffac9012707bd320d5e66103f3e69` independently; no automatic promotion.
- `validation/immersive/test-omniphony-reference-comparison.sh` reuses the exact baseline JOC carrier/Harletty bridge, fetches candidate by exact SHA, fails closed on bridge ABI/layout/canonical-label incompatibility, builds the candidate externally, renders the same 7.1.4 input, records frame/duration/SHA/RMS/peak/active-lane metrics and informational wall time, and rejects frame-count or active-lane regressions.
- Upstream audit: bridge API source and `layouts/7.1.4.yaml` are identical between stable/candidate; enum discriminants and canonical labels are unchanged.
- First PR #134 Immersive run `34696195145`: baseline JOC + paced realtime proof passed; candidate build passed; comparison then hung because candidate `orender` now keeps the process alive after `StreamEnd` even with one file. Source audit confirmed `handle_stream_end()` finalizes/resets then waits for another stream while a sender remains live.
- Follow-up comparison behavior is bounded and fail-closed: explicitly request `--no-continuous`, wait for the post-finalize `Handler reset complete, ready for next stream` marker, request normal SIGTERM shutdown, reject non-zero exit/timeout/forced kill/incomplete output, and record the lifecycle termination mode in JSON. Future candidates that exit naturally remain accepted if render metrics pass.
- Immersive JOC workflow uploads comparison + label-contract JSON evidence.

Next:
1. Require fresh PR #134 general CI + Immersive JOC CI green on the lifecycle-fix head.
2. Inspect comparison JSON; a pass does not promote the candidate. Stable remains v0.5.2 unless a separate explicit promotion decision is justified.
3. Squash-merge green #134; verify #115 closes.
4. Reconcile #38 3D loudspeaker truth registry with already-merged software evidence, then close only if literal acceptance is satisfied.
5. Implement #70 out-of-process Plugin Host runtime/isolation needed for literal kill/restart evidence.
6. Finish #116 Source Manager acceptance using the real plugin-isolation proof.
7. Continue the critical-path software audit until remaining blockers genuinely require physical hardware, lawful external service/media behavior, or acoustic measurement.

### #70 / #116 dependency
`aurora-plugin-api` and package registry/update/rollback foundations exist, and Source Manager already implements most typed lifecycle/session/arbitration behavior. However, `aurora-plugin-host` does not yet spawn/manage isolated plugin processes, so #116's literal “killing/upgrading an unrelated plugin does not disturb the active source” proof depends on #70 process-host work. Do not close #116 on architecture intent alone.

### Critical-path target
All hardware-independent blockers needed for `TV/eARC -> JOC -> render/DSP -> generic multichannel output` should be merged and tested. Optional product extras (Music Hub, Home Assistant, UI polish) are not critical unless they become necessary dependencies. Queued trackers must be classified honestly; hardware-dependent acceptance stays open until measured on hardware.

## Validation policy

Base Rust gate:
```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

Immersive/JOC changes require official `.github/workflows/immersive-joc-stack-ci.yml` PR evidence. Runtime/config changes require official CI plus simulation smoke/realtime soak where applicable. Do not merge required red gates. Synthetic fixtures prove only the layer they actually exercise.
