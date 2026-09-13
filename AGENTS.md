# Aurora Agent Reference — Living Handoff

> Read this file first. It is Aurora's canonical compact handoff for coding agents.
>
> **Mandatory maintenance rule:** every meaningful code/schema/architecture/validation/PR/issue/critical-path change must update this file in the same PR or immediately after merge. Keep it factual and compact.

Last updated: **2026-09-13**

### PR #156 — software gap closure in progress (2026-09-13)

Branch: `gap-closure-software-p0`, base `main-v2`. Keep the PR draft until required final-head gates are green.

Implemented on this branch:
- hardware-neutral `StreamingDecoder` contract for live encoded audio; transport, codec data type, discontinuity, source PCM channel semantics and explicit object-id -> PCM-channel bindings are preserved instead of inferred;
- `LiveImmersiveRuntime` in `aurora-source-runtime`: bounded priming, mute/fail-closed states, explicit recovery, native-object enforcement, source PCM validation, fixed-bed routing, object gain rendering and final speaker-PCM mixing;
- media/control-side `PanicIsolatedStreamingDecoder` catches in-process decoder panics, latches the adapter faulted across ordinary `reset_stream()`/discontinuity handling, and permits re-arming only through explicit `recover_stream()`; `LiveImmersiveRuntime::try_recover()` keeps failed recovery faulted and begins a fresh priming epoch after successful recovery;
- real pinned Harletty JOC validation through the Aurora runtime contract, with plain E-AC-3 as a negative control. Immersive JOC Stack run `34765726373` passed the baseline, Harletty -> Aurora live runtime gate, OpenJOC differential lane and temporal fail-closed controls;
- fixed-storage clock-rate estimator with discontinuity trust reset, median-of-3 filtering and trusted feed-forward into the existing ASRC controller; ratio changes remain slew-limited;
- executable resilience evidence drives the real Rust `DriftController` estimator/feed-forward plus `RubatoAsrc` ratio/sample path for both +250 ppm and -250 ppm over 24 simulated hours while keeping a fixed-capacity ring bounded;
- transition resilience evidence covers bounded clock jitter through median-of-3 filtering and continuous +250 -> -250 ppm clock steps with correction movement constrained to <=2 ppm/update through the actual ASRC ratio path;
- unsupported adaptive controller/resampler/capacity faults now latch the production `AdaptiveDuplexConsumer` fail-closed: subsequent callbacks remain silent/Fatal without being miscounted as ordinary underflow or self-clearing through a synthetic clock-epoch reset; recovery is an explicit control-plane bridge rebuild;
- bounded reconnect attempts with exponential 250/500/1000/2000/4000 ms backoff, late successful reappearance, stable-run reset semantics, repeated successful-but-unstable reopen cycles, and explicit fail-closed exhaustion after five attempts are exercised through the real `DuplexStateMachine`;
- `Aurora Resilience Simulation CI` was added and the mandatory simulation coverage registry includes the baseline plus jitter/step/discontinuity/out-of-range/flapping profiles. The out-of-range profile additionally gates the persistent adaptive mute latch. Final-head validation still must be green before merge;
- Gate A-Live verdict is threshold-driven rather than unconditional PASS, retained diagnostics are bounded, and live ingress/stream tooling is exercised on Linux and Windows;
- paced wall-clock realtime-health soak runs alongside accelerated media-time tests; the integrated faulted soak evaluates 7.1.4 callback deadlines, bounded RSS, clock jitter/reset/reacquisition/slew and bounded device-loss recovery in one software process while keeping its virtual fault-control truth boundary explicit;
- `config/simulation-coverage-v1.json` records `live-joc-decoder-runtime` as software-reference covered and promotes `adaptive-clock-rate-correction` plus `device-reconnect-recovery` to virtual covered capabilities backed by executable profiles. `validate_simulation_coverage.py` aggregates the full-system simulator and the dedicated resilience capability catalogue so the Rust resilience proof is not falsely attributed to the older Python sink model.

Truth boundary: this closes software/runtime/virtual contracts only. It does **not** prove physical eARC, physical clock correction or hotplug behavior, any particular USB/TDM/DAC path, protected-service Atmos, acoustic performance or Dolby certification.

### Existing Simics / live-ingress evidence (2026-09-13)

- Intel Simics 7.84.0 vacuum + Python `pyobj` harness passes 21 profiles, including MMIO/DMA timing, fail-closed negatives, full DSP replay, speaker-load routing and a previously modeled UAC2/TDM transport contract. Healthy replay: 3,624,960 frames / 75.52 simulated seconds with zero healthy xruns.
- The existing staged Simics model includes a documented dual-TDM8/TDM16 example and ideal DAC/load model. **It is retained as one validation fixture, not a hardware selection.**
- Generic asynchronous-feedback contract tests tolerate +/-250 ppm when feedback follows the device and intentionally fail without feedback. This is protocol simulation, not USB PHY or firmware proof.
- `validation/physical/aurora_live_ingress.py` provides software Gate A-Live classification/relock; `validation/physical/aurora_alsa_iec61937_stream.py` provides a hardware-neutral ALSA slot-conversion adapter. Physical live eARC capture and real reset/drop counters remain unproven.
- `validation/simics/run-simics.ps1` is the standalone Simics entry; `validation/simics/run_tv_to_speakers.py` is the staged encoded-source-to-load validation fixture.
- No Netflix/DRM bypass, protected-stream extraction or device-identity spoofing is part of Aurora.

### Native ARM64 platform validation (2026-09-13)

- `.github/workflows/arm64-platform-validation.yml` passes on GitHub native `ubuntu-24.04-arm` with portable `-C target-cpu=generic` codegen.
- Complete workspace tests, release Aurora CLI/evaluator, Harletty JOC golden path and 7.1.4 rendering have passed on `aarch64-unknown-linux-gnu`.
- Neoverse-N2 Criterion evidence at 48 kHz / 256 frames showed large software headroom. It proves that runner only; it is **not** a performance proxy for any future board/SoC.
- The Omniphony PipeWire FFI portability fix uses `std::ffi::c_char`; ARM64 and x86_64 validation pass.

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
- hardware-neutral ALSA encoded-ingress capture adapter: PR #150, `1afe66ab40ab2c7391610d5e5def27b6ba426644`;
- mandatory simulation coverage / AuroraSim v2: PR #152, `b3a3d831062c1528720e7a25c7f42b4e5c227efe`;
- pinned external AOMedia OAR reference: PR #154, `eee052e66efbe2ca1312616d0567c9561cd74c3d`.

Completed trackers include #118, #119, #130, #132, #135, #141, #145, #147.

Issue #143 remains the physical end-to-end acceptance tracker, but it **must not select or freeze hardware before software/simulation requirements are complete and an explicit later hardware decision is made**.

## 2. Product goal and non-negotiable rules

Aurora is an open, modular, hardware-agnostic immersive-audio stack, primarily Rust.

Direction:
- realtime multichannel audio, initially 7.1.4 and expandable toward 11.1.4;
- replaceable source, decoder, renderer, DSP, audio-I/O, transport and hardware adapters;
- authorized TV/eARC ingress and physical multichannel output through capability-based adapters;
- later private wireless speaker/rear/multi-room transport;
- no SBC, MCU, CPU/SoC, DAC, eARC board, USB interface, amplifier, speaker product, AVR, soundbar or OS image may define core architecture;
- **hardware remains unselected until the software pipeline, simulator contracts and acceptance requirements are sufficiently closed to compare candidates against one stable interface.**

Rules:
- realtime callback: no allocation after preparation, locks, logging/formatting, filesystem/process access, config parsing/registry lookup, or silent device changes;
- prepare -> validate -> commit is transactional; failed candidates do not mutate active state;
- unknown/incompatible component IDs, schemas, contracts, capabilities, generations, channel roles or object/channel mappings fail closed;
- object decoding, channel decoding and synthetic upmixing are distinct; never silently substitute one for another;
- a native object path must preserve which decoded PCM channel carries each object; metadata without PCM binding is not sufficient proof of object rendering;
- configuration/simulation is not runtime or physical evidence;
- only physical loopback may be called measured latency;
- no DRM/Widevine circumvention, protected-media extraction or device-identity spoofing;
- no Dolby certification/conformance claim from software CI;
- GPL/incompatible references remain external;
- every new or materially changed simulator-testable capability must be declared in `config/simulation-coverage-v1.json` in the same change; a virtual capability may be marked `covered` only when it maps to an exported AuroraSim capability plus executable healthy/fault profile evidence. Software-reference evidence may be tracked separately without pretending it is physical or virtual-hardware proof.

## 3. Evidence truth

### Pinned moving JOC carrier

Dolby Digital Plus Online Delivery Kit v1.4.1 source: `Living-Room-Atmos_6ch_640kbps_ddp_joc.ec3`.

Pinned hashes:
- ZIP: `f94d5e3e933f756856686546763f42a8a5f16b10c264fc7af1d228acc09baa62`;
- untouched carrier: `2470373db2c3621d56a2852df070e140293e9a99fdaa07e5c06de3c86bec307f`;
- byte-identical suffix after dropping exactly malformed AU0: `0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0`.

OpenJOC 0.17.0 classifies AU0 as `MALFORMED_OAMD_METADATA` / reserved OAMD object-size index 3. The positive temporal lane removes exactly that 2560-byte AU without re-encoding.

Reference evidence: 2360 accepted AUs, 37,760 metadata updates, 15 dynamic object indices, 12-channel/48 kHz render for ~75.52 s with temporal diversity.

### PR #142 — moving-JOC software proof

Using the exact derived SHA above:
- IEC61937 type `0x15`, 2360 bursts;
- Harletty: 2360 frames, 48 kHz, 3,624,960 samples, zero resets;
- 2360 metadata frames, 35,400 object events, object IDs 10..24 position-varying;
- Omniphony: 12 channels, 48 kHz, 3,624,960 frames = 75.52 s;
- paced elapsed 75.604572284 s; realtime factor `0.9988813866483852`;
- zero xrun/underrun/overrun markers; final verdict `pass`.

This is software evidence only.

### PR #146 / #148 / #152 — virtual-system evidence

AuroraSim validates the pinned JOC path through deterministic virtual transport/output models on Linux and native Windows, including real Aurora output DSP. It checks frame accounting, channel activity/order/PCM identity and fail-closed faults including dropout, channel silence/swap, disconnect, excessive drift, sample-rate change, latency spike, non-finite PCM and transport padding corruption.

The existing transport/DAC fixtures are **examples used to exercise interfaces**, not selected product hardware. Simulated latency is not measured latency.

### PR #156 — executable resilience evidence

`crates/aurora-realtime-audio-sim/examples/resilience_evidence.rs` and `resilience_transition_evidence.rs` exercise the real Rust timing/recovery components rather than duplicating their behavior in Python:
- +250 ppm and -250 ppm virtual clock cases run for 86,400 simulated seconds using the real fixed-storage PPM estimator, `DriftController` feed-forward/slew and `RubatoAsrc` ratio/sample processing;
- the fixed-capacity virtual ring must remain bounded and finite for both directions;
- bounded window-to-window jitter is median-filtered before feed-forward is trusted;
- a continuous +250 -> -250 ppm clock step must be re-estimated and correction must converge through the ASRC ratio path without exceeding 2 ppm per controller update;
- discontinuity clears stale estimator/controller state before the reversed clock epoch is reacquired, while a 5000 ppm relationship fails closed;
- unsupported adaptive faults are additionally driven through the production `AdaptiveDuplexConsumer`; the first faulting callback and a subsequent callback must both remain silent/Fatal with zero ordinary-underflow increments and no self-generated clock-epoch recovery;
- reconnect success follows exactly 250, 500, 1000, 2000 and 4000 ms backoffs, preserves attempt history until `StableRunObserved`, then resets it;
- exhaustion performs the same five attempts and remains `Faulted` with no sixth attempt; repeated successful-but-unstable reopen cycles retain the same recovery history and also exhaust fail-closed.

The dedicated resilience capability catalogue is `validation/virtual-hardware/aurora_resilience_sim.py`; the mandatory coverage validator aggregates it with `aurora_full_system_sim.py`. This proves hardware-independent software behavior only, not physical clocks, backend hotplug or device reopen behavior.

### PR #149 / #150 — physical-ingress tooling only

`validation/physical/aurora_physical_ingress.py` validates canonical IEC61937 capture properties including E-AC-3 type `0x15`, burst grid, error flags, padding, payload reconstruction, timestamps and externally supplied reset/drop counters.

`validation/physical/aurora_alsa_iec61937_capture.py` can preserve raw ALSA capture, negotiated parameters, stderr and monotonic timing while converting supported 32-bit slot layouts into canonical IEC61937. Device identity is operator-selected; no device name or hardware model is embedded in Aurora core.

Tooling self-tests do not prove physical capture.

### Historical physical ingress proof — limited

A prior experiment demonstrated `eARC -> IEC61937 type 0x15 -> raw E-AC-3 -> FFmpeg 5.1 PCM` using a specific prototype chain. It proves only that historical setup and is **not** the current Aurora hardware architecture or a frozen procurement choice.

### Still not physically/external proven

- continuous physical `eARC -> E-AC-3 JOC -> Aurora live runtime -> synchronous physical 7.1.4`;
- real capture reset/drop truth from future selected ingress hardware;
- physical clock correction, device reconnect/hotplug, output clocking/electrical mapping or loopback latency;
- legitimate Netflix/other protected-service Atmos compatibility through Aurora;
- final DAC/amplifier/speaker design;
- acoustic parity with Samsung Q995-class systems;
- private wireless speaker-network latency/sync/reliability;
- thermal/EMI/production readiness;
- Dolby certification.

Software/reference work still queued includes IAMF/OAR integration/differential coverage beyond the pinned reference, ADM/BS.2127 reference coverage, binaural/head tracking, standards loudness and room-field-control research.

## 4. Hardware-selection policy / physical acceptance contract

**No hardware is currently selected.** Historical Pi, N100, STM32, Lindy/SiI9437, MCHStreamer, PCM3168A/CS42448 or other candidate names are reference experiments/candidates only unless a future explicit decision changes this section.

Aurora software must expose capability-driven boundaries for:
- encoded ingress and discontinuity/reset/drop reporting;
- decoder semantics and maximum source PCM/object capacity;
- render/DSP compute budget;
- clock/timestamp quality and independent clock domains;
- multichannel output channel count/layout/sample rate/format;
- realtime scheduling/memory guarantees;
- optional network/wireless transport.

A future hardware candidate is acceptable only if it satisfies those contracts and passes the same software/virtual acceptance gates plus device-specific physical tests. Hardware substitution must not require rewriting the decoder, renderer, DSP, timing or control-plane architecture.

## 5. Current architecture / repository map

Control plane:
`aurora-config -> aurora-runtime-assembly -> aurora-runtime-inspection -> aurora-runtime-materialization -> prepared components -> aurora-realtime-engine`

Media direction:
`encoded/source input -> StreamingDecoder/Decoder API -> source PCM + explicit object/channel semantics -> renderer/mixer -> DSP -> generic realtime audio I/O`

Key locations:
- core/layouts: `crates/aurora-core/`, `crates/aurora-scene/`;
- renderers: `crates/aurora-renderer-api/`, `aurora-renderer-basic/`, `aurora-renderer-vbap/`;
- decoders/contracts: `crates/aurora-decoder-api/`, `aurora-decoder-*`;
- live decoded-source runtime: `crates/aurora-source-runtime/src/live.rs`;
- DSP: `crates/aurora-dsp-api/`, `aurora-dsp-basic/`, `aurora-dsp-camilladsp/`;
- audio I/O: `crates/aurora-audio-io/`, `crates/aurora-realtime-audio-*`;
- realtime engine/clock recovery: `crates/aurora-realtime-engine/`;
- config/runtime: `crates/aurora-config/`, `aurora-runtime-assembly/`, `aurora-runtime-materialization/`, `aurora-runtime-inspection/`;
- immersive/JOC validation: `validation/immersive/`;
- Harletty-through-Aurora live contract proof: `validation/immersive/test-joc-aurora-live-runtime.sh`;
- full-system virtual hardware: `validation/virtual-hardware/`, `docs/aurora-full-system-sim.md`;
- resilience evidence: `crates/aurora-realtime-audio-sim/examples/resilience_evidence.rs`, `crates/aurora-realtime-audio-sim/examples/resilience_transition_evidence.rs`, `validation/virtual-hardware/aurora_resilience_sim.py`, `validation/virtual-hardware/test-aurora-resilience-sim.sh`;
- mandatory simulation coverage: `config/simulation-coverage-v1.json`, `validation/virtual-hardware/validate_simulation_coverage.py`;
- physical ingress tooling: `validation/physical/`;
- external components/licenses: `config/external-components-v1.json`, `THIRD_PARTY_LICENSES.md`.

## 6. Current work / Next actions

PR #156 is the current software-hardening critical path. Continue in this order:
1. Run final-head validation for PR #156 and fix every real failure before changing the PR out of draft. Required gates include workspace fmt/check/clippy/tests, Immersive JOC Stack, official/moving JOC lanes, simulation coverage, full-system Linux/Windows, resilience, sustained realtime soak and physical-ingress tooling.
2. Preserve the real Harletty -> Aurora live runtime gate, including plain E-AC-3 fail-closed negative control, complete object-id -> PCM-channel binding validation, panic isolation and explicit recovery/re-priming semantics.
3. Preserve the executable +/-250 ppm 24-hour estimator/feed-forward/ASRC proof, jitter/step/discontinuity/out-of-range transitions, persistent adaptive hard-fault mute latch, and bounded reconnect success/exhaustion/flapping proof.
4. Preserve the integrated paced faulted wall-clock soak so callback health, bounded memory, clock correction and recovery counters are evaluated together under faults. Do not equate accelerated media time with wall-clock endurance.
5. Keep ingress classification/relock bounded and fail-closed. A hardware adapter must provide honest reset/drop counters; do not synthesize missing counters.
6. After PR #156 is green, continue the Aurora-vs-pinned-OAR differential slice described in `docs/oar-reference.md`, then independent ADM/BS.2127 references where licensing/interfaces permit.
7. Keep IAMF/OAR, ADM, binaural/head tracking, standards loudness and other unimplemented capabilities explicitly planned until executable evidence exists.
8. **Do not choose physical hardware yet.** When software/simulation gaps are sufficiently closed, define a capability matrix and compare candidate ingress/compute/output implementations against it.
9. After a later explicit hardware decision, execute physical issue #143 using the selected adapters: preserve raw capture/evidence, validate canonical IEC61937/JOC, feed the unchanged Aurora live runtime, verify synchronous physical multichannel output, and measure loopback latency where possible.
10. Test legitimate protected streaming services only as a separate external-integration gate without DRM circumvention.

Other queued trackers:
- #115: evaluate newer Omniphony behind a separate reference lane; do not upgrade the stable pin by recency alone;
- #116: reassess Source Manager acceptance before closing.

## 7. Required validation before merge

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
```

Additional gates:
- immersive/JOC changes: `Immersive JOC Stack CI`, including the Harletty -> Aurora live runtime contract;
- moving reference: Official Dolby JOC Temporal CI;
- Aurora moving path: Aurora Moving JOC CI;
- simulation coverage contract: `python3 validation/virtual-hardware/validate_simulation_coverage.py self-test` and `check`;
- full-system virtual hardware: Aurora Full-System Sim Linux/Windows;
- executable clock/reconnect resilience: `Aurora Resilience Simulation CI`;
- realtime resilience: Sustained Realtime Health Soak;
- physical ingress/capture tooling: Aurora Physical Ingress Tooling CI Linux/Windows.

Tooling/simulation/software-reference gates must never be reported as physical proof. Do not merge while required gates are red.
