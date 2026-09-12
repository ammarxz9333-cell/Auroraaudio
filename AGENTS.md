# Aurora Agent Reference — Living Handoff

> Canonical compact handoff. Update this file with every meaningful architecture, validation, issue, PR, or critical-path change.

Last updated: **2026-09-12**

## Repository policy

- Source of truth: `main-v2`.
- Keep at most one focused implementation branch; squash/merge green work and remove stale branches.
- Never leave temporary workflows/scripts on `main-v2`.
- “كمل” means continue the first unfinished item below without rediscovery.

Latest merged architecture: PR #131, squash `0f37b6df1587d429587b74eac714309dc8299d34`.
Latest merged validation: PR #133, squash `1445f0f29b9144dd2a958d6b57ff59285e439dec`.
Completed trackers include #118, #119, #130, #132.

## Product / safety invariants

Aurora is a hardware-agnostic Rust immersive-audio stack targeting realtime 7.1.4 and later 11.1.4, replaceable decoder/renderer/DSP/audio-I/O/transport/hardware adapters, authorized TV/eARC ingestion, and private wireless speaker transport.

- no allocation, locks, logging, filesystem/process/config/registry work in the steady-state realtime callback;
- prepare -> validate -> commit; failed candidates never mutate active state;
- unknown/incompatible IDs, schemas, capabilities, generations, or contracts fail closed;
- object decode, channel decode, and synthetic upmix are distinct;
- configured/simulated/estimated latency is never called measured latency;
- application plugins stay outside realtime memory and use Aurora-owned source/control boundaries;
- no DRM circumvention, Widevine bypass, protected-media extraction, or device-identity spoofing;
- no Dolby/proprietary-streaming certification claim from software CI;
- GPL/incompatible references remain external validation processes.

## Merged software truth

- Hardware-agnostic Rust workspace with Linux/Windows/MSRV CI.
- Deterministic simulation assurance and sustained accelerated realtime-health soak.
- Generic prepared renderer + realtime DSP boundary in `RealTimeEngine`.
- Renderer and input/output backend selection use versioned `ComponentReference`; config schema v3 has explicit v0/v1/v2 migration.
- Renderer/backend registries fail closed; prepared identities and runtime inspection report exact implementation/version/contract intent.
- Pinned Harletty/Omniphony validate the software JOC/IEC61937 path; pinned OpenJOC is an independent external differential/fail-closed reference.
- PR #133 added fail-closed temporal JOC evidence. Public Harletty `joc_atmos_1s.eac3` passes JOC admission but must fail moving-object proof as `insufficient_temporal_diversity`.

PR #131 gates: CI `34693844433`, simulation `34693844411`, realtime soak `34693844412` — PASS.
PR #133 gates: CI `34694981490`, Immersive JOC `34694981484` — PASS.

Historical hardware proof remains only:
`Lindy 38368 / SiI9437 eARC -> Raspberry Pi 5 I2S slave -> IEC61937 type 0x15 @ 192 kHz -> raw E-AC-3 -> FFmpeg 5.1 PCM`.
It proves DD+/E-AC-3 5.1 extraction/decoding only for that tested setup.

## Physical / external truth still unproven

- positive reproducible moving-object E-AC-3 JOC carrier satisfying the temporal harness;
- authored object-position correctness through OpenJOC;
- continuous physical eARC -> JOC -> Aurora -> physical 7.1.4;
- Netflix/other DRM-service Atmos compatibility through an authorized physical input;
- physical USB/TDM/STM32/DAC/amplifier/speaker chain;
- private wireless measured latency/sync/reliability;
- acoustic parity, thermal/EMI/production readiness, Dolby certification.

A moving-object carrier is useful evidence but may not block unrelated software completion. Use only lawful/public or user-supplied authorized media with provenance/SHA and never weaken the gate.

## Current architecture

Control: `aurora-config -> runtime-assembly -> runtime-inspection -> runtime-materialization -> prepared components -> realtime-engine`

Media: `Source Manager/input -> decoder -> scene/audio -> renderer -> DSP -> generic realtime audio I/O`

Apps: `application plugin <-> plugin host <-> Source Manager/control APIs`

Key paths: `crates/`, `validation/immersive/`, `.github/workflows/`.

## Active work — #115 / PR #134

Branch: `validation/omniphony-eval-v1`.

The evaluation lane keeps stable Omniphony v0.5.2 `f9a79721af64ad9c39042d4deded158b568fc598` and candidate `4903c893d25ffac9012707bd320d5e66103f3e69` independent; it never auto-promotes the candidate.

It reuses the same Harletty/JOC IEC61937 carrier and bridge, fails closed on bridge ABI / 7.1.4 layout / canonical channel-label incompatibility, renders the same 12-channel output, and records frame count, duration, SHA-256, per-channel RMS/peak, active-lane count, and lifecycle evidence.

Candidate lifecycle audit: current upstream sends `StreamEnd` only in continuous mode. The evaluation therefore runs `--continuous`, waits for `Handler reset complete, ready for next stream` (which occurs after output finalization), then requests normal SIGTERM. Timeout, forced kill, non-zero shutdown, incomplete output, frame drift, or active-lane regression fail closed.

External Cargo reproducibility is now explicit. Omniphony upstream has no committed lockfile and `env_logger = "0.11.8"` previously floated to newer transitive packages; a later `jiff 0.2.36` registry package broke CI because expected source docs were missing. Both stable and candidate now apply `validation/immersive/omniphony-external-cargo-pins.patch`, pin exact `env_logger=0.11.8` + `jiff=0.2.15`, generate a lockfile, assert those exact versions, and build `--locked`. This is validation-only dependency stabilization, not a production Aurora dependency.

Required next actions:
1. Require fresh official General CI + Immersive JOC CI green on the production-only PR #134 head.
2. Inspect comparison JSON; a pass does not promote candidate Omniphony.
3. Squash-merge #134 and verify #115 closes.
4. Integrate the Aurora Digital Twin simulator as a separate focused branch with deterministic self-tests/build CI and explicit modeled-vs-measured truth labels.
5. Reconcile #38 3D loudspeaker capability truth with existing 5.1.2/7.1.4 software evidence.
6. Implement #70 real out-of-process plugin isolation (spawn/kill/restart/quarantine).
7. Finish #116 Source Manager acceptance using the real plugin-isolation proof.
8. Continue software audit until remaining blockers genuinely require physical hardware, lawful external service/media behavior, or acoustic measurement.

## #70 / #116 dependency

Plugin package admission/update/rollback and most Source Manager lifecycle/session/arbitration behavior already exist. `aurora-plugin-host` still lacks real isolated process supervision, so #116's literal “kill/upgrade unrelated plugin without disturbing active source” acceptance depends on #70. Do not close #116 on architecture intent alone.

## Digital Twin direction

The simulator is a validation front-end, not a replacement for production Rust components. Its model must preserve an opaque authorized streaming/DRM boundary and simulate the allowed device output onward: HDMI/eARC, IEC61937, E-AC-3/JOC scene, renderer/DSP, transport, DAC/amp/speakers, room, and listener. Model/surrogate evidence must be labeled separately from Aurora production trace replay and measured HRTF/BRIR data.

## Validation policy

Base Rust gate:
```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

Immersive/JOC changes require official `immersive-joc-stack-ci.yml` PR evidence. Runtime/config changes require official CI plus simulation smoke/realtime soak where applicable. Do not merge required red gates. Synthetic/model fixtures prove only the layer they actually exercise.
