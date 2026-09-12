# Aurora Agent Reference — Living Handoff

> Read this file first. It is Aurora's canonical compact handoff for coding agents.
>
> **Mandatory maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge. Keep it factual and compact.

Last updated: **2026-09-12**

## 1. Branch policy and resume point

- **Only long-lived branch:** `main-v2`.
- Feature/debug/validation branches are temporary. Delete them immediately after their accepted work is merged.
- Never leave temporary GitHub Actions workflows in a PR or on `main-v2`.
- Default continuation rule: if the user says **“كمل”**, continue the first unfinished item in **Current work / Next actions** below.

Current temporary branch: `feature/config-renderer-component-ref-v2`.
Current validated production commit on that branch: `309171a5575d19a509eed67b0cc9b4583c7a1f85` (`Migrate renderer selection to component references`).
Temporary config-v2 validation workflows have been removed from the branch.

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

### Merged/proven software baseline

- Hardware-agnostic Rust workspace with Linux/Windows/MSRV CI.
- Deterministic simulation assurance and sustained accelerated realtime-health soak.
- Generic prepared renderer + prepared realtime delay/DSP boundary in `RealTimeEngine`.
- Basic and VBAP renderers materialize through the same engine boundary.
- Second `RealtimeDelayProcessor` test adapter uses the same boundary.
- Failed replacement preparation leaves the active engine unchanged.
- Runtime assembly owns stable prepared component identities/compatible contract versions; runtime inspection reports them as prepared control-plane intent.
- Software JOC/IEC61937 validation lanes exist using pinned external Harletty/Omniphony references.
- OpenJOC is an independent external fail-closed/differential reference lane.

Key merged landmarks: #107, #108, #109-#112, #114, #117, #120, #121, #125, #126, #127, #128. Issue #118 is completed.

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
- Runtime assembly/registry: `crates/aurora-runtime-assembly/src/`
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

## 7. Current work — Issue #119

Issue #119 removes remaining control-plane coupling to replaceable implementation enums.

### Renderer slice — VALIDATED, pending merge to main-v2

Implemented/validated on `feature/config-renderer-component-ref-v2`:
- root renderer selection moved to generic versioned `ComponentReference` instead of hard-coded renderer enum variants;
- current root config schema is v2;
- explicit deterministic v0/v1 -> v2 migration;
- v1 fixtures retained as migration sources; v2 fixtures are canonical current fixtures;
- explicit fail-closed `RendererComponentRegistry` in runtime assembly;
- Basic and VBAP resolve by stable component references;
- unknown IDs, incompatible contracts, and invalid component config schemas/payloads fail closed;
- a test renderer can be registered without adding a root `AuroraConfiguration` enum variant;
- runtime inspection snapshots correctly report configuration schema v2;
- this is a control-plane/config change, not new audio/hardware capability.

Validated before production commit:
- workspace check + locked check PASS;
- full workspace clippy for the validation slice PASS;
- `aurora-config` contracts 17/17 PASS + zero-allocation immutable read PASS;
- runtime assembly 48/48 + contract suite 6/6 PASS;
- runtime inspection 22/22 PASS including exact JSON/TXT snapshots;
- realtime engine 48/48 PASS;
- prepared-delay tests 3/3 PASS;
- runtime materialization 5/5 PASS;
- relevant doc tests PASS.

The validator initially falsely matched the new symbol `RendererConfigurationResolver`; final gate correctly checks only legacy `RendererConfiguration::` enum usage.

### What remains for Issue #119

After the renderer slice is merged:
1. migrate `BackendIntent::{Virtual,Cpal,Offline}` to versioned audio-backend component references;
2. add explicit backend registry and capability/contract validation before activation;
3. prove adding a second backend needs no root config enum change;
4. migrate old backend values deterministically;
5. update runtime inspection/evidence with selected backend contract + implementation identity/version;
6. run full CI/simulation/realtime gates;
7. close #119 only when all acceptance criteria are actually satisfied.

## 8. Required validation before merge

Normally run:
```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

For performance/realtime changes also run the relevant release benchmarks/allocation guards.
For PRs affecting runtime/config behavior, require official PR CI plus simulation PR smoke and sustained realtime-health soak when those workflows apply.

Do not merge while any required gate is red. Delete temporary validation workflows before opening the PR.

## 9. Clean-development protocol

- `main-v2` is the single source of truth.
- Create a short-lived branch only when isolation is needed.
- Keep changes reviewable and evidence explicit.
- Merge only green accepted work.
- Immediately delete the merged branch.
- Never keep abandoned experimental branches as alternate baselines; PR/commit history already preserves them.
- Update this file so the next agent can resume without chat history.
