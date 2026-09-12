# Aurora Agent Reference — Living Handoff

> Read this file first. It is Aurora's canonical compact handoff for coding agents.
>
> **Mandatory maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge. Keep it factual and compact.

Last updated: **2026-09-12**

## 1. Repository state and branch policy

- **Single long-lived source of truth:** `main-v2`.
- All stale feature/debug/validation branches were purged on 2026-09-12 after accepted work was consolidated.
- Feature/debug branches may be created temporarily when isolation is useful, but must be deleted immediately after accepted work is merged.
- Never leave temporary GitHub Actions workflows or patch scripts on `main-v2` or in a PR diff.
- If the user says **“كمل” / “continue”**, continue the first unfinished item in **Current work / Next actions** below. Do not redo project discovery first.

Latest integrated architecture slice: **PR #129**, squash commit `8ac1db7293628456eded4dc61fb79f791eaf497d`. Current `main-v2` baseline before the backend slice: `04cb0f1eb3fce29b6f6e169e17907f97a10d2164`. Active short-lived branch: `refactor/backend-component-ref-v2` for issue #130 / parent #119.

## 2. Product goal

Aurora is an open, modular, hardware-agnostic immersive-audio stack, primarily Rust.

Long-term direction:
- realtime multichannel audio, initially 7.1.4-class and expandable toward 11.1.4;
- replaceable source, decoder, renderer, DSP, audio-I/O, transport, and hardware adapters;
- deterministic simulation and strong evidence gates;
- eventual authorized TV/eARC ingestion and physical multichannel output;
- later private wireless speaker/rear/multi-room transport;
- no specific SBC, MCU, DAC, eARC board, speaker product, AVR, soundbar, or OS image may define the core architecture.

Read only when relevant: `README.md`, `VISION.md`, `docs/architecture.md`, `docs/PROJECT_SCOPE.md`, `docs/plugin-architecture.md`, `docs/configuration.md`, `THIRD_PARTY_LICENSES.md`.

## 3. Non-negotiable rules

- Core remains hardware-agnostic; platform/device work enters through narrow Aurora-owned adapters.
- Realtime callback code: no allocation after preparation, locks, logging/formatting, filesystem/process access, or silent device changes.
- Use caller-owned/fixed buffers on steady-state renderer/DSP paths.
- Prepare -> validate -> commit is transactional; failed candidates must not mutate active runtime/source state.
- Unknown/incompatible component IDs, schemas, contracts, capabilities, or generations fail closed.
- Object decoding, channel decoding, and synthetic upmixing are distinct. Never silently substitute one for another.
- Configuration/prepared intent is not runtime or physical evidence.
- Never call configured/simulated/estimated latency “measured”. Physical loopback is required for measured round-trip latency.
- Application plugins stay out of the realtime callback and use Aurora-owned source/control APIs.
- No DRM circumvention, Widevine bypass, protected-media extraction, or device-identity spoofing.
- No Dolby certification/proprietary-streaming claim from software CI.
- GPL/incompatible reference implementations remain external; do not copy/link them into Aurora core.

## 4. Evidence truth

### Proven/merged software behavior

- Hardware-agnostic Rust workspace with Linux/Windows/MSRV CI.
- Deterministic simulation assurance and sustained accelerated realtime-health soak.
- Generic prepared renderer + prepared realtime delay/DSP boundary in `RealTimeEngine`.
- Basic and VBAP renderers materialize through the same engine boundary.
- Second `RealtimeDelayProcessor` test adapter uses the same boundary.
- Failed replacement preparation leaves the active engine unchanged.
- Root renderer configuration uses versioned `ComponentReference` selection rather than hard-coded renderer implementation enum variants.
- Config schema v2 has explicit deterministic v0/v1 migration and canonical v2 fixtures.
- Runtime assembly has a fail-closed `RendererComponentRegistry`; adding a test renderer does not require a new root config enum variant.
- Prepared component identities are Aurora-owned and runtime inspection reports them as prepared control-plane intent.
- Software JOC/IEC61937 validation lanes exist using pinned external Harletty/Omniphony references.
- OpenJOC is an independent external fail-closed/differential reference lane.

Key merged landmarks: #107, #108, #109-#112, #114, #117, #120, #121, #125, #126, #127, #128, #129. Issue #118 is completed.

### Backend component-reference slice — implemented on branch, not merged yet

- Root input/output backend selection now uses versioned `ComponentReference` instead of `BackendIntent::{Virtual,Cpal,Offline}`.
- Root config schema is **v3** with explicit deterministic v2 -> v3 migration layered on existing v0/v1 migration; no silent reinterpretation.
- `BackendComponentRegistry` resolves built-ins by stable Aurora IDs and fails closed on unknown IDs, contract mismatch, implementation-version pin mismatch, config-schema/payload mismatch, direction/platform/format/channel incompatibility, and realtime-safety mismatch.
- A custom test backend registers without adding a root config enum variant.
- `PreparedComponentIdentity` now carries implementation ID, implementation version, contract major, and contract minor.
- Runtime inspection schema is **v3** and reports exact requested backend identity/version/contract/config schema plus renderer/delay identities as control-plane intent.
- Final focused validation run `34693520855` passed compile, locked compile, clippy `-D warnings`, config/runtime-assembly/runtime-inspection tests, realtime materialization/engine tests, snapshot stability, and the legacy-`BackendIntent` rejection check.
- All temporary backend validation workflows/scripts were removed from the branch diff after the green run.
- **Official PR CI/simulation/soak gates are still pending; do not merge yet.**

### Historical hardware proof — limited

Previously demonstrated:
`Lindy 38368 / SiI9437 eARC -> Raspberry Pi 5 I2S slave -> IEC61937 type 0x15 @ 192 kHz -> raw E-AC-3 -> FFmpeg 5.1 PCM`.

This proves only DD+/E-AC-3 5.1 extraction/decoding in that tested setup.

### Not yet proven

- physical continuous eARC -> E-AC-3 JOC -> Aurora -> physical 7.1.4;
- Netflix/other DRM-service Atmos compatibility through Aurora;
- physical STM32/TDM16/USB multichannel path;
- final DAC/amplifier/speaker design;
- acoustic parity with Samsung Q995-class systems;
- private wireless speaker-network latency/sync/reliability;
- thermal/EMI/production hardware readiness;
- Dolby certification.

## 5. Current architecture

Control plane:
`aurora-config -> aurora-runtime-assembly -> aurora-runtime-inspection -> aurora-runtime-materialization -> prepared components -> aurora-realtime-engine`

Media/realtime direction:
`source/input -> decoder API -> scene/audio -> renderer API -> DSP API -> generic realtime audio I/O`

Provider/application integrations:
`application plugin <-> aurora-plugin-host <-> Aurora source/control APIs`

Stable prepared/component IDs established:
- `org.aurora.renderer.basic`
- `org.aurora.renderer.vbap`
- `org.aurora.dsp.basic-delay`
- `org.aurora.backend.virtual`
- `org.aurora.backend.cpal`
- `org.aurora.backend.offline`

Current built-in backend implementation versions are `0.1.0`; realtime backend contract is 1.0.

## 6. Fast repository map

- Workspace/MSRV/deps: `Cargo.toml`, `Cargo.lock`
- Core layouts/capabilities: `crates/aurora-core/`
- Scene/object model: `crates/aurora-scene/`
- Renderer API/basic/VBAP: `crates/aurora-renderer-api/`, `aurora-renderer-basic/`, `aurora-renderer-vbap/`
- Decoder API/adapters: `crates/aurora-decoder-api/`, `aurora-decoder-*`
- DSP API/basic/external: `crates/aurora-dsp-api/`, `aurora-dsp-basic/`, `aurora-dsp-camilladsp/`
- Generic audio I/O: `crates/aurora-audio-io/`
- Realtime backend API/CPAL/sim: `crates/aurora-realtime-audio-api/`, `aurora-realtime-audio-cpal/`, `aurora-realtime-audio-sim/`
- Realtime engine: `crates/aurora-realtime-engine/src/lib.rs`
- Drift/ASRC: `crates/aurora-realtime-engine/src/{drift,drift_controller,asrc}.rs`
- Duplex/transport/device/latency: `crates/aurora-realtime-engine/src/{duplex,transport,device_state,latency}.rs`
- Config model/validation/migration/presets: `crates/aurora-config/src/{model,validation,migration,preset}.rs`
- Config fixtures: `fixtures/config/`
- Runtime assembly/registries: `crates/aurora-runtime-assembly/src/`
- Backend registry: `crates/aurora-runtime-assembly/src/backend_registry.rs`
- Runtime materialization: `crates/aurora-runtime-materialization/src/lib.rs`
- Runtime inspection/snapshots: `crates/aurora-runtime-inspection/`
- Source Manager: `crates/aurora-source-runtime/`
- Plugin API/host: `crates/aurora-plugin-api/`, `crates/aurora-plugin-host/`
- Simulation/acceptance: `crates/aurora-simulation-assurance/`, `crates/aurora-realtime-acceptance/`
- CLI/evaluation: `crates/aurora-cli/`
- Immersive/JOC evidence: `validation/immersive/`
- Main CI: `.github/workflows/ci.yml`
- PR simulation smoke: `.github/workflows/simulation-assurance-pr.yml`
- Realtime soak: `.github/workflows/realtime-health-soak-ci.yml`
- External component/license decisions: `config/external-components-v1.json`, `THIRD_PARTY_LICENSES.md`

Useful search symbols:
`RealTimeEngine::new_with_prepared_components`, `RendererCapabilities`, `RealtimeDelayProcessor`, `PreparedComponentIdentity`, `PreparedRealtimeComponentSelection`, `ComponentReference`, `RendererComponentRegistry`, `BackendComponentRegistry`, `VIRTUAL_BACKEND_IMPLEMENTATION_ID`, `CPAL_BACKEND_IMPLEMENTATION_ID`, `OFFLINE_BACKEND_IMPLEMENTATION_ID`, `CURRENT_SCHEMA_VERSION`, `SourceManager`.

Legacy invariant: `BackendIntent` must not exist in current production/config/test/docs surfaces after the v3 backend slice.

## 7. Current work / Next actions — Issue #119

Issue #119 removes remaining control-plane coupling to replaceable implementation enums.

### Renderer slice — DONE / merged in #129

- generic versioned renderer `ComponentReference` in root config;
- config schema v2;
- explicit deterministic v0/v1 -> v2 migration;
- v1 migration fixtures + canonical v2 fixtures/checksums;
- fail-closed `RendererComponentRegistry`;
- Basic and VBAP resolve by stable component IDs/contracts;
- unknown/incompatible IDs/contracts/config schemas fail closed;
- test renderer proves no new root config enum variant is required;
- runtime inspection metadata/snapshots updated for config schema v2.

Official #129 gates all passed: Linux, Windows, MSRV 1.78, Simulation Assurance PR Smoke, Sustained Realtime Health Soak, Criterion/regression policy, deterministic renderer evaluation, 3D VBAP evaluation, generic 7.1.4 rendering/output DSP, and public API docs.

### Backend slice — READY FOR PR (#130)

Implemented:
1. `BackendIntent::{Virtual,Cpal,Offline}` implementation selection replaced by versioned input/output backend `ComponentReference` values.
2. Config schema v3 with explicit deterministic v2 -> v3 migration and canonical v3 fixtures/checksums.
3. Explicit fail-closed `BackendComponentRegistry` validates ID, contract/version, payload schema, direction, platform, sample rate, channel count, and realtime safety before activation.
4. Custom test backend proves no root `AuroraConfiguration` enum change is required.
5. Prepared runtime plan and inspection report exact backend implementation ID/version + contract 1.0 as prepared control-plane intent.
6. Renderer/realtime-delay prepared identities also expose exact implementation versions and contract major/minor.
7. Focused validation run `34693520855` is green and branch diff contains no temporary validation files.

Next action:
- open the focused PR against `main-v2` and run official CI + Simulation Assurance PR Smoke + Sustained Realtime Health Soak;
- squash-merge only if every required gate is green;
- close #130 after merge;
- then review #119 acceptance literally and close it only if the merged renderer + backend slices satisfy every criterion.

After #119, re-evaluate the critical path toward stronger JOC/Atmos realtime evidence rather than doing unrelated refactors.

## 8. Required validation before merge

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

For performance/realtime changes also run relevant release benchmarks/allocation guards. For runtime/config PRs require official PR CI plus simulation smoke and sustained realtime-health soak when applicable.

Do not merge while any required gate is red. Delete temporary validation workflows before PR/merge.

## 9. Clean-development protocol

- `main-v2` is the only long-lived branch and source of truth.
- Use short-lived branches only when needed for isolation.
- Merge only green accepted work, preferably squash when a branch contains exploratory/debug commits.
- Delete the merged branch immediately.
- PR/commit history preserves experiments; do not keep abandoned branches as alternate baselines.
- Keep this file current so the next agent can resume without chat history.
