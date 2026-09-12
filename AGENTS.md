# Aurora Agent Reference — Living Handoff

> Read this file first. It is Aurora's canonical compact handoff for coding agents.
>
> **Mandatory maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge. Keep it factual and compact.

Last updated: **2026-09-12**

## 1. Repository state and branch policy

- **Single long-lived source of truth:** `main-v2`.
- Feature/debug branches may be created temporarily when isolation is useful, but must be deleted immediately after accepted work is merged.
- Never leave temporary GitHub Actions workflows or patch scripts on `main-v2` or in a PR diff.
- If the user says **“كمل” / “continue”**, continue the first unfinished item in **Current work / Next actions** below. Do not redo project discovery first.

Latest integrated architecture slice: **PR #131**, squash commit `0f37b6df1587d429587b74eac714309dc8299d34`.

Completed control-plane trackers: **#118**, **#119**, **#130**.

Active validation slice: **#132** on `validation/joc-temporal-evidence-v1` — fail-closed temporal JOC evidence.

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
- Realtime callback code: no allocation after preparation, locks, logging/formatting, filesystem/process access, configuration parsing/registry lookup, or silent device changes.
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
- Root renderer configuration uses versioned `ComponentReference` selection rather than hard-coded renderer implementation enums.
- Root input/output backend selection uses versioned `ComponentReference` rather than `BackendIntent::{Virtual,Cpal,Offline}`.
- Root config schema is **v3** with explicit deterministic v0/v1/v2 migration and canonical v3 fixtures/checksums; no silent reinterpretation.
- `RendererComponentRegistry` and `BackendComponentRegistry` resolve stable Aurora-owned component IDs and fail closed on incompatible IDs/contracts/config/capabilities before activation.
- Custom test renderer and backend registrations require no new root `AuroraConfiguration` implementation enum variants.
- `PreparedComponentIdentity` carries implementation ID, implementation version, contract major, and contract minor.
- Runtime inspection schema is **v3** and reports exact renderer/realtime-delay/backend prepared identities as control-plane intent.
- Realtime-engine boundary is documented in `crates/aurora-realtime-engine/README.md`: only prepared typed components cross into realtime execution; component JSON/registry logic stays in the control plane.
- Software JOC/IEC61937 validation lanes exist using pinned external Harletty/Omniphony references.
- OpenJOC is an independent external fail-closed/differential reference lane.

Key merged landmarks: #107, #108, #109-#112, #114, #117, #120, #121, #125, #126, #127, #128, #129, #131.

### PR #131 / issue #119 completion evidence

PR #131 merged as `0f37b6df1587d429587b74eac714309dc8299d34` after every required gate on final head `44100e5c1076d09a9fb37d2ec9baafbbac08e1e0` passed:
- CI run `34693844433`: Linux stable, Windows stable, MSRV 1.78, full workspace tests/clippy/docs, Criterion + regression policy, renderer evaluation, 3D VBAP evaluation, generic 7.1.4 + output DSP.
- Simulation Assurance PR Smoke run `34693844411`: 500-scenario deterministic campaign, warmed-up allocation guards, bounded report upload.
- Sustained Realtime Health Soak run `34693844412`: accelerated 10-minute media-time 7.1.4 soak + report validation/evidence upload.
- Earlier focused backend-v3 validation run `34693520855` also passed compile/locked compile/clippy/config/runtime/inspection/materialization/realtime tests and legacy-`BackendIntent` rejection.

Issue #130 closed automatically by #131. Issue #119 was reviewed against its literal acceptance criteria after the green merge and closed **completed**.

### Temporal JOC truth boundary under active #132

The public Harletty `joc_atmos_1s.eac3` carrier is valid positive JOC/admission/differential input but is **not** sufficient evidence of moving-object behavior: its public metadata is temporally weak and must not be upgraded into a motion claim by repetition/soak alone.

The pinned public OpenJOC source provides inspector/renderer tooling and object-scene statistics, but no tracked public `.eac3/.ec3/.mp4/.m4a` moving-JOC corpus suitable as reproducible CI evidence. A previously researched 4-second dual-object carrier is private evidence and is not an acceptable repository/CI dependency.

Issue #132 therefore separates four evidence classes:
1. codec/JOC admission and timing continuity;
2. timed OAMD/object-state diversity from OpenJOC inspection;
3. independent 12-channel rendered temporal-energy diversity;
4. realtime pacing/health evidence.

`render-joc` remains self-consistency evidence, **not** an independent oracle for original authored object-position correctness.

### Historical hardware proof — limited

Previously demonstrated:
`Lindy 38368 / SiI9437 eARC -> Raspberry Pi 5 I2S slave -> IEC61937 type 0x15 @ 192 kHz -> raw E-AC-3 -> FFmpeg 5.1 PCM`.

This proves only DD+/E-AC-3 5.1 extraction/decoding in that tested setup.

### Not yet proven

- reproducible public moving-object JOC carrier with sufficient timed OAMD/object-state diversity;
- authored object-position correctness through OpenJOC rendering;
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
- Realtime boundary note: `crates/aurora-realtime-engine/README.md`
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
- Temporal JOC analyzer/harness: `validation/immersive/joc_temporal_evidence.py`, `validation/immersive/test-joc-temporal-evidence.sh`
- OpenJOC reference lane: `validation/immersive/test-openjoc-reference.sh`
- Realtime JOC pacing lane: `validation/immersive/test-joc-realtime-soak.sh`
- Immersive truth matrix: `validation/immersive/README.md`
- Main CI: `.github/workflows/ci.yml`
- Immersive JOC CI: `.github/workflows/immersive-joc-stack-ci.yml`
- PR simulation smoke: `.github/workflows/simulation-assurance-pr.yml`
- Realtime soak: `.github/workflows/realtime-health-soak-ci.yml`
- External component/license decisions: `config/external-components-v1.json`, `THIRD_PARTY_LICENSES.md`

Useful search symbols:
`RealTimeEngine::new_with_prepared_components`, `RendererCapabilities`, `RealtimeDelayProcessor`, `PreparedComponentIdentity`, `PreparedRealtimeComponentSelection`, `ComponentReference`, `RendererComponentRegistry`, `BackendComponentRegistry`, `VIRTUAL_BACKEND_IMPLEMENTATION_ID`, `CPAL_BACKEND_IMPLEMENTATION_ID`, `OFFLINE_BACKEND_IMPLEMENTATION_ID`, `CURRENT_SCHEMA_VERSION`, `SourceManager`.

Legacy invariants: implementation-selection `RendererConfiguration` and `BackendIntent` enums must not reappear in current root config/runtime-control surfaces.

## 7. Current work / Next actions — issue #132 temporal JOC evidence

Branch: `validation/joc-temporal-evidence-v1`.

Implemented on the branch:
- `joc_temporal_evidence.py`: deterministic analyzer for codec/JOC gates, timed object-scene diversity, windowed 12-channel RMS/peak/active-lane diversity, pacing metadata, stable machine-readable report, and fail-closed classifications.
- `test-joc-temporal-evidence.sh`: requires input carrier, exact SHA-256, non-empty provenance; runs pinned OpenJOC reference first; normalizes 7.1.4 output; returns `0` only for sufficient temporal evidence, `3` for `insufficient_temporal_diversity`, and `2` for invalid/contract evidence.
- `test-openjoc-reference.sh` now requests `--aus` so AU timestamps are retained alongside object/EMDF evidence.
- Immersive JOC CI runs analyzer self-tests and then asserts that the current public Harletty carrier reaches the codec/JOC gates but fails the temporal proof for the expected insufficient-diversity reason.
- `validation/immersive/README.md` documents the evidence matrix and truth boundary.

Next actions, in order:
1. Open the focused #132 PR against `main-v2`.
2. Run the official Immersive JOC Stack CI on the PR head and inspect the real analyzer JSON/artifacts.
3. If the public Harletty carrier does not fail for exactly `insufficient_temporal_diversity`, fix the harness/contract rather than weakening the gate.
4. Preserve #114 OpenJOC/differential/realtime-soak behavior; no regressions accepted.
5. Merge only after green official evidence and update this file with PR/run/SHA evidence.
6. Do **not** close the broader PC moving-object proof as “proven” until a reproducible carrier actually satisfies timed object-state and rendered temporal-diversity requirements.
7. After #132 lands, find/acquire or legally generate a reproducible moving-object JOC carrier; only then run the harness in positive-proof mode.

Other queued trackers:
- #115: evaluate newer Omniphony behind a separate reference lane; do not upgrade the stable pinned reference by recency alone.
- #116: Source Manager acceptance tracker; #121 landed the transactional foundation, but reassess literal remaining acceptance before closing.

## 8. Required validation before merge

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

For immersive/JOC validation changes, require the official `.github/workflows/immersive-joc-stack-ci.yml` PR run and inspect its emitted evidence. Synthetic analyzer tests prove only report/metric logic; they are never codec/JOC proof.

For performance/realtime changes also run relevant release benchmarks/allocation guards. For runtime/config PRs require official PR CI plus simulation smoke and sustained realtime-health soak when applicable.

Do not merge while any required gate is red. Delete temporary validation workflows before PR/merge.

## 9. Clean-development protocol

- `main-v2` is the only long-lived branch and source of truth.
- Use short-lived branches only when needed for isolation.
- Merge only green accepted work, preferably squash when a branch contains exploratory/debug commits.
- Delete the merged branch immediately.
- PR/commit history preserves experiments; do not keep abandoned branches as alternate baselines.
- Keep this file current so the next agent can resume without chat history.
