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
- PR #184 merged native Configuration v4 elevation-aware canonical 7.1.4 intent at merge commit `61beeed8c8f296955a4489107c11ea78480e374b`.
- PR #186 merged the exact-pin/governance boundary for Scramble Tools `esp_avb` + `esp_ptp`.
- PR #187 merged deterministic canonical 7.1.4 -> six stereo ESP-AVB endpoint fanout.
- PR #189 merged six-transport ESP-AVB orchestration at merge commit `87119b6b89d6ccf8123ae9ce3d7ce91302af395c`.
- Active draft PR #191 (`genavb-aaf-talker-adapter`) adds a platform-gated NXP GenAVB/TSN AAF stereo talker adapter. Exact-pin Rust/C/API CI now proves its declared software contract; physical NXP/ESP evidence remains separate.
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
- #160–#170 — JOCForge, IAMF, FFmpeg, MPEG-H, ADM/EAR/SAF and 3D reference lanes.
- #172 — decoded-PCM upmix validation through custom 11.1.4.
- #173–#175 — RoomEQ/CamillaDSP reference, synthetic optimization, and real 12-channel execution differential.
- #176 — Google OBR / EBU BEAR / `sofar` binaural reference baseline.
- #179–#183 — open pro-audio boundary, AOO runtime, libspatialaudio runtime, object-PCM materialization, control-plane selection.
- #184 — Configuration v4 canonical elevation-aware 7.1.4 intent.
- #186 — exact-pinned ESP-AVB/ESP-PTP source/governance/license boundary.
- #187 — deterministic 12-channel Aurora 7.1.4 -> six stereo ESP-AVB endpoint fanout.
- #189 — six-endpoint AVB transport orchestration with whole-set fail-closed behavior.

These are software/reference milestones only unless a specific item explicitly states physical evidence.

## 4. Room correction and binaural boundaries

### RoomEQ / `pierreaubert/autoeq`
- pin: `579dd7486024fc18ff219e31eb7337362814f602`; GPL-3.0-or-later; external optimizer/reference only.

### CamillaDSP
- pin: `05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74`; 4.1.3; GPL-3.0-only OR MPL-2.0; external DSP executor/reference only.

### Binaural references
- Google OBR pin `478dc7c752d5eccae534635139ff0253eee3a14a`;
- EBU BEAR pin `6127e897b941211051c2ad135ee09b00be2e6ae0`;
- `andreiltd/sofar` pin `06a629292689e99841e5dacaa25c4c6298616ca6`.

Physical room behavior, personalized HRTF quality, head tracking, and perceptual parity remain unproven.

## 5. Open pro-audio and embedded endpoint integration

Aurora owns decoder/scene/renderer/DSP/realtime/runtime boundaries. Network I/O stays on worker threads; common rendering/system DSP completes before local/wired/wireless fanout.

### AOO
- pin: `dc2a5be2962ba02d6cebe297f31f2774f34e7bc7`;
- software-proven worker-side runtime adapter with real host UDP fault/drift/reconnect evidence;
- not RF/physical-sync proof and not production-selected.

### NXP GenAVB/TSN
- pin: `6b962d6c34b0c3f142295a213dfa70bda193b23d` (7.3.2);
- wired AVB/TSN/Milan candidate on supported NXP platforms;
- pinned Linux media-app reference contains six stereo AAF talker streams at 48 kHz using `AAF_FORMAT_INT_32BIT`, 24-bit PCM depth, SR Class B and 1 ms batching;
- `genavb_init()` is process-global at the pinned Linux API, so Aurora's six talkers share one native GenAVB runtime;
- hardware timestamping, gPTP lock, Milan interoperability and physical latency remain hardware evidence.

### Scramble Tools ESP endpoints
- `esp_avb` pin `5e75bd3ed91b5407a254a5e49bfc18fc35e6cbb9` (2.18.0, MIT): AAF PCM 24-bit/48 kHz, max two channels/stream, P4 Ethernet and C6 Wi-Fi endpoint modes.
- `esp_ptp` pin `5b7eec233a93733ae954beefb6df3bb9c12dc901` (1.2.3, Apache-2.0): IEEE 1588 / 802.1AS candidate; P4 hardware-timestamp path and C6 software-disciplined path.
- no physical RF/synchronization/latency claim exists yet.

### Aurora ESP-AVB fanout + orchestration — merged #187/#189
- package: `adapters/aurora-network-esp-avb`;
- channel mapping: `FL/FR`, `FC/LFE`, `SL/SR`, `SBL/SBR`, `TFL/TFR`, `TRL/TRR`;
- every endpoint stream is stereo / 48 kHz and preserves the source sequence + `MediaTimestamp`;
- `EspAvbTransportSet` prepares and starts six AVB `NetworkAudioTransport`s with `PtpFollower` discipline;
- partial start rolls back; any submit fault stops/resets all six and requires prepare/start again;
- executable CI proved all six observe the same sequence/timestamp for a submitted block.

### NXP GenAVB AAF talker adapter — active draft PR #191
- standalone package: `adapters/aurora-network-genavb`, outside the default workspace/runtime selection;
- implements one stereo `NetworkAudioTransport` talker with fixed v1 contract: 48 kHz, 48 frames, two channels, `PtpFollower`, no adaptive rate correction, no packet-repair claim;
- Rust worker preallocates its AAF payload buffer and converts Aurora f32 PCM deterministically to signed 24-bit samples in 32-bit AAF slots, eight zero LSBs, big-endian, with saturation and non-finite rejection;
- native Aurora shim compiles against exact NXP 7.3.2 public headers and creates AAF INT32/24-bit stereo SR Class B talker streams;
- six talker adapter instances share one process-global GenAVB runtime and require the same AVTP clock;
- first submitted Aurora media frame establishes one **shared** media-frame -> AVTP/PTP anchor for the concurrently started talkers; all six therefore receive identical AVTP presentation time for the same Aurora frame instead of independently sampling the clock;
- `genavb_stream_presentation_offset()` is enforced before submission; late/unsafe presentation fails closed;
- exact-pin CI on Rust 1.78 proves fmt/check/clippy/tests, compiles the C shim against the real NXP headers, and runs a deterministic six-talker contract probe;
- probe result: six talkers, one `genavb_init()`, shared PTP anchor, identical first timestamp, and exactly +1,000,000 ns for the next 48-frame block;
- current evidence is software/API only: no real GenAVB service, NXP hardware timestamp, gPTP lock, ESP listener, RF or measured physical latency claim.

### Sound Open Firmware
- pin: `11cfcaf8f46d5c02b1c30e8394d10351ccd00e7c`; i.MX8M Plus HiFi4 DSP execution candidate; physical execution remains unproven.

### VideoLAN `libspatialaudio`
- pin: `d149ed9744fd399b835c6f2920511f8cbcfce5ea`; LGPL-2.1-or-later;
- software-proven canonical 7.1.4 object-PCM adapter at 48 kHz / 256 frames with 255-frame direct-path latency and explicit opt-in selection;
- not automatic/default production renderer; no physical/acoustic parity claim.

### Clock/DSP ownership rules
- Aurora owns the logical media timeline.
- Exactly one adaptive sample-rate controller may own any clock-domain crossing.
- GenAVB/ESP-PTP may map Aurora time into the PTP/gPTP domain; they do not redefine Aurora logical time.
- Wired/wireless/local output fanout occurs after common rendering and system DSP.
- Only one production speaker renderer processes a block.

## 6. Current work / Next actions

Continue in this order unless the user explicitly changes priorities:

1. Keep PR #191 draft until refreshed final-head `GenAVB AAF Talker CI`, `Open Audio Stack CI`, and general CI are green with the handoff/governance changes included.
2. Add a higher-level six-GenAVB-talker construction path that maps Aurora's six endpoint specs to six unique AVTP stream IDs and multicast destination MACs without hard-coding product network identities into core contracts.
3. Add negative software tests for mismatched AVTP clocks, inconsistent target latency, late presentation deadlines, stream create failure, and one-talker send failure; the six-stream set must fail closed rather than let one speaker pair drift.
4. Build a real platform test on supported NXP GenAVB hardware/service only when hardware is available: establish gPTP lock, create six talkers, connect at least one real ESP-AVB listener, then expand to six listeners.
5. Prove multi-endpoint synchronization, loss/reconnect behavior, drift, RF resilience and measured physical latency on selected ESP32-P4/C6 hardware before considering the wireless transport production-capable.
6. Resume Phase 11 binaural work and physical tracker #143 according to user priority.

## 7. Physical acceptance critical path — tracker #143

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

## 8. Key repository map

- core/layouts/config: `crates/aurora-core/`, `crates/aurora-scene/`, `crates/aurora-config/`;
- renderers: `crates/aurora-renderer-api/`, `aurora-renderer-basic/`, `aurora-renderer-vbap/`, external adapters under `adapters/`;
- decoders: `crates/aurora-decoder-api/`, `aurora-decoder-*`;
- DSP: `crates/aurora-dsp-api/`, `aurora-dsp-basic/`, `aurora-dsp-camilladsp/`;
- audio I/O/network: `crates/aurora-audio-io/`, `crates/aurora-realtime-audio-*`, network adapters under `adapters/`;
- realtime engine: `crates/aurora-realtime-engine/`;
- open pro-audio integration: `config/open-audio-stack-v1.json`, `validation/open-audio-stack/`, `docs/adr/0021-open-pro-audio-stack-integration.md`;
- ESP fanout/orchestration: `adapters/aurora-network-esp-avb/`;
- GenAVB talker: `adapters/aurora-network-genavb/`, `.github/workflows/genavb-aaf-talker-ci.yml`;
- immersive/JOC: `validation/immersive/`; open immersive references: `validation/open-immersive/`;
- binaural validation: `validation/binaural/`; room correction: `validation/room-correction/`;
- physical ingress: `validation/physical/`; virtual hardware: `validation/virtual-hardware/`;
- external registry: `config/external-components-v1.json`; licenses: `THIRD_PARTY_LICENSES.md`;
- roadmap: `docs/pre-hardware-roadmap-v5.md`.

## 9. Required base validation before merge

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

Also run every domain-specific gate touched by the change. GenAVB adapter changes must run `GenAVB AAF Talker CI`; open pro-audio config/source-policy changes must run `Open Audio Stack CI`; ESP fanout/orchestration changes must run `ESP-AVB Endpoint Contract CI`. Tooling/simulation/reference gates must never be reported as physical or perceptual proof.
