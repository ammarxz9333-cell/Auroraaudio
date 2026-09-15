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
- PR #173 merged the Phase 10 pinned RoomEQ/CamillaDSP reference baseline into `main-v2`.
- PR #174 merged the deterministic synthetic 7.1.4 RoomEQ lane, review remediation and simulation-coverage registration into `main-v2` at `ff3fe185bdd32538929edbe8148a80034efd18c7`.
- PR #175 contains the implemented RoomEQ -> CamillaDSP PCM execution differential and role-aware 7.1/7.1.4 WAV channel mapping. Its branch now contains the merged #174 base and must prove a fresh final head before merge.
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
- object decoding, channel-bed decoding, and synthetic upmixing are distinct capabilities;
- simulation is not runtime evidence; runtime evidence is not physical evidence;
- only physical loopback may be called measured physical latency;
- no DRM/Widevine/HDCP circumvention, protected-media extraction, or device/certification-state spoofing;
- no Dolby/DTS/HDMI certification or conformance claim from software CI;
- GPL/incompatible references remain external unless a deliberate licensing decision changes that boundary;
- every new simulator-testable capability must be declared in `config/simulation-coverage-v1.json` with executable healthy/fault evidence before it can be marked covered.

## 3. Recent merged software/reference milestones

- #158 — Aurora-vs-OAR 5.1 object semantic differential.
- #159 — software/runtime completion: adaptive clock-rate correction evidence, bounded reconnect recovery, panic-isolated decoder/runtime recovery, live JOC validation, placeholder regression audit, and sustained realtime/fault evidence.
- #160 — pinned JOCForge external fixture/conformance-generator lane.
- #161 — pinned IAMF stereo encode/decode independent reference cross-check using `iamf-tools` and `libiamf`; broader IAMF roadmap remains bounded to separately proven subsets.
- #163 — exact-pin FFmpeg compatibility matrix; 6.1.6 is Aurora's tested maintenance baseline and 9.0.1 remains a green evaluation candidate.
- #164 — exact-pin MPEG-H dual-oracle validation with Fraunhofer `mpeghdec` and Ittiam `libmpegh`.
- #165 — exact-pin EBU `libadm` ADM structure/round-trip validation.
- #166 — exact-pin EBU EAR ADM/BS.2127-oriented renderer reference lane.
- #167 — exact-pin SAF 3D-VBAP differential reference lane.
- #170 — Aurora 7.1.4 plus explicit custom 11.1.4 geometry/continuity validation against pinned SAF semantics.
- #172 — decoded-PCM upmix validation matrix through custom 11.1.4.
- #173 — exact-pinned RoomEQ + CamillaDSP Phase 10 external-reference baseline with fail-closed provenance evidence.
- #174 — deterministic RoomEQ 7.1.4 synthetic optimization, four PR-eligible LFE/sub topologies, phase/policy guards and repository-resident simulation coverage evidence.

These are software/reference milestones only. They do not establish physical eARC/DAC/acoustic behavior, protected-service compatibility, or certification.

## 4. Phase 10 — room correction and system DSP

Pinned external references remain external to Aurora core:

### RoomEQ / `pierreaubert/autoeq`
- pin: `579dd7486024fc18ff219e31eb7337362814f602`;
- observed workspace version: `0.5.73`;
- root package license: `GPL-3.0-or-later`;
- integration class: `external-process-validation-reference`;
- roles: multichannel room-correction optimizer/reference, multi-sub, crossover/bass management, FIR/IIR/hybrid, timing/phase alignment, CamillaDSP export reference.

### CamillaDSP
- pin: `05e9cfcdf43c0dfe078ed3feb8af4c8bd701fd74`;
- version: `4.1.3`;
- upstream license: `GPL-3.0-only OR MPL-2.0`;
- integration class: external DSP executor/reference only.

Merged baseline files from #173:
- `config/room-correction-reference-v1.json`;
- `validation/room-correction/room_correction_reference_evidence.py`;
- `.github/workflows/room-correction-reference-ci.yml`;
- `docs/room-correction-reference.md`;
- `config/external-components-v1.json`;
- `THIRD_PARTY_LICENSES.md`.

Merged synthetic lane from #174:
- `config/room-correction-synthetic-v1.json`;
- `validation/room-correction/room_correction_synthetic_evidence.py`;
- `.github/workflows/room-correction-synthetic-ci.yml`;
- `docs/room-correction-synthetic.md`;
- `config/simulation-coverage-v1.json` declaration.

The synthetic lane exercises deterministic 7.1.4 RoomEQ optimization, the four PR-eligible LFE/sub topologies, multi-seat phase guards, final-chain headroom/peak constraints and Stage 3 policy plumbing. Its evidence analyzer binds the contract pin to the exact tested RoomEQ checkout and requires both corrupted-log and corrupted-pin negative controls to fail closed.

RoomEQ -> CamillaDSP execution differential implemented in PR #175 / `phase10-camilladsp-differential`:
- `config/room-correction-camilladsp-differential-v1.json`;
- `validation/room-correction/camilladsp_differential_evidence.py`;
- `.github/workflows/room-correction-camilladsp-differential-ci.yml`;
- `docs/room-correction-camilladsp-differential.md`;
- `docs/camilladsp-channel-order.md`;
- `crates/aurora-dsp-camilladsp/src/lib_entry.rs`.

The differential lane builds exact-pinned CamillaDSP 4.1.3 with a generated-and-recorded dependency lock, validates RoomEQ-generated CamillaDSP configuration, executes the required real-PCM contracts for fractional group delay, polarity/delay, convolution FIR, LR crossover gain, peaking EQ, routed channel matrices and multi-sub coherent peak behavior, and requires unsupported graph semantics to fail closed. Aurora additionally proves a real 12-channel 7.1.4 sentinel through CamillaDSP.

Aurora's canonical 7.1/7.1.4 logical order places side surrounds before back surrounds, while WAVE_FORMAT_EXTENSIBLE serializes those roles in speaker-mask order. The role-aware CamillaDSP adapter therefore maps Aurora 7.1.4 logical indices to WAV physical indices as `[0,1,2,3,6,7,4,5,8,9,10,11]`; front center remains index 2. Custom/mismatched/duplicate role mappings fail closed. Legacy numeric adapter APIs retain file-order semantics.

A green Phase 10 software lane does **not** prove microphone/acoustic correction, physical DAC/speaker routing, measured physical latency, protected-service compatibility, Dolby/DTS/HDMI certification, or arbitrary unrepresented DSP graphs.

## 5. Current work / Next actions

Continue in this order unless the user explicitly changes priorities:

1. Finish PR #175 on its synchronized `main-v2` base and retarget the PR to `main-v2`.
2. Require fresh final-head green evidence for Aurora role-aware adapter tests, the real 12-channel 7.1.4 sentinel, exact CamillaDSP build/preflight, all required RoomEQ-to-CamillaDSP PCM contracts, unsupported-feature rejection, negative evidence mutation, Software Completion Audit, Room Correction Reference CI and any repository-wide gates triggered by the coverage/handoff edits; merge only when green and review threads remain resolved.
3. Once #175 is merged, Phase 10 is complete for its declared software/reference scope. Continue with Phase 11 binaural reference validation, then Phase 12 runtime-contract/realtime-safety hardening unless a higher-priority regression appears.
4. Keep physical tracker #143 visible in parallel; resume physical eARC/JOC validation when authorized hardware is available, but do not block truthful software-only progress on absent hardware.

## 6. Physical acceptance critical path — tracker #143

Still unproven physically:
- continuous `eARC -> E-AC-3 JOC -> Aurora -> synchronous physical 7.1.4`;
- real Gate A capture under the merged validator;
- synchronous physical 12-channel DAC output and electrical channel mapping;
- protected-service Atmos through a legitimate TV/streamer -> eARC path;
- physical loopback latency/drift;
- acoustic correction/parity and amplifier/speaker design.

Existing first-choice validation chain remains a validation hypothesis, not a product freeze:
`authorized TV/player -> existing Lindy 38368 / SiI9437 project tap -> Linux capture host -> Aurora -> USB UAC2 -> multichannel TDM/DAC -> 12 physical outputs`.

Do not invent ALSA device names, reset/drop counters, hardware timings, or measured acoustic results. Use actual physical evidence when hardware is present.

## 7. Key repository map

- core/layouts: `crates/aurora-core/`, `crates/aurora-scene/`;
- renderers: `crates/aurora-renderer-api/`, `aurora-renderer-basic/`, `aurora-renderer-vbap/`;
- decoders: `crates/aurora-decoder-api/`, `aurora-decoder-*`;
- DSP: `crates/aurora-dsp-api/`, `aurora-dsp-basic/`, `aurora-dsp-camilladsp/`;
- audio I/O: `crates/aurora-audio-io/`, `crates/aurora-realtime-audio-*`;
- realtime engine: `crates/aurora-realtime-engine/`;
- config/runtime: `crates/aurora-config/`, `aurora-runtime-assembly/`, `aurora-runtime-materialization/`, `aurora-runtime-inspection/`;
- immersive/JOC: `validation/immersive/`;
- open immersive references: `validation/open-immersive/`;
- virtual hardware: `validation/virtual-hardware/`;
- physical ingress: `validation/physical/`;
- room correction: `validation/room-correction/`;
- external registry: `config/external-components-v1.json`;
- license boundaries: `THIRD_PARTY_LICENSES.md`;
- roadmap: `docs/pre-hardware-roadmap-v5.md`.

## 8. Required base validation before merge

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

Also run every domain-specific gate touched by the change. For Phase 10 this includes `Room Correction Reference CI`, `Room Correction Synthetic CI`, and `Room Correction CamillaDSP Differential CI` as applicable. Tooling/simulation/reference gates must never be reported as physical proof.
