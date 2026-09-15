# Aurora Agent Reference — Canonical Living Handoff

> Read this file first. Keep it factual, compact, and current.
>
> **Maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge.

Last updated: **2026-09-15**

## 1. Source of truth and continuation rule

- Canonical long-lived branch: `main-v2`.
- Temporary feature/debug branches are allowed only for isolated work and should not become alternate baselines.
- If the user says **“كمل” / “continue”**, continue the first unfinished item under **Current work / Next actions**. Do not restart project discovery.
- Aurora stays hardware-neutral until the software/reference/resilience gates in `docs/pre-hardware-roadmap-v5.md` are satisfied.

Current canonical base:
- Phase 9 decoded-PCM upmix matrix is complete for the declared software subset: 5.1 -> 7.1.4, 5.1 -> custom 11.1.4, and 7.1 -> custom 11.1.4.
- PR #173 merged the Phase 10 pinned RoomEQ/CamillaDSP reference baseline.
- PR #174 merged the deterministic synthetic 7.1.4 RoomEQ lane and simulation-coverage registration.
- PR #175 merged the RoomEQ -> CamillaDSP PCM execution differential, role-aware 7.1/7.1.4 WAV mapping, real 12-channel CamillaDSP sentinel, fail-closed unsupported semantics, and non-vacuous RoomEQ reference regression at merge commit `ed46a671cae70fa14c14658f6df89ca6eb6378ca`.
- Phase 10 is complete for its declared **software/reference** scope. Physical room/DAC/speaker evidence remains separate and unproven.
- PR #176 merged the initial Phase 11 pinned binaural-reference baseline into `main-v2` at merge commit `0b2b1d722f473db4caee43510c04914b2d217104`.
- Active user-priority integration work is draft PR #179 on `open-audio-stack-integration`.
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

## 3. Recent merged software/reference milestones

- #158 — Aurora-vs-OAR 5.1 object semantic differential.
- #159 — software/runtime completion: adaptive clock-rate correction evidence, bounded reconnect recovery, panic-isolated decoder/runtime recovery, live JOC validation, placeholder regression audit, and sustained realtime/fault evidence.
- #160 — pinned JOCForge external fixture/conformance-generator lane.
- #161 — pinned IAMF stereo encode/decode independent reference cross-check using `iamf-tools` and `libiamf`.
- #163 — exact-pin FFmpeg compatibility matrix.
- #164 — exact-pin MPEG-H dual-oracle validation with Fraunhofer `mpeghdec` and Ittiam `libmpegh`.
- #165 — exact-pin EBU `libadm` ADM structure/round-trip validation.
- #166 — exact-pin EBU EAR ADM/BS.2127-oriented renderer reference lane.
- #167 — exact-pin SAF 3D-VBAP differential reference lane.
- #170 — Aurora 7.1.4 plus explicit custom 11.1.4 geometry/continuity validation against pinned SAF semantics.
- #172 — decoded-PCM upmix validation matrix through custom 11.1.4.
- #173 — exact-pinned RoomEQ + CamillaDSP Phase 10 external-reference baseline.
- #174 — deterministic RoomEQ 7.1.4 synthetic optimization, four PR-eligible LFE/sub topologies, phase/policy guards and simulation-coverage evidence.
- #175 — exact RoomEQ -> CamillaDSP real-PCM execution differential, 7.1/7.1.4 semantic channel mapping, real 12-channel CamillaDSP sentinel, unsupported-graph fail-closed behavior, and hardened non-vacuous RoomEQ reference test.
- #176 — exact-pinned Google OBR / EBU BEAR / `sofar` Phase 11 binaural-reference provenance and upstream build/test baseline.

These are software/reference milestones only. They do not establish physical eARC/DAC/acoustic behavior, protected-service compatibility, perceptual parity, or certification.

## 4. Phase 10 — room correction and system DSP — merged software scope

Pinned external references remain external to Aurora core:

### RoomEQ / `pierreaubert/autoeq`
- pin: `579dd7486024fc18ff219e31eb7337362814f602`;
- observed workspace version: `0.5.73`;
- root package license: `GPL-3.0-or-later`;
- integration: external optimization/validation reference; it generates correction intent rather than running inside Aurora's realtime callback.

### CamillaDSP
- pin: `05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74`;
- version: `4.1.3`;
- upstream license: `GPL-3.0-only OR MPL-2.0`;
- integration: external DSP executor/reference only.

Merged Phase 10 evidence includes exact provenance, deterministic 7.1.4 optimization, multi-seat/phase/policy guards, real 12-channel CamillaDSP execution, role-aware WAVE mapping, fail-closed graph semantics, and non-vacuous regression gates.

A green Phase 10 lane does **not** prove microphone/acoustic correction, physical DAC/speaker routing, measured physical latency, protected-service compatibility, Dolby/DTS/HDMI certification, or arbitrary unrepresented DSP graphs.

## 5. Phase 11 — binaural — baseline merged, semantic expansion pending

PR #176 is merged. Its exact external reference pins remain:

### Google OBR
- pin: `478dc7c752d5eccae534635139ff0253eee3a14a`;
- boundary: external binaural validation oracle only;
- licensing: BSD-style source license plus **Open Binaural Renderer Patent License 1.0**.

### EBU BEAR
- pin: `6127e897b941211051c2ad135ee09b00be2e6ae0`;
- license: Apache-2.0;
- upstream maturity: explicitly pre-release;
- boundary: independent external ADM-oriented binaural validation oracle only.

### `andreiltd/sofar`
- pin: `06a629292689e99841e5dacaa25c4c6298616ca6`;
- observed crate version: `0.3.0`;
- license: `MIT OR Apache-2.0`;
- exact `libmysofa` gitlink: `da9e4adc619ee3d1ae5e68da3ed14aa5e60b3ec1`;
- boundary: Rust-native SOFA/HRTF/convolution candidate only, not selected runtime implementation.

The merged baseline proves provenance and selected upstream build/test execution only. It does not yet prove Aurora binaural output, agreement between independent renderers, personalized HRTF quality, perceptual front/back/elevation performance, head-tracker hardware behavior, or measured latency.

## 6. Open pro-audio stack integration — active PR #179

Active branch: `open-audio-stack-integration`.

The integration preserves Aurora-owned decoder/scene/renderer/DSP/realtime/runtime boundaries instead of importing a second architecture.

### Implemented in PR #179
- `aurora-realtime-audio-api` now owns a backend-neutral `NetworkAudioTransport` contract with exact PCM shape validation, absolute media-frame timestamps, explicit clock discipline, bounded latency/rate-correction policy and fixed telemetry/fault events.
- The network transport contract is **worker-thread only**. Network I/O is forbidden from Aurora's audio callback.
- `aurora-realtime-engine` contains a preallocated callback -> network-worker bridge using bounded fixed-block PCM storage plus bounded metadata; sequence and media timestamp continuity fail closed.
- Allocation tests require steady-state callback-side bridge push/pop to allocate zero times after preparation.
- `aurora-realtime-audio-sim` contains a deterministic bounded `SimNetworkTransport` for common lifecycle/timestamp/overflow/format-drift semantics.
- `network-audio-transport-contract` is registered in `config/simulation-coverage-v1.json`; the Rust simulator exports its capability/fault catalogue through `validation/open-audio-stack/network_transport_sim_contract.py`, while executable evidence remains the Rust tests run by Open Audio Stack CI.
- `config/open-audio-stack-v1.json` pins exact upstream source candidates and defines the single-clock/single-rate-controller/single-production-renderer rules.
- The exact AOO probe now exercises client setup, source registration, stream start/stop, 12-channel 48 kHz/48-frame PCM-f32 processing, and Aurora media-frame -> AOO NTP timestamp mapping.
- The exact libspatialaudio probe configures 7.1.4, verifies 12 outputs and finite/non-silent object rendering. Its direct object path has a documented `(512-1)/2 = 255` sample compensation delay, so the probe renders two 256-frame blocks before evaluating steady-state output.
- The dedicated CI builds pinned AOO and VideoLAN `libspatialaudio`, verifies NXP GenAVB/TSN and SOF source/platform contracts, runs Aurora fmt/check/clippy/tests for touched realtime/network crates, and requires source-pin drift to fail closed.

### Exact candidates and boundaries

#### AOO
- pin: `dc2a5be2962ba02d6cebe297f31f2774f34e7bc7`;
- license: Standard Improved BSD;
- role: peer/wireless network-audio candidate behind Aurora's worker contract;
- useful mechanisms include timestamped PCM, jitter/loss handling, retransmission and adaptive clock-rate compensation;
- AOO is not Aurora's logical media-clock owner.

#### NXP GenAVB/TSN
- pin: `6b962d6c34b0c3f142295a213dfa70bda193b23d` (7.3.2);
- role: preferred wired AVB/TSN/Milan candidate on supported NXP platforms;
- license surface is mixed: main user-space stack is permissive/BSD-oriented while Linux modules include GPL surfaces; keep the boundary explicit;
- hardware timestamping, gPTP lock and Milan interoperability remain physical/platform evidence, not generic CI claims.

#### Sound Open Firmware
- pin: `11cfcaf8f46d5c02b1c30e8394d10351ccd00e7c`;
- role: i.MX8M Plus HiFi4 DSP execution candidate;
- Aurora continues to own DSP policy, semantic graph and coefficients;
- physical i.MX8MP firmware/topology execution is not yet proven.

#### VideoLAN `libspatialaudio`
- pin: `d149ed9744fd399b835c6f2920511f8cbcfce5ea`;
- license: LGPL-2.1-or-later;
- role: production spatial-renderer candidate behind a replaceable adapter/library boundary;
- exact upstream software accepts Aurora's 7.1.4/48 kHz object-render contract after its documented direct-path compensation latency;
- it cannot become the selected renderer until it passes Aurora realtime/allocation contracts and semantic differentials against Aurora plus independent EAR/SAF/OAR evidence where applicable.

### Clock/DSP ownership rules
- Aurora owns the logical media timeline.
- Exactly one adaptive sample-rate controller may own any clock-domain crossing.
- PTP/GenAVB may own the wired network-time mapping on a supported endpoint; AOO may track an asynchronous peer only when no other rate controller owns that crossing.
- Wired/wireless/local output fan-out occurs after common rendering and system DSP.
- RoomEQ generates correction intent; a runtime DSP target such as SOF may execute supported operations after explicit translation/validation.
- Only one production speaker renderer processes a block; EAR/SAF/OAR remain independent reference/oracle lanes.

### Truth boundary for PR #179
A green software CI can prove source pins, portable builds, Aurora contract behavior, deterministic simulation, AOO worker lifecycle/timestamp-format compatibility and bounded libspatialaudio 7.1.4 software output. It **cannot** prove i.MX8MP native eARC capture, GenAVB hardware timestamps/gPTP/Milan, SOF-on-HiFi4 execution, Wi-Fi/RF resilience, peer packet-loss recovery, multi-speaker physical synchronization, measured physical latency, or acoustic performance.

## 7. Current work / Next actions

Continue in this order unless the user explicitly changes priorities:

1. Make PR #179 green at its final head across repository-wide CI, Open Audio Stack CI, simulation/governance and sustained realtime checks; fix failures without weakening contracts.
2. After #179 is stable, implement a real AOO peer/network adapter only behind the callback -> worker bridge and add deterministic packet loss/reorder/reconnect/drift-soak evidence. The current lifecycle/process probe is not a production network path.
3. Add a `libspatialaudio` Aurora adapter and deterministic 7.1.4 semantic/realtime differential, including its 255-sample direct-path compensation behavior, before considering renderer selection. EAR/SAF/OAR remain independent comparison lanes.
4. Add NXP GenAVB/TSN and SOF platform adapters only with explicit i.MX8MP gating; generic CI may validate configuration/provenance but not claim physical platform behavior.
5. Resume Phase 11 binaural semantic differential/head-rotation/HRTF-transition work after the higher-priority open-audio integration is stabilized, unless the user changes priority again.
6. Keep physical tracker #143 visible; resume physical eARC/JOC and multichannel output validation when authorized hardware exists.

## 8. Physical acceptance critical path — tracker #143

Still unproven physically:
- continuous `eARC -> E-AC-3 JOC -> Aurora -> synchronous physical 7.1.4`;
- real Gate A capture under the merged validator;
- synchronous physical 12-channel DAC output and electrical channel mapping;
- protected-service Atmos through a legitimate TV/streamer -> eARC path;
- physical loopback latency/drift;
- acoustic correction/parity and amplifier/speaker design;
- physical wired/wireless multi-speaker network synchronization;
- physical head tracker and headphone/HRTF transfer behavior.

Two ingress paths are now relevant hypotheses, neither a product freeze:
- existing fallback validation chain: `authorized TV/player -> Lindy 38368 / SiI9437 project tap -> Linux capture host -> Aurora`;
- preferred low-component-count investigation: legitimate TV/player eARC -> native i.MX8M Plus audio-XCVR/Linux path -> Aurora, subject to real compressed/JOC/MAT capture evidence on the selected board/design.

Do not invent ALSA device names, reset/drop counters, hardware timings, supported compressed formats, or measured acoustic results. Use actual physical evidence when hardware is present.

## 9. Key repository map

- core/layouts: `crates/aurora-core/`, `crates/aurora-scene/`;
- renderers: `crates/aurora-renderer-api/`, `aurora-renderer-basic/`, `aurora-renderer-vbap/`;
- decoders: `crates/aurora-decoder-api/`, `aurora-decoder-*`;
- DSP: `crates/aurora-dsp-api/`, `aurora-dsp-basic/`, `aurora-dsp-camilladsp/`;
- audio I/O: `crates/aurora-audio-io/`, `crates/aurora-realtime-audio-*`;
- realtime engine: `crates/aurora-realtime-engine/`;
- open pro-audio integration: `config/open-audio-stack-v1.json`, `validation/open-audio-stack/`, `docs/adr/0021-open-pro-audio-stack-integration.md`;
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

Also run every domain-specific gate touched by the change. PR #179 must run `Open Audio Stack CI` plus repository-wide realtime/simulation/governance checks. Tooling/simulation/reference gates must never be reported as physical or perceptual proof.
