# Aurora Agent Reference — Canonical Living Handoff

> Read this file first. Keep it factual, compact, and current.
>
> **Maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge.

Last updated: **2026-09-17**

### Binaural software continuation

- #212 prepared FIR core, #213 bounded pose timeline, #214 zero-allocation proof and
  #216 world-to-head direction transforms are merged. #215 merged the isolated,
  exact-pinned SOFA/sofar FIR reference gate at `798ced82aa82c5638109fbbf6b166f4761eb81f4`.
- PR #217 (`head-pose-hrtf-preparation`) connects world-space object directions and an
  explicit pose/media-frame snapshot to bounded nearest-direction HRTF preparation on
  the control thread. It rejects stale/missing poses, malformed banks and uncovered
  directions and reuses the borrowed allocation-free FIR commit/crossfade path.
- Draft PR #218 (`head-pose-scheduler-v1`) adds `HeadPoseHrtfScheduler`: a single bounded
  control-owned candidate, stable object IDs tied to PCM channel order, monotonic filter
  generations and an exact target media-frame boundary. Boundary commit only borrows the
  candidate; release/cancel is a separate control-thread operation so candidate storage
  is not dropped at the realtime boundary.
- Draft PR #219 (`head-pose-clock-mapping-v1`) maps explicit tracker source-clock
  timestamps onto Aurora logical media frames with bounded source gaps, delivery lag/lead,
  transactional rejection and explicit reset/re-anchor semantics.
- Draft PR #221 (`head-pose-delivery-sim-v1`) adds deterministic AuroraSim delivery traces
  for healthy/jitter, burst loss, reorder, duplicate, timestamp jump, stale hold and reconnect.
  Reconnect resets both the clock mapper and scheduler pose epoch while Aurora media time,
  scheduler boundary history and filter generation continue.
- Validate #217 with `hrtf_preparation`, `prepared_binaural_allocation`, general CI and
  `Prepared Binaural SOFA FIR Reference CI`. Validate #218 with its scheduler/allocation
  tests, #219 with clock-mapping/integration tests, and #221 with the AuroraSim tracker
  profiles plus `head_tracker_hrtf`; all remain software/reference evidence only.
- After #221, the remaining pre-hardware tracker software gap is a bounded transport/control
  adapter that accepts real adapter-thread samples without device I/O or unbounded work in
  the audio callback. Physical tracker latency and perceptual HRTF quality remain unproven.

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
- #198 merged the Rust talker-side AVDECC prepare bridge and cross-language FFI probe at merge commit `e030cd9ee152352e156ebd597492b8e2721466f2`.
- #200 merged the six-stream AVDECC-owned GenAVB runtime at merge commit `7979bdbd0f1c0923166baa6e0ed87ffa59983404`.
- #201 merged deterministic whole-set failure, stale-state and reconnect recovery probes at merge commit `098c7ea32480259f93c4b6ed19d06fb5a0085812`.
- #202 merged the Linux/NXP one-listener hardware-ready host probe and strict epoch evidence protocol at merge commit `c209a74dddce1b1e0fc2ed326f8e61076ca39cd1`.
- #203 merged the fail-closed one-listener physical evidence correlator at merge commit `c4f744c1bfb3c788e1fd16f556e0f7e45ffa1312`.
- #204 merged exact-public-API NXP gPTP snapshots and before/after evidence bundling at merge commit `18c75cfe9388f1667546f46b96835dcd6a991ad1`.
- #205 merged exact-pin ESP ACMP/stream/RX/gPTP evidence tooling at merge commit `3efcc4005724b46fef021ecb7654ab54d744d5a3`.
- #206 merged the exact-pin wired ESP32-P4 validation-firmware build gate at merge commit `2470aa5ef33f93c829ffc4870d4c6047861aa3ed`; it pins compatible Scramble Tools ESP-IDF `eff8fd1d0b182429b1b574cba4ae8e9be7afa457` because stock 6.0.2 lacks required hardware-clock APIs. Build success remains compile/toolchain evidence only.
- #207 merged the one-command host/NXP/ESP one-listener evidence bundler at merge commit `991e63dceecb12ef65bb7fc6853621a204adf5d9`; `--physical-run` or `--fixture-mode` must be selected explicitly and fixture mode cannot emit `PHYSICAL-PASS`.
- #208 merged prepared-plan-driven native libspatialaudio materialization at merge commit `1e8b6fbd0e87046363983a54e17951ae8ddd6a1b`.
- #209 merged external PCM multi-block callback continuity at merge commit `9e5e55fbb89148e8e0f1f8adbfdad6899f0dc28d`; full callback input is validated before state advance and successive blocks consume successive PCM frames with zero callback allocations.
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
- #198 — Rust AVDECC-driven GenAVB talker preparation with same-shim validation and cross-language FFI proof.
- #200 — worker-side six-talker AVDECC-owned GenAVB runtime with canonical 7.1.4 fanout and whole-set fail-closed positive lifecycle proof.
- #201 — deterministic prepare/start/submit fault rollback, duplicate/reordered CONNECT, active DISCONNECT, stale-state and reconnect recovery proof.
- #202 — hardware-ready one-listener host probe with explicit epoch, AVDECC-owned stream identity and deterministic AAF test signal.
- #203 — same-epoch host/NXP/ESP evidence correlator with fixture-mode physical-pass prevention and fail-closed identity/clock/RX/time checks.
- #204 — exact-public-API NXP gPTP snapshot collection and stable-GM before/after evidence bundling.
- #205 — exact-pin ESP validation-firmware status instrumentation, machine-readable listener snapshots and fail-closed before/after evidence bundling.
- #206 — exact-pinned ESP32-P4 endpoint/ESP-IDF firmware build gate for the #205 validation instrumentation.
- #207 — one-command raw host/NXP/ESP evidence bundling into the existing fail-closed one-listener correlator.
- #208 — prepared-plan-driven libspatialaudio native materialization with exact renderer identity, media-contract and canonical 7.1.4 plan validation before native loading.
- #209 — external PCM continuity across multi-block callbacks, fail-closed whole-callback input validation and zero-allocation regression proof.

These are software/reference/tooling milestones unless a specific item explicitly states physical evidence.

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
- public `GENAVB_CTRL_GPTP` GM status and `GENAVB_CTRL_CLOCK_DOMAIN` status expose a direct machine-readable clock evidence path; Aurora treats a snapshot as locked only when the clock domain is `LOCKED`, source is INTERNAL/PTP_CLK and GM identity is non-zero;
- hardware timestamping, physical gPTP behavior, AVDECC/Milan interoperability and latency remain hardware evidence until collected on the target.

### Scramble Tools ESP endpoints
- `esp_avb` pin `5e75bd3ed91b5407a254a5e49bfc18fc35e6cbb9` (2.18.0, MIT): AAF PCM 24-bit/48 kHz, maximum two channels/stream, P4 Ethernet and C6 Wi-Fi endpoint modes;
- `esp_ptp` pin `5b7eec233a93733ae954beefb6df3bb9c12dc901` (1.2.3, Apache-2.0): IEEE 1588 / 802.1AS candidate; P4 hardware-timestamp and C6 software-disciplined paths;
- ESP listener binding is AVDECC/ACMP-oriented: talker entity + talker stream identity matter; raw packet arrival alone is not treated as the native plug-and-play connection lifecycle;
- exact-pinned code contains internal receive evidence primitives including `avb_stream_in_last_rx_us(...)` and `avb_get_stream_in_counters(...)`; the latter's `frames_rx` is sourced from the live stream RX context packet counter;
- exact-pinned `esp_ptp` status exposes active PTP profile, remote-clock validity and selected best-clock identity;
- exact-pinned ATDECC GET_COUNTERS command/response/unsolicited functions are still explicit not-implemented stubs, so controller-visible AECP GET_COUNTERS must not be claimed for this pin;
- merged #205 uses a narrow exact-source validation-firmware patch to surface those existing ACMP/RX/gPTP facts through the serialized `avb_status()` path and machine-readable before/after snapshots;
- merged #206 pins the upstream `ESP-AVB-Endpoint` app plus exact compatible Scramble Tools ESP-IDF SDK (see `docs/esp-p4-sdk-compatibility.md`) and compiles the instrumented wired P4 firmware in CI;
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

### AVDECC-owned GenAVB lifecycle — merged #194/#195/#196/#198/#200/#201/#202/#203/#204/#205/#206/#207
- #194 opens `GENAVB_CTRL_AVDECC_MEDIA_STACK` on the shared runtime, exposes the control RX fd, accepts only supported AAF/48 kHz/stereo/24-bit talker CONNECTs, caches exact stack-supplied `genavb_stream_params`, and invalidates them on matching DISCONNECT;
- `aurora_genavb_prepare_avdecc(...)` creates a talker from the cached stack-owned parameters, so stream ID, destination MAC, port, class and format are not invented by Aurora;
- #195 wraps that control channel in Rust and exposes only sanitized CONNECT/DISCONNECT events plus a worker-only opaque native handle while the channel is open;
- #196 maps exactly six deployment-selected AVDECC Stream Output descriptor indices to Front, Center/LFE, Surround, Back Surround, Top Front and Top Rear roles; duplicate/unknown indices, duplicate stream IDs/MACs, invalid media contracts, partial sets and mismatched DISCONNECTs fail closed;
- #198 exposes the AVDECC-driven talker prepare bridge through Rust with same-shim validation and a cross-language FFI probe;
- #200 adds `adapters/aurora-network-genavb-runtime`: one worker-owned AVDECC control channel + six-stream session gate + six AVDECC-prepared GenAVB talkers + canonical 12-channel-to-six-stereo fanout;
- #201 proves deterministic whole-set rollback/recovery for prepare/start/submit faults, duplicate/reordered CONNECT, active DISCONNECT, stale start and reconnect;
- #202 adds Linux `physical_single_listener_probe`: one real selected AVDECC CONNECT -> prepare from ACMP-owned parameters -> deterministic AAF send -> `HOST_PASS` with `physical_complete:false` and explicit epoch/timestamps;
- #203 adds `validation/physical/aurora_genavb_single_listener_evidence.py`: real PHYSICAL-PASS requires matching epoch/stream identity, NXP+ESP clock lock to one GM, ESP RX delta and time-window correlation; CI fixture mode can never emit PHYSICAL-PASS;
- #204 adds `genavb_gptp_snapshot.c` using exact public GM + clock-domain control APIs and `aurora_genavb_nxp_gptp_evidence.py` to form the before/after NXP evidence consumed by #203;
- #205 adds an exact-pin ESP validation-firmware instrumentation path, snapshot emitter and fail-closed before/after bundler that produces the ESP evidence schema consumed by #203;
- #206 adds a pinned upstream ESP32-P4 endpoint application and compatible exact ESP-IDF build gate for the #205 instrumentation;
- #207 adds `aurora_genavb_one_listener_run.py` to bundle raw host/NXP/ESP captures through the existing fail-closed evidence builders and final correlator in one operator command;
- exact-pin/native/Rust/tooling/build CI remains software/API/build evidence only; no real NXP service + ESP endpoint epoch has yet satisfied the complete physical gate.

### Sound Open Firmware / libspatialaudio
- SOF pin `11cfcaf8f46d5c02b1c30e8394d10351ccd00e7c`; i.MX8M Plus HiFi4 candidate; physical execution unproven.
- libspatialaudio pin `d149ed9744fd399b835c6f2920511f8cbcfce5ea`; LGPL-2.1-or-later; software-proven canonical 7.1.4 object-PCM adapter, explicit selection only, no physical/acoustic parity claim.
- #208 makes `PreparedRuntimePlan` the portable source of renderer kind/identity/media/topology intent before native libspatialaudio loading; the machine-local shim path remains external.
- #209 proves host external PCM continuity across callbacks larger than one engine block and fails closed on a short whole-callback input before media state advances.
- Remaining software gap in this lane: `RenderScene` is still supplied independently from the prepared plan. Before claiming plan-owned topology end to end, bind or validate scene speaker directions against prepared layout geometry without confusing normalized direction with physical speaker distance.

### Clock/DSP ownership
- Aurora owns the logical media timeline.
- Exactly one adaptive sample-rate controller may own a clock-domain crossing.
- GenAVB/ESP-PTP map Aurora time into PTP/gPTP; they do not redefine Aurora logical time.
- AVDECC/ACMP owns network stream connection identity/state on the GenAVB path; Aurora owns PCM production and worker lifecycle around that state.
- fanout occurs after common rendering/system DSP; only one production speaker renderer processes a block.

## 6. Current work / Next actions

Continue in this order unless the user explicitly changes priorities:

1. Finish PR #217 only after all final-head CI is green; merge it into `main-v2` without inflating the physical/perceptual truth boundary.
2. Finish draft PR #218: stable object identity, exact boundary scheduling and control-owned candidate lifetime remain fail-closed; merge only after #217 and its own required gates are green.
3. Finish draft PR #219: keep tracker source-clock -> Aurora media-frame mapping explicit, bounded and transactional; merge only after lower stacked PRs and required CI are green.
4. Finish draft PR #221: keep deterministic AuroraSim coverage for jitter, burst loss, reorder, duplicate, timestamp jump, stale hold and reconnect; explicit re-anchor must reset mapper + scheduler pose epoch without resetting Aurora media time or filter generation.
5. Add the remaining hardware-neutral tracker transport/control adapter with bounded queueing between a future device/adapter thread and the existing mapper/scheduler. No device I/O, allocation, locks or unbounded work may enter the audio callback.
6. Independently close the libspatialaudio prepared-plan/RenderScene geometry-binding gap; compare normalized directions with tolerance rather than raw meter coordinates, and keep all validation before native loading.
7. Physical NXP/ESP listener, physical head tracker, DAC/eARC and acoustic acceptance remain separate later gates when the required hardware is actually present.

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
- GenAVB talker/control/session/runtime: `adapters/aurora-network-genavb/`, `adapters/aurora-network-genavb-avdecc/`, `adapters/aurora-network-genavb-session/`, `adapters/aurora-network-genavb-runtime/`, `.github/workflows/genavb-aaf-talker-ci.yml`;
- GenAVB one-listener physical protocol: `docs/genavb-single-listener-physical-probe.md`, `adapters/aurora-network-genavb/examples/physical_single_listener_probe.rs`, `validation/physical/aurora_genavb_single_listener_evidence.py`;
- one-command one-listener bundler: `validation/physical/aurora_genavb_one_listener_run.py`, `docs/genavb-one-listener-runner.md`;
- NXP gPTP evidence: `adapters/aurora-network-genavb/native/genavb_gptp_snapshot.c`, `validation/physical/aurora_genavb_nxp_gptp_evidence.py`;
- ESP listener evidence: `validation/physical/aurora_patch_esp_avb_listener_evidence.py`, `validation/physical/esp_avb_aurora_snapshot.c`, `validation/physical/aurora_esp_avb_listener_evidence.py`;
- ESP32-P4 validation build reference: `config/esp-avb-p4-build-reference-v1.json`, `.github/workflows/esp-avb-p4-firmware-build-ci.yml`;
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

Also run every domain-specific gate touched by the change. GenAVB adapter/control/runtime changes must run `GenAVB AAF Talker CI`; open pro-audio config/source-policy changes must run `Open Audio Stack CI`; ESP fanout/orchestration changes must run `ESP-AVB Endpoint Contract CI`; ESP physical validation-firmware changes must run `ESP-AVB P4 Firmware Build CI`. Tooling/simulation/reference/build gates must never be reported as physical, RF, synchronization, interoperability, acoustic or perceptual proof.