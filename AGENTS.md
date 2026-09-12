# Aurora Agent Reference — Living Handoff

> Read this file first. It is Aurora's canonical compact handoff for coding agents.
>
> **Mandatory maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge. Keep it factual and compact.

Last updated: **2026-09-12**

## 1. Repository state and branch policy

- **Single long-lived source of truth:** `main-v2`.
- Temporary feature/debug branches are allowed for isolation; delete them after accepted merge when tooling permits.
- Never leave abandoned experiments as alternate baselines.
- If the user says **“كمل” / “continue”**, continue the first unfinished item in **Current work / Next actions** below. Do not redo project discovery first.

Latest merged milestones:
- architecture/runtime/config: PR #131, `0f37b6df1587d429587b74eac714309dc8299d34`;
- authorized carrier runner: PR #136, `02e5495c9ad891372fca706c233940e35a2d9464`;
- positive moving-object reference: PR #140, `5d6a164d6c4bad603aacebe9756e418a0d93ae26`;
- Aurora moving-JOC software path: PR #142, `9b153e1d34754aa47672fe2a6e5d46fb0e0966dc`;
- full-system virtual hardware lab: PR #146, `2d53aec6b7180a1480b781b23680098f40f23cf0`;
- native Windows AuroraSim: PR #148, `44746431975c65126c88b31ce253fc82c737cce7`;
- physical Gate A IEC61937 validator: PR #149, `9c380c64818057e10b0b488ecba4129efe86de56`;
- hardware-neutral ALSA encoded-ingress capture adapter: PR #150, `1afe66ab40ab2c7391610d5e5def27b6ba426644`.

In-flight hardening:
- PR #152, `world-class-sim-coverage`: mandatory simulation coverage contract, real Aurora output-DSP in the full-system path, expanded fail-closed virtual fault model, Linux/Windows parity. Do not call this accepted evidence until all required checks are green and the PR is merged.

Completed trackers include #118, #119, #130, #132, #135, #141, #145, #147.

**Active physical critical-path tracker: #143** — physical continuous eARC/JOC to synchronous 7.1.4 output.

## 2. Product goal and non-negotiable rules

Aurora is an open, modular, hardware-agnostic immersive-audio stack, primarily Rust.

Direction:
- realtime multichannel audio, initially 7.1.4 and expandable toward 11.1.4;
- replaceable source, decoder, renderer, DSP, audio-I/O, transport and hardware adapters;
- authorized TV/eARC ingress and physical multichannel output;
- later private wireless speaker/rear/multi-room transport;
- no SBC, MCU, DAC, eARC board, speaker product, AVR, soundbar or OS image may define core architecture.

Rules:
- realtime callback: no allocation after preparation, locks, logging/formatting, filesystem/process access, config parsing/registry lookup, or silent device changes;
- prepare -> validate -> commit is transactional; failed candidates do not mutate active state;
- unknown/incompatible component IDs, schemas, contracts, capabilities or generations fail closed;
- object decoding, channel decoding and synthetic upmixing are distinct; never silently substitute one for another;
- configuration/simulation is not runtime or physical evidence;
- only physical loopback may be called measured latency;
- no DRM/Widevine circumvention, protected-media extraction or device-identity spoofing;
- no Dolby certification/conformance claim from software CI;
- GPL/incompatible references remain external;
- every new or materially changed simulator-testable capability must be declared in `config/simulation-coverage-v1.json` in the same change; a virtual capability may be marked `covered` only when it maps to an exported AuroraSim capability plus executable healthy/fault profile evidence, while unimplemented gaps remain explicit as `planned`/pending.

## 3. Evidence truth

### Pinned moving JOC carrier

Dolby Digital Plus Online Delivery Kit v1.4.1 source: `Living-Room-Atmos_6ch_640kbps_ddp_joc.ec3`.

Pinned hashes:
- ZIP: `f94d5e3e933f756856686546763f42a8a5f16b10c264fc7af1d228acc09baa62`;
- untouched carrier: `2470373db2c3621d56a2852df070e140293e9a99fdaa07e5c06de3c86bec307f`;
- byte-identical suffix after dropping exactly malformed AU0: `0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0`.

OpenJOC 0.17.0 classifies AU0 as `MALFORMED_OAMD_METADATA` / reserved OAMD object-size index 3. Aurora does not hide this. The positive lane removes exactly that 2560-byte AU without re-encoding.

Reference evidence: 2360 accepted AUs, 37,760 metadata updates, 15 dynamic object indices, 12-channel/48 kHz render for ~75.52 s with temporal diversity.

### PR #142 — Aurora moving-JOC software proof

Using the exact derived SHA above:
- IEC61937 type `0x15`, 2360 bursts;
- Harletty: 2360 frames, 48 kHz, 3,624,960 samples, zero resets;
- 2360 metadata frames, 35,400 object events, object IDs 10..24 position-varying;
- Omniphony: 12 channels, 48 kHz, 3,624,960 frames = 75.52 s;
- paced elapsed 75.604572284 s; realtime factor `0.9988813866483852`;
- zero xrun/underrun/overrun markers; final verdict `pass`.

This is software evidence only.

### PR #146 — full-system virtual hardware lab

Healthy path:
`pinned moving JOC -> IEC61937 -> Harletty -> Omniphony 7.1.4 -> paced 12ch/48 kHz PCM -> deterministic virtual TDM16/DAC sink`.

Healthy evidence:
- 3,624,960 source/sink frames, 0 dropped, 0 virtual xruns;
- all 12 channels active;
- synchronous virtual clock, 0 ppm drift;
- slots 0..11 assigned; 12..15 unused/zero;
- configured virtual latency 256 frames = 5.333 ms, simulated not measured;
- fail-closed profiles: dropout, channel-silence, disconnect and +250 ppm drift all fail as expected.

This does not prove physical eARC/UAC2/TDM/DAC behavior.

### PR #148 — native Windows AuroraSim

Final-head CI:
- `CI` run `34716256141` — PASS;
- `Aurora Full-System Sim Windows CI` run `34716256185` — PASS;
- `Aurora Full-System Sim CI` run `34716256159` — PASS.

Windows evidence:
- exact pinned moving carrier identity;
- Harletty 2360 packets/frames, 48 kHz, 3,624,960 samples, zero resets;
- 15 position-varying objects;
- paced expected/actual frames `3,624,960 / 3,624,960`;
- elapsed `75.5579752 s`, realtime factor `0.9994974031543353`;
- zero xruns; 12/12 channels active;
- fault profiles fail closed.

Run locally on native Windows:
```powershell
.\validation\virtual-hardware\run-aurora-sim-windows.ps1
```

### PR #152 — AuroraSim v2 hardening (in flight; not accepted evidence yet)

Proposed healthy path:
`pinned moving JOC -> IEC61937 -> Harletty -> Omniphony 7.1.4 -> media-paced PCM -> real Aurora SpeakerPostProcessor -> deterministic virtual TDM16/DAC sink`.

Proposed mandatory simulation policy:
- `config/simulation-coverage-v1.json` is the machine-readable coverage contract;
- `validation/virtual-hardware/validate_simulation_coverage.py` fails CI for undeclared simulator capabilities, missing evidence, duplicate mappings, or unmapped fault profiles;
- healthy transport computes per-channel SHA-256 and full PCM identity instead of asserting channel order by declaration;
- negative profiles cover dropout, channel silence, channel swap, device disconnect, excessive drift, sample-rate change, latency spike, non-finite PCM and TDM padding corruption;
- Linux and native Windows run the same real Aurora output-DSP boundary before virtual transport analysis;
- IAMF/OAR, ADM/BS.2127 differential rendering, binaural/head tracking, adaptive runtime clock correction, reconnect recovery and physical/external acceptance remain explicit non-covered gaps until separately proven.

No PR #152 result may be promoted to evidence truth until required CI is green and the PR is merged.

### PR #149 — physical Gate A admission validator

Merged file: `validation/physical/aurora_physical_ingress.py`.

Tooling CI:
- `Aurora Physical Ingress Tooling CI` run `34717647820` — PASS Ubuntu/Windows;
- base `CI` run `34717647839` — PASS MSRV/Linux/Windows.

For the first physical carrier it requires:
- IEC61937 E-AC-3 data type `0x15`;
- exact 24576-byte burst grid;
- 2360 bursts;
- no Pc error flag;
- zero transport padding mutation;
- exact 16-bit payload reconstruction;
- explicit monotonic capture timestamps when required;
- explicit reset/drop counters, both zero;
- reconstructed E-AC-3 SHA-256 `0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0`.

Synthetic self-tests inject wrong type, payload mutation, nonzero padding, byte slip and reset failure. CI validates the tool only; physical Gate A is still unproven.

### PR #150 — hardware-neutral ALSA capture adapter

Merged files:
- `validation/physical/aurora_alsa_iec61937_capture.py`;
- `docs/physical-alsa-capture.md`.

Tooling CI:
- `Aurora Physical Ingress Tooling CI` run `34718156830` — PASS Ubuntu/Windows;
- base `CI` run `34718156862` — PASS MSRV/Linux/Windows.

Purpose: bridge a real Linux ALSA capture into the canonical IEC61937 bytes consumed by PR #149 without inventing hardware details.

Capture contract:
- operator explicitly selects the ALSA PCM device;
- first supported probe is `S32_LE`, 2 channels, 192000 Hz;
- preserves raw ALSA bytes, hw-parameter log, stderr log and monotonic start/end timestamps;
- converts one 16-bit IEC word from each 32-bit ALSA sample slot;
- fail-closed auto-detection only across high16/low16 x LR/RL;
- candidate selection requires a fixed 24576-byte IEC61937 burst train;
- `arecord` xrun markers are diagnostic only;
- hardware reset/drop counters are never fabricated; if unavailable, Gate A remains incomplete.

Self-tests round-trip all four supported slot/channel interpretations and require sync-free input to fail closed.

### Historical physical ingress proof — limited

Previously demonstrated:
`Lindy 38368 / SiI9437 eARC -> Raspberry Pi 5 I2S slave -> IEC61937 type 0x15 @ 192 kHz -> raw E-AC-3 -> FFmpeg 5.1 PCM`.

This proves only DD+/E-AC-3 extraction/5.1 decoding in that tested setup. The logic-level tap is project-specific; it is not a stock Lindy connector or vendor-supported I2S output.

### Not yet proven

- physical continuous `eARC -> E-AC-3 JOC -> Aurora -> synchronous physical 7.1.4`;
- a real #143 Gate A capture under the merged validator;
- authored object-position correctness;
- IAMF/OAR decode/render integration;
- ADM/BS.2127 differential rendering against EAR/libear;
- binaural/head-tracked rendering validation;
- adaptive runtime clock-rate correction/ASRC;
- controlled output-device reconnect recovery;
- Netflix/other DRM-service Atmos compatibility through Aurora;
- final DAC/amplifier/speaker design;
- acoustic parity with Samsung Q995-class systems;
- private wireless speaker-network latency/sync/reliability;
- thermal/EMI/production readiness;
- Dolby certification.

## 4. Selected physical validation chain for #143

Frozen first-choice chain, documented in `docs/physical-joc-validation-v1.md`:

`authorized TV/player -> existing Lindy 38368 / SiI9437 logic tap -> Raspberry Pi 5 I2S capture -> Aurora Harletty/Omniphony -> USB UAC2 -> miniDSP MCHStreamer Lite TDM16 @ 48 kHz -> two synchronized 8-channel TDM DAC stages -> 12 used physical outputs`

Output principles:
- one MCHStreamer Lite = one USB audio clock domain;
- TDM16 provides enough synchronous slots for 7.1.4;
- use two shared-clock 8-channel DAC stages, preferably PCM3168A-class for first proof;
- only first 12 outputs are required; 13-16 stay unused;
- amps/speakers are outside the first electrical gate;
- if Pi 5 cannot sustain rendering, move host compute only to the N100 Linux target; do not redesign Aurora core.

## 5. Current architecture / repository map

Control plane:
`aurora-config -> aurora-runtime-assembly -> aurora-runtime-inspection -> aurora-runtime-materialization -> prepared components -> aurora-realtime-engine`

Media direction:
`source/input -> decoder API -> scene/audio -> renderer API -> DSP API -> generic realtime audio I/O`

Key locations:
- core/layouts: `crates/aurora-core/`, `crates/aurora-scene/`;
- renderers: `crates/aurora-renderer-api/`, `aurora-renderer-basic/`, `aurora-renderer-vbap/`;
- decoders: `crates/aurora-decoder-api/`, `aurora-decoder-*`;
- DSP: `crates/aurora-dsp-api/`, `aurora-dsp-basic/`, `aurora-dsp-camilladsp/`;
- audio I/O: `crates/aurora-audio-io/`, `crates/aurora-realtime-audio-*`;
- realtime engine: `crates/aurora-realtime-engine/`;
- config/runtime: `crates/aurora-config/`, `aurora-runtime-assembly/`, `aurora-runtime-materialization/`, `aurora-runtime-inspection/`;
- immersive/JOC validation: `validation/immersive/`;
- full-system virtual hardware: `validation/virtual-hardware/`, `docs/aurora-full-system-sim.md`;
- mandatory simulation coverage: `config/simulation-coverage-v1.json`, `validation/virtual-hardware/validate_simulation_coverage.py`;
- physical Gate A: `validation/physical/aurora_physical_ingress.py`;
- ALSA capture adapter: `validation/physical/aurora_alsa_iec61937_capture.py`, `docs/physical-alsa-capture.md`;
- physical chain: `docs/physical-joc-validation-v1.md`;
- external components/licenses: `config/external-components-v1.json`, `THIRD_PARTY_LICENSES.md`.

## 6. Current work / Next actions — issue #143 + requested world-class hardening

The physical #143 chain remains the acceptance critical path. In parallel, the user explicitly requested continuous comparison against leading immersive-audio platforms/projects and a simulator that cannot silently omit new Aurora capabilities. PR #152 is the current non-physical hardening item and must finish green before returning to the hardware-only #143 steps.

Next actions, in order:
1. Finish PR #152: require Linux + Windows full-system simulation, base CI and relevant immersive/JOC gates green; fix any harness/regression failure rather than weakening gates.
2. After #152 merges, evaluate the newly identified AOMedia Open Audio Renderer (OAR) as an external IAMF/open-rendering reference lane. Do not replace the stable JOC renderer by recency alone.
3. Add independent ADM/BS.2127 differential-reference coverage with EBU EAR/libear/BEAR where licensing and interfaces permit; keep GPL/incompatible code external.
4. Only promote IAMF/OAR, ADM, binaural/head-tracking, adaptive clock correction or reconnect recovery from `planned` when executable evidence exists and the simulator/coverage contract is updated in the same change.
5. Resume physical #143: assemble/reuse the existing Lindy 38368 / SiI9437 -> Pi 5 I2S capture path.
6. On the real Linux capture host run `arecord -l`; identify the actual capture PCM. Do not invent or hard-code a device name before this step.
7. Play the exact pinned carrier and run `validation/physical/aurora_alsa_iec61937_capture.py capture` with the real PCM device. Preserve raw ALSA capture, `arecord` hw params/stderr, monotonic timestamps and canonical `.spdif` output.
8. Obtain trustworthy reset/drop counters from the real capture driver/adapter. `arecord` xrun text alone is diagnostic and does not substitute for these counters.
9. Run `validation/physical/aurora_physical_ingress.py analyze --require-capture-metadata`; require 2360 type-`0x15` bursts, exact 24576-byte grid, zero resets/drops/padding mutation and reconstructed pinned SHA.
10. Feed the accepted physical capture into the unchanged Aurora moving-JOC path and require moving-JOC plus full-system Linux/Windows gates to remain green.
11. Attach MCHStreamer Lite TDM16 @48 kHz; verify one synchronous 16-slot output clock domain.
12. Attach two synchronized 8-channel DAC stages; verify electrical activity and exact mapping on 12 used outputs.
13. Record ALSA identity/format, buffer/period settings, expected vs actual frames, xruns, CPU load, clock/drift and physical loopback latency where a return path exists.
14. Close #143 only after one full-duration run proves `physical eARC capture -> raw JOC preservation -> Aurora moving-object render -> synchronous physical 12-channel output`.
15. Only then test legitimate Netflix/other service Atmos separately, followed by final DAC/amplifier/speaker and wireless transport work.

Other queued trackers:
- #115: evaluate newer Omniphony behind a separate reference lane; do not upgrade stable pin by recency alone;
- #116: reassess Source Manager acceptance before closing.

## 7. Required validation before merge

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

Additional gates:
- immersive/JOC changes: official Immersive JOC Stack evidence;
- moving reference: Official Dolby JOC Temporal CI;
- Aurora moving path: Aurora Moving JOC CI;
- simulation coverage contract: `python3 validation/virtual-hardware/validate_simulation_coverage.py self-test` and `check`;
- full-system virtual hardware, including real Aurora output DSP: `Aurora Full-System Sim CI`;
- native Windows launcher with coverage parity: `Aurora Full-System Sim Windows CI`;
- physical ingress/capture tooling: `Aurora Physical Ingress Tooling CI` on Linux and Windows.

Tooling/simulation gates must never be reported as physical proof. Do not merge while required gates are red.
