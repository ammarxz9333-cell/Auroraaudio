# Aurora Agent Reference — Canonical Living Handoff

> Read this file first. Keep it factual, compact, and current.
>
> **Maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge.

Last updated: **2026-09-16**

## 1. Source of truth and continuation rule

- Canonical long-lived branch: `main-v2`.
- Temporary feature/debug branches are allowed only for isolated work and should not become alternate baselines.
- If the user says **“كمل” / “continue”**, continue the first unfinished item under **Current work / Next actions**. Do not restart project discovery.
- Aurora stays hardware-neutral until the software/reference/resilience gates in `docs/pre-hardware-roadmap-v5.md` are satisfied.

Current canonical base:
- Phase 9 decoded-PCM upmix matrix is complete for the declared software subset: 5.1 -> 7.1.4, 5.1 -> custom 11.1.4, and 7.1 -> custom 11.1.4.
- PRs #173–#175 completed the declared Phase 10 RoomEQ/CamillaDSP software/reference scope; physical room/DAC/speaker evidence remains separate and unproven.
- PR #176 merged the initial Phase 11 binaural-reference baseline.
- PRs #179–#183 merged the open pro-audio boundary, real AOO runtime transport adapter, libspatialaudio runtime adapter, object-PCM realtime materialization, and explicit libspatialaudio control-plane selection.
- Draft PR #184 separately adds Configuration v4 native 7.1.4 elevation intent; keep transport work decoupled from that schema change.
- PR #186 merged the exact-pin/governance boundary for Scramble Tools `esp_avb` + `esp_ptp` at merge commit `9de1014fa7d8366d37622974791f43d9427ecada`.
- PR #187 merged the deterministic canonical 7.1.4 -> six stereo ESP-AVB endpoint fanout at merge commit `0a3339bf718c08acbd612feb9fe688588918abbe`.
- Active PR #189 (`esp-avb-sender-orchestration`) coordinates six AVB `NetworkAudioTransport` workers as one fail-closed PTP-disciplined endpoint set. Its dedicated endpoint CI, Linux/Windows stable CI, and Rust 1.78 MSRV checks are green at the implementation head before this handoff refresh.
- Synthetic upmix is never described as object recovery, JOC reconstruction, IAMF rendering, or authored Atmos recovery.

## 2. Product goal and non-negotiable truth rules

Aurora is an open, modular, hardware-agnostic immersive-audio stack, primarily Rust.

Direction:
- realtime multichannel audio, initially 7.1.4 and expandable toward custom 11.1.4;
- replaceable source, decoder, renderer, DSP, audio-I/O, transport, and hardware adapters;
- legitimate TV/eARC ingress and physical multichannel output;
- later private wireless rear/speaker/multi-room transport;
- no SBC, MCU, DAC, eARC board, AVR, soundbar, speaker product, or OS image may define core architecture.

Rules:
- realtime callback: no allocation after preparation, locks, logging/formatting, filesystem/process/network access, config parsing/registry lookup, or silent device changes;
- prepare -> validate -> commit is transactional; failed candidates do not mutate active state;
- unknown/incompatible component IDs, schemas, contracts, capabilities, or generations fail closed;
- object decoding, channel-bed decoding, synthetic upmixing, speaker rendering, and binaural rendering are distinct capabilities;
- simulation is not runtime evidence; runtime evidence is not physical evidence;
- only physical loopback may be called measured physical latency;
- no DRM/Widevine/HDCP circumvention, protected-media extraction, or device/certification-state spoofing;
- no Dolby/DTS/HDMI certification or conformance claim from software CI;
- GPL/incompatible or legally conditional references remain external unless a deliberate licensing decision changes that boundary;
- every new simulator-testable capability must be declared in `config/simulation-coverage-v1.json` with executable healthy/fault evidence before it can be marked covered.

## 3. Important merged software/reference milestones

- #158 — Aurora-vs-OAR 5.1 object semantic differential.
- #159 — adaptive clock-rate correction, bounded reconnect recovery, panic-isolated decoder/runtime recovery, live JOC validation, placeholder audit, and sustained realtime/fault evidence.
- #160 — pinned JOCForge external fixture/conformance-generator lane.
- #161 — pinned IAMF encode/decode independent reference cross-check.
- #163 — exact-pin FFmpeg compatibility matrix.
- #164 — MPEG-H dual-oracle reference validation.
- #165–#170 — ADM/libadm, EAR, SAF and 3D layout/trajectory reference work.
- #172 — decoded-PCM upmix validation through custom 11.1.4.
- #173–#175 — RoomEQ/CamillaDSP reference, synthetic optimization, and real 12-channel execution differential.
- #176 — Google OBR / EBU BEAR / `sofar` binaural reference baseline.
- #179–#183 — open pro-audio boundary, AOO runtime, libspatialaudio runtime, object-PCM materialization, control-plane selection.
- #186 — exact-pinned ESP-AVB/ESP-PTP source/governance/license boundary.
- #187 — deterministic 12-channel Aurora 7.1.4 -> six stereo ESP-AVB endpoint fanout.

These are software/reference milestones only unless a specific item explicitly states physical evidence.

## 4. Phase 10 — room correction and system DSP

### RoomEQ / `pierreaubert/autoeq`
- pin: `579dd7486024fc18ff219e31eb7337362814f602`;
- observed version: `0.5.73`;
- license: GPL-3.0-or-later;
- external optimization/validation reference only.

### CamillaDSP
- pin: `05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74`;
- version: `4.1.3`;
- license: GPL-3.0-only OR MPL-2.0;
- external DSP executor/reference only.

A green Phase 10 lane does **not** prove microphone/acoustic correction, physical DAC/speaker routing, measured physical latency, protected-service compatibility, certification, or arbitrary unrepresented DSP graphs.

## 5. Phase 11 — binaural

Pinned baseline references:
- Google OBR pin `478dc7c752d5eccae534635139ff0253eee3a14a`; external oracle only; BSD-style source plus Open Binaural Renderer Patent License 1.0.
- EBU BEAR pin `6127e897b941211051c2ad135ee09b00be2e6ae0`; Apache-2.0; upstream pre-release; external oracle only.
- `andreiltd/sofar` pin `06a629292689e99841e5dacaa25c4c6298616ca6`; MIT OR Apache-2.0; Rust-native SOFA/HRTF candidate only.

Physical/perceptual binaural quality, personalized HRTF behavior and head-tracker hardware remain unproven.

## 6. Open pro-audio and embedded endpoint integration

Aurora owns decoder/scene/renderer/DSP/realtime/runtime boundaries. Network I/O stays on worker threads; common rendering/system DSP completes before local/wired/wireless fanout.

### AOO
- pin: `dc2a5be2962ba02d6cebe297f31f2774f34e7bc7`;
- license: Standard Improved BSD;
- peer/wireless network-audio candidate behind Aurora's worker contract;
- software-proven runtime adapter with real UDP host evidence for healthy transport, deterministic loss/reorder/jitter, ±250 ppm peer-clock cases, receiver restart and lifecycle/reload;
- not production-selected and not RF/physical-sync proof.

### NXP GenAVB/TSN
- pin: `6b962d6c34b0c3f142295a213dfa70bda193b23d` (7.3.2);
- wired AVB/TSN/Milan candidate on supported NXP platforms;
- hardware timestamping, gPTP lock and Milan interoperability remain physical/platform evidence.
- The pinned Linux media-app reference already defines **six AAF talker streams**, each stereo, 48 kHz, `AAF_FORMAT_INT_32BIT` with 24-bit PCM depth, SR Class B and a 1 ms batch/latency setting. This is a strong software/API match for Aurora's six-stereo ESP endpoint topology, but it is not physical proof.

### Scramble Tools `esp_avb`
- pin: `5e75bd3ed91b5407a254a5e49bfc18fc35e6cbb9` (2.18.0);
- license: MIT;
- ESP32-P4/C6 embedded AVB endpoint candidate behind Aurora's network-worker boundary;
- pinned profile: AAF PCM 24-bit/48 kHz, one talker plus one listener, maximum two channels per stream;
- upstream includes wired endpoint, Wi-Fi endpoint and experimental Ethernet/Wi-Fi bridge modes;
- no physical RF/synchronization/latency claim exists yet.

### Scramble Tools `esp_ptp`
- pin: `5b7eec233a93733ae954beefb6df3bb9c12dc901` (1.2.3);
- license: Apache-2.0;
- IEEE 1588 PTP / IEEE 802.1AS gPTP time-mapping candidate for ESP endpoints;
- upstream documents ESP32-P4 EMAC hardware timestamps and ESP32-C6 software-disciplined clock operation;
- may map Aurora media time to endpoint/network time but never becomes Aurora's logical media-clock owner.

### Aurora ESP-AVB fanout — merged PR #187
- package: `adapters/aurora-network-esp-avb`;
- canonical mapping: `FL/FR`, `FC/LFE`, `SL/SR`, `SBL/SBR`, `TFL/TFR`, `TRL/TRR`;
- each stream is exactly stereo at Aurora's canonical 48 kHz network rate;
- all endpoint blocks preserve the same source sequence and `MediaTimestamp`;
- timeline discontinuity fails closed;
- prepared buffers allocate before streaming; `split()` performs bounded PCM copies only and no network I/O;
- P4/C6 timestamp capabilities remain explicitly distinct;
- required discipline is `PtpFollower`; no adaptive-rate or packet-repair capability is claimed.

### Aurora six-endpoint transport orchestration — active PR #189
- `EspAvbTransportSet` owns six `NetworkAudioTransport` implementations behind the worker-only contract;
- each endpoint must advertise AVB/TSN family, stereo capacity, scheduled playout and no adaptive rate matcher;
- all six are prepared with identical 48 kHz stereo format, fixed latency bounds and `PtpFollower` discipline;
- partial start failure rolls back the already-started endpoints;
- one 7.1.4 block is split and submitted to all six with the exact same sequence and Aurora `MediaTimestamp`;
- any endpoint submission failure stops/resets the entire set and returns the lifecycle to `New`, requiring explicit prepare/start before more audio;
- executable CI probe proves six transports observe `sequence=23`, `timestamp=96000` for the same block;
- this remains host-side orchestration only, not actual GenAVB packet transmission or ESP firmware.

### Sound Open Firmware
- pin: `11cfcaf8f46d5c02b1c30e8394d10351ccd00e7c`;
- i.MX8M Plus HiFi4 DSP execution candidate;
- Aurora owns DSP policy/coefficients; physical i.MX8MP execution remains unproven.

### VideoLAN `libspatialaudio`
- pin: `d149ed9744fd399b835c6f2920511f8cbcfce5ea`;
- license: LGPL-2.1-or-later;
- runtime object-PCM renderer behind a replaceable dynamic-shim/control-plane boundary;
- proven software contract: canonical 7.1.4 at 48 kHz / 256 frames with 255-frame direct-path latency and explicit opt-in selection;
- not the automatic/default production renderer; no physical/acoustic parity claim.

### Clock/DSP ownership rules
- Aurora owns the logical media timeline.
- Exactly one adaptive sample-rate controller may own any clock-domain crossing.
- PTP/GenAVB or ESP-PTP may own network-time mapping on supported endpoints; AOO peer correction is used only when no other controller owns the same crossing.
- Wired/wireless/local output fanout occurs after common rendering and system DSP.
- Only one production speaker renderer processes a block.

## 7. Current work / Next actions

Continue in this order unless the user explicitly changes priorities:

1. Merge PR #189 only after its refreshed final-head general CI and `ESP-AVB Endpoint Contract CI` remain green.
2. Implement a **platform-gated NXP GenAVB AAF talker adapter** behind `NetworkAudioTransport`, using the exact pinned GenAVB 7.3.2 API. Keep it outside the default workspace/runtime selection until proven.
3. The GenAVB adapter must convert Aurora f32 stereo to deterministic saturated 24-bit PCM carried in AAF 32-bit slots and create one talker stream per stereo endpoint. Do not invent AVTP timestamps.
4. Define a control-plane mapping from Aurora absolute media frames to the GenAVB/gPTP domain using real GenAVB clock APIs and `genavb_stream_presentation_offset()`. The AVTP `genavb_event.ts` mapping must be derived from an explicit PTP anchor, not from an assumed epoch.
5. After host talker software/API evidence is green, bind the six streams to real ESP-AVB listener firmware and prove multi-endpoint gPTP synchronization, loss/reconnect behavior, drift, RF resilience and measured physical latency on selected ESP32-P4/C6 hardware.
6. Keep Configuration v4 work (#184) independent; resume Phase 11 binaural work and physical tracker #143 according to user priority.

## 8. Physical acceptance critical path — tracker #143

Still unproven physically:
- continuous `eARC -> E-AC-3 JOC -> Aurora -> synchronous physical 7.1.4`;
- real Gate A capture under the merged validator;
- synchronous physical 12-channel DAC output and electrical channel mapping;
- protected-service Atmos through a legitimate TV/streamer -> eARC path;
- physical loopback latency/drift;
- acoustic correction/parity and amplifier/speaker design;
- physical wired/wireless multi-speaker synchronization;
- physical head tracker and headphone/HRTF transfer behavior.

Two ingress paths remain hypotheses, neither a product freeze:
- fallback validation chain: `authorized TV/player -> Lindy 38368 / SiI9437 project tap -> Linux capture host -> Aurora`;
- low-component-count investigation: legitimate TV/player eARC -> native i.MX8M Plus audio-XCVR/Linux path -> Aurora, subject to real compressed/JOC/MAT capture evidence on selected hardware.

Do not invent ALSA device names, reset/drop counters, hardware timings, supported compressed formats, or measured acoustic results. Use actual physical evidence when hardware is present.

## 9. Key repository map

- core/layouts: `crates/aurora-core/`, `crates/aurora-scene/`;
- renderers: `crates/aurora-renderer-api/`, `aurora-renderer-basic/`, `aurora-renderer-vbap/`, external adapters under `adapters/`;
- decoders: `crates/aurora-decoder-api/`, `aurora-decoder-*`;
- DSP: `crates/aurora-dsp-api/`, `aurora-dsp-basic/`, `aurora-dsp-camilladsp/`;
- audio I/O/network: `crates/aurora-audio-io/`, `crates/aurora-realtime-audio-*`, network adapters under `adapters/`;
- realtime engine: `crates/aurora-realtime-engine/`;
- open pro-audio integration: `config/open-audio-stack-v1.json`, `validation/open-audio-stack/`, `docs/adr/0021-open-pro-audio-stack-integration.md`;
- ESP-AVB fanout/orchestration: `adapters/aurora-network-esp-avb/`;
- config/runtime: `crates/aurora-config/`, `aurora-runtime-assembly/`, `aurora-runtime-materialization/`, `aurora-runtime-inspection/`;
- immersive/JOC: `validation/immersive/`;
- open immersive references: `validation/open-immersive/`;
- binaural validation: `validation/binaural/`;
- virtual hardware: `validation/virtual-hardware/`;
- physical ingress: `validation/physical/`;
- room correction: `validation/room-correction/`;
- external registry: `config/external-components-v1.json`;
- license boundaries: `THIRD_PARTY_LICENSES.md`;
- roadmap: `docs/pre-hardware-roadmap-v5.md`.

## 10. Required base validation before merge

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

Also run every domain-specific gate touched by the change. ESP fanout/orchestration must run `ESP-AVB Endpoint Contract CI`; open pro-audio source/API changes must run `Open Audio Stack CI`. Tooling/simulation/reference gates must never be reported as physical or perceptual proof.
