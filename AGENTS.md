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
- PR #175 merged the RoomEQ -> CamillaDSP PCM execution differential, role-aware 7.1/7.1.4 WAV mapping, real 12-channel CamillaDSP sentinel, fail-closed unsupported semantics, and non-vacuous RoomEQ reference regression into `main-v2` at merge commit `ed46a671cae70fa14c14658f6df89ca6eb6378ca`.
- Phase 10 is complete for its declared **software/reference** scope. Physical room/DAC/speaker evidence remains separate and unproven.
- PR #176 merged the Phase 11 pinned binaural-reference baseline into `main-v2` at merge commit `0b2b1d722f473db4caee43510c04914b2d217104`.
- Active Phase 11 work is PR #177 on `phase11-binaural-differential`.
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
- realtime callback: no allocation after preparation, locks, logging/formatting, filesystem/process access, config parsing/registry lookup, or silent device changes;
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
- #176 — exact-pinned Phase 11 binaural baseline with Google OBR, EBU BEAR, and `sofar`/`libmysofa` provenance, licensing/maturity boundaries, and executable upstream gates.

These are software/reference milestones only. They do not establish physical eARC/DAC/acoustic behavior, protected-service compatibility, perceptual parity, or certification.

## 4. Phase 10 — room correction and system DSP — merged software scope

Pinned external references remain external to Aurora core:

### RoomEQ / `pierreaubert/autoeq`
- pin: `579dd7486024fc18ff219e31eb7337362814f602`;
- observed workspace version: `0.5.73`;
- root package license: `GPL-3.0-or-later`;
- integration: external optimization/validation reference.

### CamillaDSP
- pin: `05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74`;
- version: `4.1.3`;
- upstream license: `GPL-3.0-only OR MPL-2.0`;
- integration: external DSP executor/reference only.

Merged Phase 10 evidence includes:
- exact source/license/dependency provenance;
- deterministic 7.1.4 RoomEQ synthetic optimization and four LFE/sub topologies;
- multi-seat phase guards, final-chain safety constraints and Stage 3 policy gates;
- RoomEQ-generated CamillaDSP preflight and required real-PCM contracts;
- unsupported CamillaDSP graph semantics fail closed;
- real software 12-channel 7.1.4 sentinel through exact CamillaDSP;
- Aurora logical 7.1.4 -> WAVE physical mapping `[0,1,2,3,6,7,4,5,8,9,10,11]` with FC fixed at 2;
- a named RoomEQ DSP-realization regression that must report `1 passed; 0 failed`, preventing the old zero-test/vacuous gate.

A green Phase 10 lane does **not** prove microphone/acoustic correction, physical DAC/speaker routing, measured physical latency, protected-service compatibility, Dolby/DTS/HDMI certification, or arbitrary unrepresented DSP graphs.

## 5. Phase 11 — binaural — active

Merged baseline: PR #176 at `0b2b1d722f473db4caee43510c04914b2d217104`.

Initial exact references:

### Google OBR
- pin: `478dc7c752d5eccae534635139ff0253eee3a14a`;
- boundary: external binaural validation oracle only;
- licensing: BSD-style source license plus **Open Binaural Renderer Patent License 1.0**; keep legal/patent conditions explicit;
- selected upstream evidence targets cover Ambisonic binaural behavior, x/y/z soundfield rotation, and CLI rendering for 3OA, channel-based 7.1.4 and object-mono inputs including mismatched-input rejection.

### EBU BEAR
- pin: `6127e897b941211051c2ad135ee09b00be2e6ae0`;
- license: Apache-2.0;
- upstream maturity: explicitly pre-release;
- boundary: independent external ADM-oriented binaural validation oracle only;
- pinned `flake.lock` revisions for EAR, libear, VISR, nixpkgs and flake-utils are part of the evidence contract;
- upstream Nix build enables BEAR unit tests and Python test phase.

### `andreiltd/sofar`
- pin: `06a629292689e99841e5dacaa25c4c6298616ca6`;
- observed crate version: `0.3.0`;
- license: `MIT OR Apache-2.0`;
- exact `libmysofa` gitlink: `da9e4adc619ee3d1ae5e68da3ed14aa5e60b3ec1`;
- boundary: Rust-native SOFA/HRTF/convolution **candidate only**, not selected runtime implementation;
- pinned source publishes no root `Cargo.lock`; CI generates one, records SHA-256 and tests with `--locked`.

Merged Phase 11 baseline files from #176:
- `config/binaural-reference-v1.json`;
- `validation/binaural/binaural_reference_evidence.py`;
- `.github/workflows/binaural-reference-ci.yml`;
- `docs/binaural-reference.md`;
- `config/external-components-v1.json` registrations;
- `THIRD_PARTY_LICENSES.md` boundaries.

Active PR #177 adds the first Aurora-vs-reference execution differential:
- exact-pinned OBR CLI built from `//obr/cli:obr_cli` and kept external;
- dependency-neutral Aurora `GeometricBinaural` probe from `aurora-renderer-basic`;
- deterministic 16-bit/48 kHz channel-isolated 7.1.4 impulse fixtures;
- canonical sequential OBR/Aurora order `FL, FR, FC, LFE, SL, SR, SBL, SBR, TFL, TFR, TRL, TRR` with no WAVE speaker-mask remap at this boundary;
- directional left/right/center semantic checks, finite/non-zero output, exact frame accounting and mirror-pair checks;
- LFE execution integrity only, with no spatial-direction claim;
- OBR-pin drift and channel-order drift negative controls.

The #177 differential compares bounded semantics rather than raw PCM because Aurora's geometric model and OBR implement different transfer functions. A green lane does not prove HRTF parity, front/back or elevation discrimination, personalized HRTF quality, head tracking, perceptual quality, physical latency, or certification.

## 6. Current work / Next actions

Continue in this order unless the user explicitly changes priorities:

1. Require fresh final-head CI for PR #177, especially `Binaural 7.1.4 Differential CI`, plus all repository-wide checks triggered by `AGENTS.md` and the new Rust example.
2. Fix any compile, OBR execution, threshold, frame-accounting, or review failures without weakening the semantic truth boundary. Merge #177 only when final-head checks are green and review threads are resolved.
3. Add deterministic object and Ambisonics binaural differentials against independent references; keep object/channel/HOA evidence distinct.
4. Add head-rotation, HRTF-transition continuity, finite-output, front/back and elevation evidence. Treat perceptual claims separately from deterministic software metrics.
5. Register each new simulator-testable Phase 11 capability in `config/simulation-coverage-v1.json` before marking it covered.
6. After the declared Phase 11 software/reference scope is complete, continue Phase 12 runtime-contract/realtime-safety hardening unless a higher-priority regression appears.
7. Keep physical tracker #143 visible in parallel; resume physical eARC/JOC validation when authorized hardware is available, but do not block truthful software-only progress on absent hardware.

## 7. Physical acceptance critical path — tracker #143

Still unproven physically:
- continuous `eARC -> E-AC-3 JOC -> Aurora -> synchronous physical 7.1.4`;
- real Gate A capture under the merged validator;
- synchronous physical 12-channel DAC output and electrical channel mapping;
- protected-service Atmos through a legitimate TV/streamer -> eARC path;
- physical loopback latency/drift;
- acoustic correction/parity and amplifier/speaker design;
- physical head tracker and headphone/HRTF transfer behavior.

Existing first-choice validation chain remains a validation hypothesis, not a product freeze:
`authorized TV/player -> existing Lindy 38368 / SiI9437 project tap -> Linux capture host -> Aurora -> USB UAC2 -> multichannel TDM/DAC -> 12 physical outputs`.

Do not invent ALSA device names, reset/drop counters, hardware timings, or measured acoustic results. Use actual physical evidence when hardware is present.

## 8. Key repository map

- core/layouts: `crates/aurora-core/`, `crates/aurora-scene/`;
- renderers: `crates/aurora-renderer-api/`, `aurora-renderer-basic/`, `aurora-renderer-vbap/`;
- decoders: `crates/aurora-decoder-api/`, `aurora-decoder-*`;
- DSP: `crates/aurora-dsp-api/`, `aurora-dsp-basic/`, `aurora-dsp-camilladsp/`;
- audio I/O: `crates/aurora-audio-io/`, `crates/aurora-realtime-audio-*`;
- realtime engine: `crates/aurora-realtime-engine/`;
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

## 9. Required base validation before merge

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

Also run every domain-specific gate touched by the change. Phase 11 pinned-reference work must run `Binaural Reference CI`; the channel-based differential must run `Binaural 7.1.4 Differential CI`. Tooling/simulation/reference gates must never be reported as physical or perceptual proof.
