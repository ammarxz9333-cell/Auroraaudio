# Aurora Agent Reference — Living Handoff

> Read this file first. It is Aurora's canonical compact handoff for coding agents.
>
> **Mandatory maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge. Keep it factual and compact.

Last updated: **2026-09-12**

## 1. Repository state and branch policy

- **Single long-lived source of truth:** `main-v2`.
- Feature/debug branches may be created temporarily when isolation is useful, but must be deleted immediately after accepted work is merged when tooling permits.
- Never leave temporary GitHub Actions workflows or patch scripts on `main-v2` or in a PR diff.
- If the user says **“كمل” / “continue”**, continue the first unfinished item in **Current work / Next actions** below. Do not redo project discovery first.

Latest integrated architecture slice: **PR #131**, squash commit `0f37b6df1587d429587b74eac714309dc8299d34`.

Latest integrated validation-infrastructure slice: **PR #136**, squash commit `02e5495c9ad891372fca706c233940e35a2d9464`.

Latest integrated positive moving-object reference proof: **PR #140**, squash commit `5d6a164d6c4bad603aacebe9756e418a0d93ae26`.

Completed trackers: **#118**, **#119**, **#130**, **#132**, **#135**. Tracker **#141** is completed by the Aurora-side moving-JOC validation described below once PR #142 is merged.

## 2. Product goal

Aurora is an open, modular, hardware-agnostic immersive-audio stack, primarily Rust.

Long-term direction:
- realtime multichannel audio, initially 7.1.4-class and expandable toward 11.1.4;
- replaceable source, decoder, renderer, DSP, audio-I/O, transport, and hardware adapters;
- deterministic simulation and strong evidence gates;
- authorized TV/eARC ingestion and physical multichannel output;
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
- Failed replacement preparation leaves the active engine unchanged.
- Root renderer/input/output selection uses versioned `ComponentReference` and fail-closed registries rather than hard-coded implementation enums.
- Root config schema is **v3** with explicit deterministic v0/v1/v2 migration and canonical v3 fixtures/checksums.
- Runtime inspection schema is **v3** and reports exact prepared identities as control-plane intent.
- Software JOC/IEC61937 validation lanes use pinned external Harletty/Omniphony references.
- OpenJOC is an independent external fail-closed/differential reference lane.
- Temporal JOC evidence is fail-closed: codec/JOC validity, timed OAMD/object-state diversity, rendered 12-channel temporal-energy diversity, and pacing/health are reported separately.
- Authorized-carrier handling is checksum-pinned and provenance-aware; raw carriers are never silently re-encoded.
- Aurora now has a dedicated fail-closed moving-JOC lane through the pinned `IEC61937 -> Harletty bridge -> Omniphony 7.1.4` software path.

Key merged landmarks: #107, #108, #109-#112, #114, #117, #120, #121, #125, #126, #127, #128, #129, #131, #133, #136, #140.

### PR #131 / issue #119 completion evidence

PR #131 merged as `0f37b6df1587d429587b74eac714309dc8299d34` after all required gates passed, including Linux/Windows/MSRV CI, simulation assurance, sustained realtime-health soak, renderer evaluation, 3D VBAP evaluation, and generic 7.1.4 + output DSP.

### PR #133 / issue #132 temporal JOC evidence

PR #133 merged as `1445f0f29b9144dd2a958d6b57ff59285e439dec`.

The public Harletty `joc_atmos_1s.eac3` carrier passes codec/JOC admission/timing but deliberately fails the temporal proof with stable classification `insufficient_temporal_diversity`. CI requires that exact fail-closed result, preventing a static fixture from being misrepresented as moving-object proof.

The temporal contract separates:
1. codec/JOC admission and timing continuity;
2. timed OAMD/object-state diversity;
3. 12-channel rendered temporal-energy diversity;
4. realtime pacing/health evidence.

`render-joc` remains self-consistency evidence, **not** an independent oracle for original authored object-position correctness.

### PR #140 positive moving-object reference evidence

PR #140 merged as `5d6a164d6c4bad603aacebe9756e418a0d93ae26` and adds a permanent checksum-pinned lane using Dolby's publicly hosted Digital Plus Online Delivery Kit v1.4.1 carrier `Living-Room-Atmos_6ch_640kbps_ddp_joc.ec3`.

Truth-preserving boundary:
- ZIP SHA-256: `f94d5e3e933f756856686546763f42a8a5f16b10c264fc7af1d228acc09baa62`;
- untouched carrier SHA-256: `2470373db2c3621d56a2852df070e140293e9a99fdaa07e5c06de3c86bec307f`;
- pinned OpenJOC 0.17.0 identifies the untouched carrier as JOC and deployed-compatible, but classifies exactly AU0 with `MALFORMED_OAMD_METADATA` / `reserved OAMD object size index 3`;
- the positive lane removes exactly the first raw E-AC-3 AU (2560 bytes), performs no re-encoding, verifies byte identity, and pins the suffix SHA-256 `0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0`.

Positive reference evidence on the suffix:
- 2360 accepted AUs;
- 37,760 metadata updates and 15 dynamic object indices;
- OpenJOC 7.1.4 render: 12 channels, 48 kHz, ~75.520667 s;
- metadata and rendered temporal diversity both pass;
- no parser gate, threshold, or evidence requirement was weakened.

This is positive moving-object **reference/software evidence**. It is not Dolby conformance, authored-position correctness, DRM-service compatibility, or physical-output proof.

### PR #142 / issue #141 — Aurora-side moving JOC proof

PR #142 adds:
- `validation/immersive/aurora_joc_moving_evidence.py`;
- `validation/immersive/test-joc-aurora-moving.sh`;
- `.github/workflows/aurora-moving-joc-ci.yml`.

Final validated head before merge: `2dba5b4a0a31c0ee7d9fd74714bc4e93345b602b`.

Official final-head CI:
- `CI` run `34708144163` — **PASS**;
- `Immersive JOC Stack CI` run `34708144183` — **PASS**;
- `Aurora Moving JOC CI` run `34708144273`, job `103591852476` — **PASS**.

Emitted Aurora-side evidence on the exact PR #140 derived carrier SHA-256 `0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0`:
- IEC61937 data type `0x15`, 2360 packets/bursts;
- Harletty decode: 2360 frames, 48 kHz, 3,624,960 samples, zero resets, bridge ready, object state present;
- metadata: 2360 metadata frames, 35,400 object events, 15 object-channel declarations;
- all 15 object IDs 10..24 show non-zero position changes over time;
- metadata sample positions remain monotonic;
- Omniphony output: 12 channels, 48 kHz, 3,624,960 frames = 75.52 s;
- 303 analysis windows (250 ms), 300 non-silent windows, rendered temporal diversity = true;
- max normalized profile-L1 change from first = ~2.0 with fixed threshold 0.2;
- media-paced run: expected frames = actual frames = 3,624,960, elapsed 75.604572284 s for 75.52 s media, realtime factor `0.9988813866483852`, zero xrun/underrun/overrun markers, feeder exit 0, renderer exit 0;
- final analyzer verdict = `pass` with no failures.

This proves moving-object temporal behavior through Aurora's **actual pinned Harletty/Omniphony software path**, including media-paced 12-channel 7.1.4 output. OpenJOC remains a separate reference lane.

It still does **not** prove:
- authored object-position correctness against the original production authoring intent;
- Dolby certification/conformance;
- Netflix/other DRM-service Atmos compatibility;
- physical continuous eARC input/output;
- physical TDM/USB/DAC/amplifier/speaker behavior;
- acoustic parity with Samsung Q995-class systems.

### Historical hardware proof — limited

Previously demonstrated:
`Lindy 38368 / SiI9437 eARC -> Raspberry Pi 5 I2S slave -> IEC61937 type 0x15 @ 192 kHz -> raw E-AC-3 -> FFmpeg 5.1 PCM`.

This proves only DD+/E-AC-3 5.1 extraction/decoding in that tested setup. It is not yet the continuous physical JOC-to-7.1.4 proof.

### Not yet proven

- authored object-position correctness through OpenJOC or Aurora rendering;
- physical continuous `eARC -> E-AC-3 JOC -> Aurora -> physical 7.1.4`;
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

Stable prepared/component IDs:
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
- Runtime assembly/registries: `crates/aurora-runtime-assembly/src/`
- Runtime materialization: `crates/aurora-runtime-materialization/src/lib.rs`
- Runtime inspection/snapshots: `crates/aurora-runtime-inspection/`
- Source Manager: `crates/aurora-source-runtime/`
- Plugin API/host: `crates/aurora-plugin-api/`, `crates/aurora-plugin-host/`
- Simulation/acceptance: `crates/aurora-simulation-assurance/`, `crates/aurora-realtime-acceptance/`
- CLI/evaluation: `crates/aurora-cli/`
- Immersive/JOC evidence: `validation/immersive/`
- Temporal analyzer/harness: `validation/immersive/joc_temporal_evidence.py`, `validation/immersive/test-joc-temporal-evidence.sh`
- Official Dolby moving-JOC reference lane: `validation/immersive/test-dolby-official-joc-temporal.sh`
- Aurora moving-JOC lane: `validation/immersive/test-joc-aurora-moving.sh`, `validation/immersive/aurora_joc_moving_evidence.py`
- OpenJOC reference lane: `validation/immersive/test-openjoc-reference.sh`
- Realtime JOC pacing lane: `validation/immersive/test-joc-realtime-soak.sh`
- Main CI: `.github/workflows/ci.yml`
- Immersive JOC CI: `.github/workflows/immersive-joc-stack-ci.yml`
- Official Dolby temporal CI: `.github/workflows/dolby-joc-temporal-ci.yml`
- Aurora moving-JOC CI: `.github/workflows/aurora-moving-joc-ci.yml`
- External component/license decisions: `config/external-components-v1.json`, `THIRD_PARTY_LICENSES.md`

Useful search symbols:
`RealTimeEngine::new_with_prepared_components`, `RendererCapabilities`, `RealtimeDelayProcessor`, `PreparedComponentIdentity`, `PreparedRealtimeComponentSelection`, `ComponentReference`, `RendererComponentRegistry`, `BackendComponentRegistry`, `CURRENT_SCHEMA_VERSION`, `SourceManager`.

## 7. Current work / Next actions — physical continuous JOC path

The PC/software moving-object gate is now positive. The critical path advances to **physical continuous**:

`TV/player authorized output -> eARC/HDMI ingress -> IEC61937 E-AC-3 JOC -> Aurora Harletty/Omniphony -> physical 7.1.4 output`

Next actions, in order:
1. Select the smallest trustworthy physical ingress/output test chain without redesigning Aurora core around one board.
2. Reuse the same checksum-pinned official-source-derived carrier first; do not start with DRM-service debugging.
3. Prove uninterrupted physical eARC/IEC61937 `0x15` capture with JOC preserved end-to-end.
4. Feed the captured stream into the already-proven Aurora moving-JOC software path without re-encoding.
5. Drive a real 12-channel output device/DAC path and capture continuity, channel count, sample rate, frame count, xruns, clock/drift, and measured loopback latency where hardware permits.
6. Only after the physical carrier path is green, test legitimate Netflix/other service Atmos compatibility separately; keep DRM-service behavior distinct from core decoder/render evidence.
7. After physical 7.1.4 stability, proceed to final DAC/amplifier/speaker architecture and then wireless speaker-network validation.

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

For immersive/JOC changes require the official Immersive JOC Stack PR run and inspect emitted evidence. Changes to the moving-object carrier/reference lane additionally require the Official Dolby JOC Temporal CI. Changes to Aurora's moving-object path require the Aurora Moving JOC CI. Synthetic analyzer tests prove only report/metric logic; they are never codec/JOC proof.

For performance/realtime changes also run relevant release benchmarks/allocation guards. For runtime/config PRs require official PR CI plus simulation smoke and sustained realtime-health soak when applicable.

Do not merge while any required gate is red.

## 9. Clean-development protocol

- `main-v2` is the only long-lived branch and source of truth.
- Use short-lived branches only when needed for isolation.
- Merge only green accepted work, preferably squash when a branch contains exploratory/debug commits.
- Delete the merged branch immediately when tooling permits; if branch-ref deletion is unavailable through the active connector, record that limitation instead of claiming deletion.
- PR/commit history preserves experiments; do not keep abandoned branches as alternate baselines.
- Keep this file current so the next agent can resume without chat history.
