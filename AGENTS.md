# Aurora Agent Reference — Living Handoff

> Read this file first. It is Aurora's canonical compact handoff for coding agents.
>
> **Mandatory maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge. Keep it factual and compact.

Last updated: **2026-09-12**

## 1. Repository state and branch policy

- **Single source of truth and only branch:** `main-v2`.
- All stale feature/debug/validation branches were purged on 2026-09-12 after their accepted work was consolidated.
- There are currently no open pull requests at this handoff point.
- Feature/debug branches may be created temporarily when isolation is useful, but must be deleted immediately after accepted work is merged.
- Never leave temporary GitHub Actions workflows on `main-v2`.
- If the user says **“كمل” / “continue”**, continue the first unfinished item in **Current work / Next actions** below. Do not redo project discovery first.

Latest integrated architecture slice: **PR #129**, squash commit `8ac1db7293628456eded4dc61fb79f791eaf497d`.

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
- Runtime assembly owns stable prepared component identities/compatible contract versions; runtime inspection reports them as prepared control-plane intent.
- Root renderer configuration now uses versioned `ComponentReference` selection rather than hard-coded renderer implementation enum variants.
- Config schema v2 has explicit deterministic v0/v1 migration and canonical v2 fixtures.
- Runtime assembly has a fail-closed `RendererComponentRegistry`; adding a test renderer does not require a new root config enum variant.
- Software JOC/IEC61937 validation lanes exist using pinned external Harletty/Omniphony references.
- OpenJOC is an independent external fail-closed/differential reference lane.

Key merged landmarks: #107, #108, #109-#112, #114, #117, #120, #121, #125, #126, #127, #128, #129. Issue #118 is completed.

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

Stable prepared IDs already established:
- `org.aurora.renderer.basic`
- `org.aurora.renderer.vbap`
- `org.aurora.dsp.basic-delay`

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
`RealTimeEngine::new_with_prepared_components`, `RendererCapabilities`, `RealtimeDelayProcessor`, `PreparedComponentIdentity`, `PreparedRealtimeComponentSelection`, `ComponentReference`, `RendererComponentRegistry`, `CURRENT_SCHEMA_VERSION`, `BackendIntent`, `SourceManager`.

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

### Next slice — backend component references

1. Replace `BackendIntent::{Virtual,Cpal,Offline}` implementation selection with versioned audio-backend `ComponentReference` values while preserving stable product intent.
2. Add explicit backend registry with contract/version/config-schema validation before activation.
3. Prove a second/test backend can be added without a root `AuroraConfiguration` enum change.
4. Provide deterministic migration from old `Virtual/Cpal/Offline` values.
5. Report selected backend contract + implementation identity/version in runtime inspection/evidence with correct truth semantics.
6. Run full config/assembly/inspection/realtime CI + simulation + soak gates.
7. Close #119 only after all acceptance criteria are met.

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
