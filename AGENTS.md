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
- #173–#175 completed the declared RoomEQ/CamillaDSP software/reference scope; physical room/DAC/speaker evidence remains separate.
- #176 merged the initial binaural-reference baseline.
- #179–#183 merged the open pro-audio boundary, real AOO adapter, libspatialaudio runtime adapter, object-PCM materialization, and explicit renderer selection.
- #184 merged native Configuration v4 elevation-aware canonical 7.1.4 intent.
- #186 merged the exact-pin/governance boundary for Scramble Tools `esp_avb` + `esp_ptp`.
- #187 merged deterministic 7.1.4 -> six stereo ESP-AVB fanout.
- #189 merged six-transport ESP-AVB orchestration with whole-set fail-closed behavior.
- #191 merged the platform-gated NXP GenAVB/TSN AAF talker adapter at merge commit `d297647f42e450b89d69f46221f1cf8248ff3f18`.
- #192 merged Configuration-v4 -> prepared external Object-PCM runtime plans for libspatialaudio.
- #194 merged AVDECC media-stack CONNECT/DISCONNECT ownership and AVDECC-supplied talker parameters at merge commit `d6b0f6171003a6833bd05b8c81e8739d7bb43868`.
- #195 merged the Rust AVDECC worker/control wrapper at merge commit `674553b3c21be3782f5c01552ddd84bf165ea6b3`.
- #196 merged the fixed-size fail-closed six-stream AVDECC session gate at merge commit `e675f400fd471c6b4c0a45e38df0c40e7639e624`.
- Active draft #198 (`genavb-rust-avdecc-prepare`) exposes the native AVDECC-driven talker prepare path through the Rust GenAVB adapter and adds a cross-language FFI probe.
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
- unknown/incompatible component IDs, schemas, contracts, capabilities, generations, stream identities, or control-plane state fail closed;
- object decoding, channel-bed decoding, synthetic upmixing, speaker rendering, and binaural rendering are distinct capabilities;
- simulation is not runtime evidence; runtime evidence is not physical evidence;
- only physical loopback may be called measured physical latency;
- no DRM/Widevine/HDCP circumvention, protected-media extraction, or device/certification-state spoofing;
- no Dolby/DTS/HDMI/AVB/Milan certification or conformance claim from software CI;
- GPL/incompatible or legally conditional references remain external unless a deliberate licensing decision changes that boundary;
- every new simulator-testable capability must be declared in `config/simulation-coverage-v1.json` with executable healthy/fault evidence before it can be marked covered.

## 3. Important merged software/reference milestones

- #158–#170 — Aurora-vs-OAR, JOCForge, IAMF, FFmpeg, MPEG-H, ADM/EAR/SAF and 3D reference lanes.
- #172 — decoded-PCM upmix validation through custom 11.1.4.
- #173–#175 — RoomEQ/CamillaDSP reference, synthetic optimization, and real 12-channel execution differential.
- #176 — Google OBR / EBU BEAR / `sofar` binaural-reference baseline.
- #179–#183 — open pro-audio boundary, AOO runtime, libspatialaudio runtime, object-PCM materialization, control-plane selection.
- #184 — Configuration v4 canonical elevation-aware 7.1.4 intent.
- #186 — exact-pinned ESP-AVB/ESP-PTP source/governance/license boundary.
- #187 — deterministic 12-channel 7.1.4 -> six stereo ESP endpoint fanout.
- #189 — six-endpoint AVB transport orchestration with whole-set fail-closed behavior.
- #191 — exact-pin NXP GenAVB AAF stereo talker adapter and shared six-talker PTP anchor software proof.
- #192 — prepared external Object-PCM runtime plan for native-v4 libspatialaudio selection.
- #194 — NXP AVDECC media-stack control adapter and stack-owned CONNECT -> talker-prepare parameters.
- #195 — Rust AVDECC worker/control wrapper with pollable control fd and sanitized CONNECT/DISCONNECT events.
- #196 — fixed-size six-stream AVDECC session gate requiring all six canonical stereo endpoint roles before immersive start eligibility.

These are software/reference milestones unless a specific item explicitly states physical evidence.

## 4. Room correction and binaural boundaries

- RoomEQ / `pierreaubert/autoeq`: pin `579dd7486024fc18ff219e31eb7337362814f602`; GPL-3.0-or-later; external optimizer/reference only.
- CamillaDSP: pin `05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74`; 4.1.3; GPL-3.0-only OR MPL-2.0; external DSP executor/reference only.
- Binaural references: Google OBR `478dc7c752d5eccae534635139ff0253eee3a14a`; EBU BEAR `6127e897b941211051c2ad135ee09b00be2e6ae0`; `andreiltd/sofar` `06a629292689e99841e5dacaa25c4c6298616ca6`.

Physical room behavior, personalized HRTF quality, head tracking and perceptual parity remain unproven.

## 5. Open pro-audio and embedded endpoint integration

Aurora owns decoder/scene/renderer/DSP/realtime/runtime boundaries. Network and AVDECC control I/O stay on worker/control threads; common rendering/system DSP completes before local/wired/wireless fanout.

### AOO
- pin `dc2a5be2962ba02d6cebe297f31f2774f34e7bc7`;
- software-proven worker-side runtime adapter with host UDP fault/drift/reconnect evidence;
- not RF/physical-sync proof and not production-selected.

### NXP GenAVB/TSN
- pin `6b962d6c34b0c3f142295a213dfa70bda193b23d` (7.3.2);
- wired AVB/TSN/Milan candidate on supported NXP platforms;
- pinned API documents static stream creation and AVDECC media-stack connection state;
- in AVDECC mode the stack sends `GENAVB_MSG_MEDIA_STACK_CONNECT` on `GENAVB_CTRL_AVDECC_MEDIA_STACK`; CONNECT carries the `genavb_stream_params` used by `genavb_stream_create()`;
- NXP's Linux reference lifecycle waits for CONNECT, applies stack-supplied parameters, creates/processes the stream, and stops on DISCONNECT;
- `genavb_init()` is process-global at the pinned Linux API, so Aurora control and talker handles share one native runtime;
- hardware timestamping, gPTP lock, physical AVDECC/Milan interoperability and latency remain hardware evidence.

### Scramble Tools ESP endpoints
- `esp_avb` pin `5e75bd3ed91b5407a254a5e49bfc18fc35e6cbb9` (2.18.0, MIT): AAF PCM 24-bit/48 kHz, maximum two channels/stream, P4 Ethernet and C6 Wi-Fi endpoint modes;
- `esp_ptp` pin `5b7eec233a93733ae954beefb6df3bb9c12dc901` (1.2.3, Apache-2.0): IEEE 1588 / 802.1AS candidate; P4 hardware-timestamp and C6 software-disciplined paths;
- ESP listener binding is AVDECC/ACMP-oriented: talker entity + talker stream identity matter; raw packet arrival alone is not treated as the native plug-and-play connection lifecycle;
- no physical RF/synchronization/latency claim exists yet.

### Aurora ESP fanout/orchestration — merged #187/#189
- package `adapters/aurora-network-esp-avb`;
- mapping `FL/FR`, `FC/LFE`, `SL/SR`, `SBL/SBR`, `TFL/TFR`, `TRL/TRR`;
- every endpoint stream is stereo / 48 kHz and preserves source sequence + `MediaTimestamp`;
- `EspAvbTransportSet` prepares/starts six transports with `PtpFollower` discipline;
- partial start rolls back; any submit fault stops/resets all six and requires prepare/start again.

### Aurora NXP GenAVB AAF talker — merged #191
- package `adapters/aurora-network-genavb`, outside default workspace/runtime selection;
- fixed software contract: stereo, 48 kHz, 48-frame block, AAF INT32 container with 24-bit PCM depth, `PtpFollower`, no adaptive rate correction or packet-repair claim;
- deterministic saturating f32 -> signed 24-bit / 32-bit AAF-slot big-endian conversion; non-finite PCM fails closed;
- six talkers share one GenAVB runtime and one AVTP clock requirement;
- first submitted Aurora frame establishes one shared media-frame -> AVTP/PTP anchor; equal Aurora frames therefore map to equal AVTP presentation times across six sequential talker submissions;
- presentation offset/deadline checks fail closed;
- exact-pin CI proved six talkers, one `genavb_init()`, equal first timestamps and exactly +1,000,000 ns per 48 frames;
- the static `aurora_genavb_prepare(...)` path remains a software/validation fallback and is not the preferred native ESP plug-and-play path.

### AVDECC-owned GenAVB lifecycle — merged #194/#195/#196; active #198
- #194 opens `GENAVB_CTRL_AVDECC_MEDIA_STACK` on the shared runtime, exposes the control RX fd, accepts only supported AAF/48 kHz/stereo/24-bit talker CONNECTs, caches exact stack-supplied `genavb_stream_params`, and invalidates them on matching DISCONNECT;
- `aurora_genavb_prepare_avdecc(...)` creates a talker from the cached stack-owned parameters, so stream ID, destination MAC, port, class and format are not invented by Aurora;
- #195 wraps that control channel in Rust and exposes only sanitized CONNECT/DISCONNECT events plus a worker-only opaque native handle while the channel is open;
- #196 maps exactly six deployment-selected AVDECC Stream Output descriptor indices to Front, Center/LFE, Surround, Back Surround, Top Front and Top Rear roles; duplicate/unknown indices, duplicate stream IDs/MACs, invalid media contracts, partial sets and mismatched DISCONNECTs fail closed;
- immersive start eligibility is explicit: `require_complete()` requires all six accepted connections; no missing speaker pair is silently routed;
- #198 adds the Rust talker-side AVDECC prepare bridge, an AVDECC-only constructor with no placeholder network identities, same-shim validation before native pointer crossing, and a cross-language FFI probe;
- exact-pin/native/Rust CI is software/API evidence only; no real NXP service or ESP endpoint has yet completed the physical ADP/ACMP/gPTP path.

### Sound Open Firmware / libspatialaudio
- SOF pin `11cfcaf8f46d5c02b1c30e8394d10351ccd00e7c`; i.MX8M Plus HiFi4 candidate; physical execution unproven.
- libspatialaudio pin `d149ed9744fd399b835c6f2920511f8cbcfce5ea`; LGPL-2.1-or-later; software-proven canonical 7.1.4 object-PCM adapter, explicit selection only, no physical/acoustic parity claim.

### Clock/DSP ownership
- Aurora owns the logical media timeline.
- Exactly one adaptive sample-rate controller may own a clock-domain crossing.
- GenAVB/ESP-PTP map Aurora time into PTP/gPTP; they do not redefine Aurora logical time.
- AVDECC/ACMP owns network stream connection identity/state on the GenAVB path; Aurora owns PCM production and worker lifecycle around that state.
- fanout occurs after common rendering/system DSP; only one production speaker renderer processes a block.

## 6. Current work / Next actions

Continue in this order unless the user explicitly changes priorities:

1. Finish #198: keep Rust 1.78 fmt/check/clippy/tests, the cross-language Rust AVDECC-prepare FFI probe, exact NXP 7.3.2 compile/probes, and general CI green before merge.
2. Add a six-talker GenAVB worker runtime that consumes accepted session events, prepares each talker from the matching AVDECC cached stream state, and requires both six accepted connections and six successful current-epoch prepares before start.
3. Make six-talker start/submit fail closed as a set: partial start rolls back, any one-talker submit fault stops the set, DISCONNECT immediately removes start eligibility, and reconnect must re-prepare the affected stream in the new epoch.
4. Add deterministic negative probes for one prepare failure, one start failure, one submit failure, duplicate/reordered CONNECTs, DISCONNECT while active, stale prepared state, and reconnect recovery.
5. On supported physical NXP GenAVB hardware/service, establish gPTP lock and prove ADP/ACMP connection to one real ESP-AVB listener first, then six listeners.
6. Prove reconnect, drift, multi-endpoint synchronization, RF resilience and physical loopback latency before production-selection claims.
7. Continue native-v4/libspatialaudio work (#193) independently; resume binaural and physical tracker #143 by user priority.

## 7. Physical acceptance critical path — tracker #143

Still unproven physically:
- continuous `eARC -> E-AC-3 JOC -> Aurora -> synchronous physical 7.1.4`;
- real Gate A capture under the merged validator;
- synchronous physical 12-channel DAC output and electrical channel mapping;
- protected-service Atmos through a legitimate TV/streamer -> eARC path;
- physical loopback latency/drift;
- acoustic correction/parity and amplifier/speaker design;
- physical wired/wireless multi-speaker synchronization;
- physical AVDECC/ACMP/gPTP interoperability between selected NXP host and ESP endpoints;
- physical head tracker and headphone/HRTF transfer behavior.

Ingress hypotheses, not product freezes:
- fallback: `authorized TV/player -> Lindy 38368 / SiI9437 project tap -> Linux capture host -> Aurora`;
- low-component-count investigation: legitimate TV/player eARC -> native i.MX8M Plus audio-XCVR/Linux -> Aurora, subject to real compressed/JOC/MAT capture evidence.

Do not invent ALSA device names, reset/drop counters, hardware timings, supported compressed formats, AVDECC entity identities, stream IDs, multicast MACs, or measured acoustic results. Use actual physical evidence when hardware is present.

## 8. Key repository map

- core/layout/config: `crates/aurora-core/`, `crates/aurora-scene/`, `crates/aurora-config/`;
- renderers: `crates/aurora-renderer-api/`, `aurora-renderer-basic/`, `aurora-renderer-vbap/`, external adapters under `adapters/`;
- decoders: `crates/aurora-decoder-api/`, `aurora-decoder-*`;
- DSP: `crates/aurora-dsp-api/`, `aurora-dsp-basic/`, `aurora-dsp-camilladsp/`;
- audio I/O/network: `crates/aurora-audio-io/`, `crates/aurora-realtime-audio-*`, network adapters under `adapters/`;
- realtime engine: `crates/aurora-realtime-engine/`;
- open pro-audio: `config/open-audio-stack-v1.json`, `validation/open-audio-stack/`, `docs/adr/0021-open-pro-audio-stack-integration.md`;
- ESP fanout/orchestration: `adapters/aurora-network-esp-avb/`;
- GenAVB talker/control/session: `adapters/aurora-network-genavb/`, `adapters/aurora-network-genavb-avdecc/`, `adapters/aurora-network-genavb-session/`, `.github/workflows/genavb-aaf-talker-ci.yml`;
- immersive/JOC: `validation/immersive/`; open immersive references: `validation/open-immersive/`;
- binaural: `validation/binaural/`; room correction: `validation/room-correction/`;
- physical ingress: `validation/physical/`; virtual hardware: `validation/virtual-hardware/`;
- registry/licenses: `config/external-components-v1.json`, `THIRD_PARTY_LICENSES.md`;
- roadmap: `docs/pre-hardware-roadmap-v5.md`.

## 9. Required base validation before merge

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

Also run every domain-specific gate touched by the change. GenAVB adapter/control changes must run `GenAVB AAF Talker CI`; open pro-audio config/source-policy changes must run `Open Audio Stack CI`; ESP fanout/orchestration changes must run `ESP-AVB Endpoint Contract CI`. Tooling/simulation/reference gates must never be reported as physical, RF, synchronization, interoperability, acoustic or perceptual proof.
