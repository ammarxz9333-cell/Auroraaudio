# Aurora Agent Reference — Living Handoff

> Read this file first. It is Aurora's canonical compact handoff for coding agents.
>
> **Mandatory maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge. Keep it factual and compact.

Last updated: **2026-09-12**

## 1. Repository state and branch policy

- **Single long-lived source of truth:** `main-v2`.
- Temporary feature/debug branches are allowed for isolation; delete them after accepted merge when tooling permits.
- Never leave temporary validation workflows or patch scripts on `main-v2`.
- If the user says **“كمل” / “continue”**, continue the first unfinished item in **Current work / Next actions** below. Do not redo project discovery first.

Latest integrated architecture slice: **PR #131**, squash commit `0f37b6df1587d429587b74eac714309dc8299d34`.

Latest integrated validation-infrastructure slice: **PR #136**, squash commit `02e5495c9ad891372fca706c233940e35a2d9464`.

Latest positive moving-object reference proof: **PR #140**, squash commit `5d6a164d6c4bad603aacebe9756e418a0d93ae26`.

Latest Aurora-side moving-JOC proof: **PR #142**, squash commit `9b153e1d34754aa47672fe2a6e5d46fb0e0966dc`.

Latest full-system virtual-hardware proof: **PR #146**, squash commit `2d53aec6b7180a1480b781b23680098f40f23cf0`.

Latest native Windows full-system proof: **PR #148**, squash commit `44746431975c65126c88b31ce253fc82c737cce7`.

Physical Gate A admission tooling is being added in **PR #149**; its CI is tooling evidence only until a real Lindy/SiI9437 -> Pi capture is supplied.

Completed trackers: **#118**, **#119**, **#130**, **#132**, **#135**, **#141**, **#145**, **#147**.

Active physical critical-path tracker: **#143** — physical continuous eARC/JOC to synchronous 7.1.4 output.

## 2. Product goal

Aurora is an open, modular, hardware-agnostic immersive-audio stack, primarily Rust.

Long-term direction:
- realtime multichannel audio, initially 7.1.4 and expandable toward 11.1.4;
- replaceable source, decoder, renderer, DSP, audio-I/O, transport, and hardware adapters;
- authorized TV/eARC ingestion and physical multichannel output;
- later private wireless speaker/rear/multi-room transport;
- no specific SBC, MCU, DAC, eARC board, speaker product, AVR, soundbar, or OS image may define the core architecture.

## 3. Non-negotiable rules

- Core remains hardware-agnostic; device work enters through narrow Aurora-owned adapters.
- Realtime callback: no allocation after preparation, locks, logging/formatting, filesystem/process access, config parsing/registry lookup, or silent device changes.
- Prepare -> validate -> commit is transactional; failed candidates must not mutate active state.
- Unknown/incompatible component IDs, schemas, contracts, capabilities, or generations fail closed.
- Object decoding, channel decoding, and synthetic upmixing are distinct. Never silently substitute one for another.
- Configuration/simulation is not runtime or physical evidence.
- Never call configured/simulated/estimated latency “measured”; physical loopback is required.
- No DRM circumvention, protected-media extraction, Widevine bypass, or device-identity spoofing.
- No Dolby certification/proprietary-streaming claim from software CI.
- GPL/incompatible reference implementations remain external.

## 4. Evidence truth

### Proven/merged software behavior

- Hardware-agnostic Rust workspace with Linux/Windows/MSRV CI.
- Deterministic simulation assurance and sustained accelerated realtime-health soak.
- Prepared renderer/DSP/backend identities and fail-closed registries through the common realtime engine boundary.
- Root config/runtime selection uses versioned `ComponentReference`; root config schema and runtime inspection schema are v3.
- Software JOC/IEC61937 validation uses pinned Harletty/Omniphony references.
- OpenJOC remains a separate external fail-closed/differential reference lane.
- Authorized-carrier handling is checksum-pinned and provenance-aware; raw carriers are never silently re-encoded.
- Temporal JOC evidence separates codec admission, timed object metadata, rendered 12-channel diversity, and pacing/health.
- A deterministic laptop-only virtual-hardware gate now extends the proven moving-JOC output through a synchronous 16-slot virtual transport and fail-closed device fault profiles.
- A native Windows launcher now reproduces the pinned Aurora moving-JOC + virtual-hardware functional path without Simics/QSP/WSL; the independent OpenJOC lane remains Linux-only reference evidence.

Key merged landmarks: #107, #108, #109-#112, #114, #117, #120, #121, #125, #126, #127, #128, #129, #131, #133, #136, #140, #142, #146, #148.

### PR #140 — positive moving-object reference

Uses Dolby's publicly hosted Digital Plus Online Delivery Kit v1.4.1 carrier `Living-Room-Atmos_6ch_640kbps_ddp_joc.ec3`.

Pinned hashes:
- ZIP: `f94d5e3e933f756856686546763f42a8a5f16b10c264fc7af1d228acc09baa62`
- untouched carrier: `2470373db2c3621d56a2852df070e140293e9a99fdaa07e5c06de3c86bec307f`
- byte-identical suffix after dropping exactly AU0: `0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0`

OpenJOC 0.17.0 identifies the untouched carrier as JOC/deployed-compatible but classifies exactly AU0 as `MALFORMED_OAMD_METADATA` / `reserved OAMD object size index 3`. Aurora does not hide that boundary. The positive lane removes exactly that 2560-byte AU with no re-encode.

Positive suffix evidence:
- 2360 accepted AUs;
- 37,760 metadata updates, 15 dynamic object indices;
- OpenJOC 12-channel 48 kHz render for ~75.520667 s;
- metadata and rendered temporal diversity pass.

This is reference/software evidence only, not Dolby conformance, authored-position correctness, DRM-service compatibility, or physical-output proof.

### PR #142 / issue #141 — Aurora moving-JOC software path

PR #142 merged as `9b153e1d34754aa47672fe2a6e5d46fb0e0966dc`.

Final-head CI:
- `CI` run `34709483752` — **PASS**;
- `Immersive JOC Stack CI` run `34709483748` — **PASS**;
- `Aurora Moving JOC CI` run `34709483759` — **PASS**.

Aurora-side evidence using the exact derived SHA above:
- IEC61937 type `0x15`, 2360 bursts;
- Harletty: 2360 frames, 48 kHz, 3,624,960 samples, zero resets;
- 2360 metadata frames, 35,400 object events, 15 object-channel declarations;
- object IDs 10..24 all show non-zero position changes;
- Omniphony: 12 channels, 48 kHz, 3,624,960 frames = 75.52 s;
- rendered temporal diversity = true;
- paced run: expected frames = actual frames = 3,624,960; elapsed 75.604572284 s for 75.52 s media; realtime factor `0.9988813866483852`; zero xrun/underrun/overrun markers; feeder/renderer exit 0;
- final analyzer verdict = `pass`.

This proves moving-object JOC behavior through Aurora's pinned Harletty/Omniphony **software** path. It does not prove physical eARC/DAC output, streaming-service compatibility, authored-position correctness, Dolby certification, or acoustic parity.

### PR #146 / issue #145 — full-system virtual hardware lab

PR #146 merged as `2d53aec6b7180a1480b781b23680098f40f23cf0` and adds `validation/virtual-hardware/`, `docs/aurora-full-system-sim.md`, and dedicated `Aurora Full-System Sim CI`.

Final validated head: `768c481e8dd751e3eb3d2c9af1b16de5085b372a`.

Final-head CI:
- `CI` run `34715335037` — **PASS**;
- `Aurora Full-System Sim CI` run `34715335081` — **PASS**.

Healthy path:
`pinned moving JOC -> IEC61937 -> Harletty -> Omniphony 7.1.4 -> paced 12ch/48 kHz PCM -> deterministic virtual TDM16/DAC sink`.

Healthy emitted evidence:
- expected/actual source frames: `3,624,960 / 3,624,960`;
- virtual sink frames: `3,624,960`, dropped frames `0`, virtual xruns `0`, disconnect `false`;
- all 12 output channel indices 0..11 active;
- one synchronous virtual clock domain; drift `0 ppm`;
- source PCM SHA-256 = virtual sink PCM SHA-256 = `904ec61978e60f4418893188e684b1f32ef25d3826cd00af2b4a187a2f5ccaef`;
- virtual TDM16 SHA-256 = `5e90e42271ff2173773e2399b6b64ab0599fafda5a0fa809336e06a058f0fda9`;
- slots 0..11 assigned in order; 12..15 unused/zero;
- configured virtual latency = 256 frames = 5.333 ms, explicitly **simulated, not measured**;
- healthy verdict `pass`, failures `[]`.

Fail-closed profiles all behaved as required:
- `dropout`: failed with sink-frame mismatch and 75 virtual xruns;
- `channel-silence`: failed with inactive output channel 11;
- `disconnect`: failed with half-stream frame mismatch and virtual disconnect;
- `drift`: failed at simulated +250 ppm.

This proves Aurora's tested software path plus the deterministic virtual output model on a laptop. It does **not** prove physical eARC, physical UAC2/TDM timing/electrical behavior, DAC/amplifier/speaker behavior, physically measured latency/drift, DRM-service compatibility, Dolby certification, or acoustic parity.

### PR #148 / issue #147 — native Windows AuroraSim launcher

PR #148 merged as `44746431975c65126c88b31ce253fc82c737cce7`. Final validated head: `f6fdd20523c319f350d149514f649cc3e2605074`.

Final-head CI:
- `CI` run `34716256141` — **PASS**;
- `Aurora Full-System Sim Windows CI` run `34716256185` — **PASS**;
- `Aurora Full-System Sim CI` run `34716256159` — **PASS**.

Native Windows path:
`official checksum-pinned carrier -> exact byte-identical suffix -> IEC61937 0x15 -> pinned Harletty Windows DLL -> pinned+patched Omniphony 7.1.4 -> paced 12ch PCM -> deterministic virtual TDM16/DAC sink`.

Windows evidence:
- carrier SHA-256 expected/actual `0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0`;
- Harletty: 2360 packets/frames, 48 kHz, 3,624,960 samples, zero resets, 2360 metadata frames, 35,400 object events, 15 object-channel declarations;
- 15 object IDs are position-varying;
- paced render: expected/actual frames `3,624,960 / 3,624,960`, media 75.52 s, elapsed `75.5579752 s`, realtime factor `0.9994974031543353`, zero xrun markers, feeder/renderer exit 0;
- virtual sink: `3,624,960` frames, 12/12 channels active, order preserved, dropped frames `0`, virtual xruns `0`, disconnect `false`;
- source/sink PCM SHA-256 = `4a8d413bf4f196583bb777b273a1ef94c119ac5c0c23ee7ad7ae98f3965867a8`;
- virtual TDM16 SHA-256 = `237fc3566fa7f3fef51caf6dfbfb9fe466ac1694c3fe08630252d8e77225a914`;
- fail-closed faults: `dropout` -> 75 xruns + frame mismatch; `channel-silence` -> inactive channel 11; `disconnect` -> half-stream frame mismatch + disconnect; `drift` -> +250 ppm failure.

Run locally on a native Windows checkout with:
```powershell
.\validation\virtual-hardware\run-aurora-sim-windows.ps1
```

This Windows lane is functional/regression evidence only. It does not replace the independent OpenJOC Linux reference lane and does not prove physical eARC/UAC2/TDM/DAC behavior, Dolby certification, or acoustic parity.

### PR #149 — physical Gate A admission tooling (pending physical evidence)

PR #149 adds `validation/physical/aurora_physical_ingress.py` and dedicated Linux/Windows tooling CI. The validator admits only an exact physical IEC61937 E-AC-3 capture for issue #143: data type `0x15`, fixed 24576-byte burst grid, zero transport padding, no Pc error flag, exact 16-bit payload reconstruction, explicit reset/drop counters, and optional required monotonic capture timestamps. For the first physical test, the reconstructed raw E-AC-3 SHA-256 must equal `0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0` over 2360 bursts.

The synthetic self-tests deliberately inject wrong type, payload mutation, non-zero padding, byte slip, and reset-counter failure and require fail-closed behavior. Passing that tooling CI is **not** a physical result. Gate A remains unproven until the real Lindy/SiI9437 -> Pi capture is supplied and passes the validator with real capture metadata.

### Historical physical ingress proof — limited

Previously demonstrated:
`Lindy 38368 / SiI9437 eARC -> Raspberry Pi 5 I2S slave -> IEC61937 type 0x15 @ 192 kHz -> raw E-AC-3 -> FFmpeg 5.1 PCM`.

This proves only DD+/E-AC-3 extraction/5.1 decoding in that tested setup. The logic-level tap is project-specific; it is not a stock Lindy connector or vendor-supported I2S output.

### Selected physical validation chain for #143

Documented in `docs/physical-joc-validation-v1.md`.

First-choice chain:
`authorized TV/player -> Lindy 38368 / existing SiI9437 logic tap -> Raspberry Pi 5 I2S capture -> Aurora Harletty/Omniphony -> USB UAC2 -> miniDSP MCHStreamer Lite TDM16 @ 48 kHz -> two synchronized 8-channel TDM DAC stages -> 12 used physical outputs`

Output selection facts:
- MCHStreamer Lite is one USB audio clock domain.
- In documented TDM16 mode, J1 pin 1 carries output channels 1-8, J1 pin 3 carries 9-16; J1 pins 9/10/11/12 provide MCLK/BCLK/GND/FSYNC.
- Use two shared-clock 8-channel TDM DAC stages, preferably PCM3168A-class for the first proof.
- Only slots 1-12 are required for 7.1.4; 13-16 remain unused.
- Amplifiers/speakers are intentionally outside the first electrical gate.
- If Pi 5 cannot sustain the full renderer, move only host compute to the existing N100 Linux target; do not redesign Aurora core.

### Not yet proven

- physical continuous `eARC -> E-AC-3 JOC -> Aurora -> synchronous physical 7.1.4`;
- physical Gate A exact pinned-carrier ingress under the new validator;
- authored object-position correctness;
- Netflix/other DRM-service Atmos compatibility through Aurora;
- exact final DAC/amplifier/speaker design;
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

Stable built-in IDs include:
- `org.aurora.renderer.basic`
- `org.aurora.renderer.vbap`
- `org.aurora.dsp.basic-delay`
- `org.aurora.backend.virtual`
- `org.aurora.backend.cpal`
- `org.aurora.backend.offline`

## 6. Fast repository map

- Core/layouts: `crates/aurora-core/`, `crates/aurora-scene/`
- Renderers: `crates/aurora-renderer-api/`, `aurora-renderer-basic/`, `aurora-renderer-vbap/`
- Decoders: `crates/aurora-decoder-api/`, `aurora-decoder-*`
- DSP: `crates/aurora-dsp-api/`, `aurora-dsp-basic/`, `aurora-dsp-camilladsp/`
- Audio I/O: `crates/aurora-audio-io/`, `crates/aurora-realtime-audio-*`
- Realtime engine: `crates/aurora-realtime-engine/`
- Config/runtime: `crates/aurora-config/`, `aurora-runtime-assembly/`, `aurora-runtime-materialization/`, `aurora-runtime-inspection/`
- Source/plugin: `crates/aurora-source-runtime/`, `aurora-plugin-api/`, `aurora-plugin-host/`
- Simulation/acceptance: `crates/aurora-simulation-assurance/`, `aurora-realtime-acceptance/`
- Immersive/JOC validation: `validation/immersive/`
- Moving reference: `validation/immersive/test-dolby-official-joc-temporal.sh`
- Aurora moving path: `validation/immersive/test-joc-aurora-moving.sh`, `aurora_joc_moving_evidence.py`
- Full-system virtual hardware: `validation/virtual-hardware/`, `docs/aurora-full-system-sim.md`
- Native Windows launcher: `validation/virtual-hardware/run-aurora-sim-windows.ps1`, `run_aurora_sim_windows.py`, `pace_orender.py`, `aurora_moving_telemetry.rs`
- Physical Gate A tooling: `validation/physical/aurora_physical_ingress.py`
- Physical v1 plan: `docs/physical-joc-validation-v1.md`
- External components/licenses: `config/external-components-v1.json`, `THIRD_PARTY_LICENSES.md`

## 7. Current work / Next actions — issue #143 physical continuous JOC

The laptop-only full-system simulator (#145/#146) and native Windows launcher (#147/#148) are complete and are now pre-hardware regression gates. They do not replace physical acceptance.

**Selection step is complete.** The first hardware chain is frozen in `docs/physical-joc-validation-v1.md`; do not reopen board selection unless the chosen hardware fails its explicit stop conditions.

Next physical actions, in order:
1. Assemble/reuse the existing Lindy 38368 / SiI9437 -> Pi 5 physical ingress and play the exact pinned carrier from #140/#142.
2. Capture the complete raw physical IEC61937 byte stream and run `validation/physical/aurora_physical_ingress.py analyze` with `--require-capture-metadata`; require 2360 type-`0x15` bursts, exact 24576-byte burst-grid continuity, zero resets/drops/padding mutation, and reconstructed SHA-256 `0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0`.
3. Feed the accepted physical capture into the unchanged Aurora moving-JOC path and require both the existing moving-JOC gates and the full-system Linux/Windows regression gates to remain green.
4. Attach one MCHStreamer Lite in TDM16 @ 48 kHz and verify one synchronous 16-slot output clock domain before attaching DACs.
5. Attach two synchronized 8-channel TDM DAC stages and verify electrical activity/channel mapping on all 12 used outputs.
6. Record negotiated ALSA device identity/format, buffer/period settings, expected vs actual frames, xruns, CPU load, clock/drift observations, and physical loopback latency where a return path exists.
7. Close #143 only after one full-duration run proves `physical eARC capture -> raw JOC preservation -> Aurora moving-object render -> synchronous physical 12-channel output`.
8. Only then test legitimate Netflix/other service Atmos separately; after that proceed to final DAC/amplifier/speaker architecture and wireless transport.

Other queued trackers:
- #115: evaluate newer Omniphony behind a separate reference lane; do not upgrade the stable pin by recency alone.
- #116: reassess Source Manager acceptance before closing.

## 8. Required validation before merge

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

For immersive/JOC changes require the official Immersive JOC Stack PR run and inspect emitted evidence. Moving-reference changes additionally require Official Dolby JOC Temporal CI; Aurora moving-path changes require Aurora Moving JOC CI. Full-system virtual-hardware changes require `Aurora Full-System Sim CI`, including healthy evidence plus all fail-closed fault profiles. Native Windows launcher changes additionally require `Aurora Full-System Sim Windows CI`. Physical ingress tooling changes require `Aurora Physical Ingress Tooling CI` on Linux and Windows; this is tooling validation only and must never be reported as physical evidence. Hardware documentation or virtual hardware evidence alone is not physical proof.

Do not merge while required gates are red.

## 9. Clean-development protocol

- `main-v2` is the only long-lived branch/source of truth.
- Merge only green accepted work, preferably squash for exploratory branches.
- Delete merged branches when tooling permits; if branch-ref deletion is unavailable, record the limitation instead of claiming deletion.
- PR/commit history preserves experiments; do not keep abandoned branches as alternate baselines.
- Keep this file current so the next agent can resume without chat history.
