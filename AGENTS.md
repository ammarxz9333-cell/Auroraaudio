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

Latest integrated validation-infrastructure slice: **PR #136**, squash commit `02e5495c9ad891372fca706c233940e35a2d9464`.

Positive moving-object reference proof is being promoted by **PR #140** after the official green validation described below.

Completed trackers: **#118**, **#119**, **#130**, **#132**.

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
- Temporal JOC evidence is fail-closed: codec/JOC validity, timed OAMD/object-state diversity, rendered 12-channel temporal-energy diversity, and pacing/health are reported separately.
- Authorized-carrier handling is checksum-pinned and provenance-aware; raw carriers are never silently re-encoded.

Key merged landmarks: #107, #108, #109-#112, #114, #117, #120, #121, #125, #126, #127, #128, #129, #131, #133, #136.

### PR #131 / issue #119 completion evidence

PR #131 merged as `0f37b6df1587d429587b74eac714309dc8299d34` after every required gate on final head `44100e5c1076d09a9fb37d2ec9baafbbac08e1e0` passed:
- CI run `34693844433`: Linux stable, Windows stable, MSRV 1.78, full workspace tests/clippy/docs, Criterion + regression policy, renderer evaluation, 3D VBAP evaluation, generic 7.1.4 + output DSP.
- Simulation Assurance PR Smoke run `34693844411`: 500-scenario deterministic campaign, warmed-up allocation guards, bounded report upload.
- Sustained Realtime Health Soak run `34693844412`: accelerated 10-minute media-time 7.1.4 soak + report validation/evidence upload.
- Earlier focused backend-v3 validation run `34693520855` also passed compile/locked compile/clippy/config/runtime/inspection/materialization/realtime tests and legacy-`BackendIntent` rejection.

Issue #130 closed automatically by #131. Issue #119 was reviewed against its literal acceptance criteria after the green merge and closed **completed**.

### PR #133 / issue #132 temporal JOC evidence

PR #133 merged as `1445f0f29b9144dd2a958d6b57ff59285e439dec` from validated head `b67f29fe1ba7fdfdefeb2b83762726d365926c2d`.

Official final-head validation:
- General CI run `34694981490`: Linux stable, Windows stable, MSRV 1.78, fmt/check/clippy/workspace tests/docs, Criterion regression policy, renderer/3D-VBAP evaluation, generic 7.1.4 + output DSP — **PASS**.
- Immersive JOC Stack run `34694981484`: analyzer self-test, baseline + paced IEC61937/Harletty/Omniphony 7.1.4, checksum-pinned OpenJOC install, independent differential validation, end-to-end temporal shell harness, and artifact upload — **PASS**.
- The public Harletty `joc_atmos_1s.eac3` carrier passes the codec/JOC admission/timing gate but the temporal harness deliberately returns the stable classification `insufficient_temporal_diversity`. CI requires that exact fail-closed result, preventing the static fixture from being misrepresented as moving-object proof.

Issue #132 closed **completed** by #133.

The temporal evidence contract separates:
1. codec/JOC admission and timing continuity;
2. timed OAMD/object-state diversity from OpenJOC inspection;
3. independent 12-channel rendered temporal-energy diversity;
4. realtime pacing/health evidence.

`render-joc` remains self-consistency evidence, **not** an independent oracle for original authored object-position correctness.

### PR #140 positive moving-object reference evidence

PR #140 adds a permanent checksum-pinned lane using Dolby's publicly hosted Digital Plus Online Delivery Kit v1.4.1 carrier `Living-Room-Atmos_6ch_640kbps_ddp_joc.ec3`.

Truth-preserving boundary:
- untouched carrier SHA-256: `2470373db2c3621d56a2852df070e140293e9a99fdaa07e5c06de3c86bec307f`;
- pinned OpenJOC 0.17.0 identifies the untouched carrier as JOC and deployed-compatible, but classifies exactly AU0 with `MALFORMED_OAMD_METADATA` / `reserved OAMD object size index 3`;
- Aurora therefore does **not** claim the untouched carrier passes OpenJOC 0.17.0;
- the positive lane removes exactly that first raw E-AC-3 AU (2560 bytes), performs no re-encoding, verifies the remaining bytes are an exact suffix, and pins derived SHA-256 `0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0`.

Official PR #140 `Official Dolby JOC Temporal CI` run `34707477177` — **PASS**:
- derived carrier: 2360 accepted access units, continuous frame/metadata timing, zero diagnostics;
- 37,760 metadata updates and 15 dynamic object indices;
- OpenJOC 7.1.4 render: 12 channels, 48 kHz, 75.520667 s;
- fail-closed analyzer: `metadata_diversity=true`, `rendered_diversity=true`, verdict `pass`;
- no parser gate, threshold, or evidence requirement was weakened.

This proves a reproducibly retrievable authorized source path plus positive moving-object **reference/software evidence**. It does **not** prove Dolby conformance, original authored-position correctness, DRM-service compatibility, physical output, or that Aurora's Harletty/Omniphony path itself has yet reproduced the same moving-object temporal behavior.

### Historical hardware proof — limited

Previously demonstrated:
`Lindy 38368 / SiI9437 eARC -> Raspberry Pi 5 I2S slave -> IEC61937 type 0x15 @ 192 kHz -> raw E-AC-3 -> FFmpeg 5.1 PCM`.

This proves only DD+/E-AC-3 5.1 extraction/decoding in that tested setup.

### Not yet proven

- moving-object temporal behavior through Aurora's Harletty/Omniphony JOC path using the new official-source-derived carrier;
- authored object-position correctness through OpenJOC or Aurora rendering;
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
- Official Dolby moving-JOC reference lane: `validation/immersive/test-dolby-official-joc-temporal.sh`
- OpenJOC reference lane: `validation/immersive/test-openjoc-reference.sh`
- Realtime JOC pacing lane: `validation/immersive/test-joc-realtime-soak.sh`
- Immersive truth matrix: `validation/immersive/README.md`
- Main CI: `.github/workflows/ci.yml`
- Immersive JOC CI: `.github/workflows/immersive-joc-stack-ci.yml`
- Official Dolby temporal CI: `.github/workflows/dolby-joc-temporal-ci.yml`
- PR simulation smoke: `.github/workflows/simulation-assurance-pr.yml`
- Realtime soak: `.github/workflows/realtime-health-soak-ci.yml`
- External component/license decisions: `config/external-components-v1.json`, `THIRD_PARTY_LICENSES.md`

Useful search symbols:
`RealTimeEngine::new_with_prepared_components`, `RendererCapabilities`, `RealtimeDelayProcessor`, `PreparedComponentIdentity`, `PreparedRealtimeComponentSelection`, `ComponentReference`, `RendererComponentRegistry`, `BackendComponentRegistry`, `VIRTUAL_BACKEND_IMPLEMENTATION_ID`, `CPAL_BACKEND_IMPLEMENTATION_ID`, `OFFLINE_BACKEND_IMPLEMENTATION_ID`, `CURRENT_SCHEMA_VERSION`, `SourceManager`.

Legacy invariants: implementation-selection `RendererConfiguration` and `BackendIntent` enums must not reappear in current root config/runtime-control surfaces.

## 7. Current work / Next actions — moving JOC through Aurora's own path

The authorized/reproducible moving-object reference gate now has a positive software result. The next critical PC/software gate is to feed the same checksum-pinned official-source-derived JOC carrier through Aurora's actual Harletty/Omniphony path and prove that Aurora's own rendered output changes coherently over time before moving the critical path to physical hardware.

Next actions, in order:
1. Reuse the exact derived carrier identity from `test-dolby-official-joc-temporal.sh`; do not create another unpinned demo source.
2. Extend the Harletty/Omniphony validation lane to accept that carrier without weakening codec/JOC or temporal gates.
3. Capture Aurora-side timed object/metadata evidence and 12-channel output over the moving sections; require more than one meaningful rendered temporal profile.
4. Differentially compare Aurora-side timing/object census/render health against the independent OpenJOC evidence where semantics permit; do not treat channel-energy similarity as authored-position proof.
5. Add media-paced/realtime execution for the moving carrier and preserve underrun/overrun/timing evidence.
6. Only after Aurora's own moving-object path is positive should the critical path advance to physical continuous `eARC -> JOC -> Aurora -> 7.1.4` I/O validation.

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

For immersive/JOC validation changes, require the official `.github/workflows/immersive-joc-stack-ci.yml` PR run and inspect its emitted evidence. Changes to the moving-object carrier/reference lane additionally require `.github/workflows/dolby-joc-temporal-ci.yml` to pass and its artifact to preserve the untouched-carrier boundary plus the positive derived-suffix report. Synthetic analyzer tests prove only report/metric logic; they are never codec/JOC proof.

For performance/realtime changes also run relevant release benchmarks/allocation guards. For runtime/config PRs require official PR CI plus simulation smoke and sustained realtime-health soak when applicable.

Do not merge while any required gate is red. Delete temporary validation workflows before PR/merge.

## 9. Clean-development protocol

- `main-v2` is the only long-lived branch and source of truth.
- Use short-lived branches only when needed for isolation.
- Merge only green accepted work, preferably squash when a branch contains exploratory/debug commits.
- Delete the merged branch immediately when tooling permits; if branch-ref deletion is unavailable through the active connector, record that limitation instead of claiming deletion.
- PR/commit history preserves experiments; do not keep abandoned branches as alternate baselines.
- Keep this file current so the next agent can resume without chat history.