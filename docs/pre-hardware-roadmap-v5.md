# Aurora Pre-Hardware Roadmap v5

Status: canonical planning document for work before hardware selection.
Research snapshot: 2026-09-14.

## Core rule

Aurora remains hardware-neutral until the software, reference-validation, resilience, and evidence gates below are complete. A project appearing in this roadmap does not make it a runtime dependency. New external projects enter first as pinned generators, validation oracles, interoperability references, or watch-list items.

Protected-service compatibility must use legitimate platform paths. Aurora will not depend on DRM/HDCP circumvention or on falsifying device/certification state.

## Phase 0 — Close the current software-completion baseline

1. Finish PR #159 without adding unrelated runtime features.
2. Fix formatting and make the final-head CI green.
3. Remove or explicitly de-scope incomplete runtime adapters; no fake support paths.
4. Reconcile capability/truth-boundary documentation with the implementation.
5. Merge only after the final software-only evidence is clean.

Gate: `main-v2` is a truthful, green software baseline before new integration lanes are added.

## Phase 1 — External-component governance

Every external component must record:

- exact tag/commit;
- archive/binary hash when applicable;
- license and redistribution constraints;
- integration class;
- supported role;
- CI/evidence path;
- explicit capability boundary.

Allowed integration classes:

- `runtime`
- `validation-oracle`
- `fixture-generator`
- `interop-reference`
- `hardware-reference`
- `hardware-watchlist`
- `deferred`

No floating `latest` or unpinned `main` is accepted in reproducible evidence.

## Phase 2 — E-AC-3/JOC validation

### JOCForge

Use JOCForge as a pinned fixture/conformance generator, not as an Aurora runtime encoder.

Representative CI must cover profiles, block partitions, dependent substreams, LFE ownership, EMDF variants, legal topology changes, CMAF where supported, and negative/malformed cases. Full exhaustive matrices may run separately from pull-request smoke CI.

### Differential lane

Compare Aurora/Harletty, OpenJOC, and generated expected topology semantically:

- duration/frame count;
- channel/object topology;
- object IDs/positions/gains where observable;
- LFE behavior;
- finite/non-silent PCM expectations;
- deterministic failure classification;
- recovery after discontinuity/reconnect.

Do not require byte-identical PCM across independent renderers.

## Phase 3 — IAMF validation

Use independent references:

- `libiamf`
- AOMedia `iamf-tools`
- AOMedia OAR

Validate Audio Elements, Mix Presentations, parameter blocks, gain automation, coordinate metadata, malformed OBUs, containers, rendered-channel PCM, and failure/recovery behavior.

IAMF is not `REFERENCE-VALIDATED` until independent references agree on the supported semantic subset.

## Phase 4 — FFmpeg modernization

Keep a compatibility matrix before changing the runtime baseline.

Test the current supported line against a current stable FFmpeg for:

- E-AC-3 channel-bed decode;
- resampling and format conversion;
- channel ordering;
- IEC 61937 related workflows used by Aurora;
- IAMF/container behavior where relevant;
- malformed-input behavior.

Raise the baseline only when differential tests are green.

## Phase 5 — MPEG-H 3D Audio reference lane

Add two independent external validation references:

- Fraunhofer IIS `mpeghdec`
- Ittiam `libmpegh`

Use them as codec/render references for channel-based, object-based, and HOA/scene-based paths. Do not make MPEG-H a production claim until a bounded supported subset has repeatable evidence.

## Phase 6 — ADM infrastructure

Use EBU `libadm` for metadata-level cross-checking and fixture tooling.

Validate:

- ADM/BW64 parsing;
- malformed ADM behavior;
- scene/metadata mapping;
- round-trip metadata where representable;
- object/direct-speaker/HOA test assets.

## Phase 7 — Spatial-renderer differential suite

Primary speaker-render references:

- Aurora renderer
- AOMedia OAR
- EBU EAR/libear
- Spatial Audio Framework (SAF)

Secondary/experimental references:

- VideoLAN `libspatialaudio`
- Cult-DSP `spatialroot` after license review
- VISR after license review
- SoundScape Renderer as a limited secondary oracle

Compare semantics rather than exact PCM:

- dominant speaker;
- normalized energy distribution;
- spatial centroid;
- azimuth/elevation error;
- mirror symmetry;
- gain response;
- trajectory continuity;
- LFE exclusion;
- NaN/Inf safety.

## Phase 8 — Layout expansion

Validate in order:

1. stereo regression;
2. 5.1;
3. 7.1;
4. 7.1.4;
5. 11.1.4.

Test anchors, arbitrary coordinates, horizontal/vertical/diagonal trajectories, simultaneous objects, beds plus objects, and object-count pressure.

7.1.4 is the first production-like immersive software target. 11.1.4 follows after the same classes of evidence pass.

## Phase 9 — Upmix validation

Keep upmix evidence separate from true object/JOC/IAMF rendering.

Required lanes:

- 5.1 -> 7.1.4
- 5.1 -> 11.1.4
- 7.1 -> 11.1.4

Metrics include dialogue anchoring, LFE preservation, spectral balance, surround decorrelation, bounded height energy, phase stability, clipping/headroom, and pumping artifacts.

## Phase 10 — Room correction and system DSP

### Optimization/reference

Evaluate `pierreaubert/autoeq` RoomEQ as a pinned external optimization/reference engine for:

- multichannel correction;
- multi-sub optimization;
- crossover and bass management;
- driver/sub timing and phase alignment;
- FIR/IIR/hybrid-phase correction;
- listening-area optimization;
- safety/acceptance gates.

### Runtime executor

Keep CamillaDSP as the current open runtime DSP executor/reference where it remains appropriate.

`DecayCore` remains external-comparison-only unless its licensing/open-source boundaries change.

## Phase 11 — Binaural

Independent binaural oracles:

- Google OBR
- EBU BEAR

Evaluate `sofar` as a Rust-native SOFA/HRTF/convolution implementation candidate, not as an automatic runtime choice.

Validate channel-based immersive input, objects, Ambisonics, head rotation, HRTF transitions, front/back discrimination, elevation, and continuity.

## Phase 12 — Runtime contracts and real-time safety

Every decoder/renderer adapter follows one lifecycle:

`create -> configure -> prime -> process -> discontinuity -> recover -> shutdown`

Standardize error classes for unsupported formats, malformed streams, metadata mismatch, clock discontinuity, transient I/O, decoder/render faults, and output-capacity faults.

Real-time paths must avoid panic propagation, unbounded allocation, blocking network/filesystem work, process spawning, unbounded locks, unbounded queues/history, and uncontrolled diagnostics.

Use `tpt-dsp` as a Rust DSP-primitives reference for filters, transforms, resampling, queues/ring buffers, and possible future embedded/no_std work. Do not adopt it blindly as a dependency.

Evaluate `discoLink` concepts for versioned shared-memory IPC between Aurora core and isolated decoder/render workers: SPSC audio rings, discovery, buffer negotiation, capability/version handshakes, and process isolation.

## Phase 13 — Linux-native integration

Add/validate a proper PipeWire adapter for:

- 7.1.4/11.1.4 channel maps;
- clock negotiation;
- hotplug/reconnect;
- graph recovery;
- latency reporting;
- XRUN handling.

PipeWire DSP is a host/output facility, not a replacement for Aurora's DSP/render core.

## Phase 14 — Soak, recovery, and evidence

Run progressive soak classes: short CI, 1 h, 8 h, 24 h.

Scenarios include static/moving JOC, IAMF, immersive layouts, renderer/profile switching, decoder restart, repeated reconnect, clock drift, underruns, timestamp jumps, metadata discontinuity, and long-run memory growth.

Track at least RSS, queue depth, callback deadline/miss metrics, recovery/mute duration, ASRC correction, and sample-count drift.

Every major validation lane emits a machine-readable evidence bundle containing Aurora SHA, external SHA, fixture hash, format/layout, sample rate, duration, metrics, verdict, and truth boundary.

## Phase 15 — TrueHD

Do not restore the removed incomplete adapter.

Evaluate `truehdd` first as an external validation reference. A production Aurora TrueHD adapter returns only after stable decode, deterministic error handling, metadata evidence, and real-time viability are demonstrated.

Until then: TrueHD is `UNSUPPORTED`, not simulated as supported.

## Phase 16 — Network / multi-room research

Compare independent approaches before selecting a transport:

- Roc Toolkit — low-latency resilient transport reference;
- OpenSonic/Soluna — RTP/UDP, sync, FEC/NACK, Wi-Fi/ESP32 reference;
- AES67 Linux Daemon — professional AES67/PTP interoperability reference;
- Sendspin — presentation timestamps, discovery, conformance, whole-home sync ideas;
- Panaudia/LASA — object-based spatial streaming over Media over QUIC;
- NXP GenAVB/TSN — future deterministic Ethernet/gPTP/AVTP reference;
- Taktwerk/SonicStream — watch-list only until maturity improves.

Aurora's network abstraction must not be limited to rendered PCM. Preserve a protocol-neutral model capable of carrying:

- stream ID and sequence;
- presentation timestamp;
- clock domain/epoch;
- sample rate/layout/frame count;
- audio payload;
- optional object pose/spatial metadata;
- discontinuity/restart state;
- FEC/retransmission metadata;
- sync-quality/drift telemetry;
- integrity flags/checksum.

A network fault lab must simulate packet loss, burst loss, reordering, duplication, jitter, clock drift, reconnects, node pause/resume, roaming, and timing-leader changes.

## Phase 17 — Hardware Reference Design Survey

No hardware is frozen in this phase. The goal is to avoid designing solved subsystems from scratch.

### Primary references

#### NXP MCIMX8M-AUD

Role: primary HDMI/eARC and high-channel-count architecture reference.

Study its HDMI/eARC daughterboard path, SAI/I2S/TDM routing, networking, and up-to-24-channel DAC output architecture. Treat proprietary/NDA HDMI/HDCP/eARC IC internals as a compliant external front-end boundary, not as code or circuitry to reverse-engineer around protection systems.

Reference:
- https://www.nxp.com/design/design-center/development-boards-and-designs/MCIMX8M-AUD

#### TI TIDA-01414

Role: soundbar channel/amplifier/acoustic architecture reference.

Use its 5.1.2 topology, multichannel amplification, power architecture, crossover/DRC/DSP partitioning, and PCB/layout lessons. Proprietary Dolby decoding is not an Aurora dependency.

Reference:
- https://www.ti.com/tool/TIDA-01414

#### TI TIDA-060026

Role: scalable TDM Class-D amplifier reference.

Use schematic/BOM/Gerber/layout concepts for a modular amplifier stage scalable beyond the original configuration.

Reference:
- https://www.ti.com/tool/TIDA-060026

#### OHDSP ADAU1452 + ADAU1966 boards

Role: open multichannel DSP/TDM/DAC reference.

Study reusable/open design patterns for TDM routing, clocks, DSP control, and a 16-channel DAC back end before creating an Aurora-specific board.

References:
- https://github.com/ohdsp/DSP-ADAU1452
- https://github.com/ohdsp/DAC-ADAU1966

### Secondary references

#### freeDSP-aurora

Study integration of ADAU1452, XMOS USB audio, DAC/ADC, clocks, SPDIF/ADAT, ESP32 control, Wi-Fi, and self-boot.

Reference:
- https://github.com/freeDSP/freeDSP-aurora

#### freeDSP Infinitas

Study high-channel-count USB/TDM/FPGA routing architecture and clock-domain partitioning.

Reference:
- https://freedsp.github.io/

#### Insane Soundbar

Use only as a mechanical/PCB/wireless-sub reference. It is currently a 2.1 design rather than an Aurora immersive architecture. Its hardware files are CC BY-NC-SA 4.0 and therefore must not become the basis of a commercial Aurora product without separate permission. Its software/firmware is MIT.

Reference:
- https://github.com/babeinlovexd/Insane-Soundbar

#### Qualcomm QCS407

Closed commercial benchmark only. Use it to understand the capability/partitioning expected from production soundbar SoCs; do not make it an open-core dependency.

## Phase 18 — Candidate physical architecture to evaluate, not yet freeze

A leading reference architecture for study is:

`TV/eARC -> compliant eARC front-end -> Linux SoC -> Aurora core -> TDM16/24 -> multichannel DAC -> scalable Class-D amplifiers -> speakers`

This is a hypothesis for evaluation, not a hardware selection.

The selection gate must compare cost, licensing, availability, clocking, Linux support, channel count, measurable latency, maintainability, open documentation, manufacturability, and long-term sourcing.

## Phase 19 — Legitimate protected-service / Netflix path

Aurora should behave as an audio endpoint/soundbar, not as a DRM circumvention layer.

Preferred production path:

`Netflix app on a supported TV/streamer -> HDMI -> TV -> eARC -> Aurora`

Alternative supported source path:

`supported streaming device (for example a Shield-class device) -> HDMI -> TV -> eARC -> Aurora`

The source device/TV remains responsible for Netflix DRM, device certification, and protected video handling. Aurora receives the legitimate audio output presented by the platform over eARC.

Do not implement or document:

- HDCP key extraction/bypass;
- DRM decryption/circumvention;
- spoofing licensed/certified device state;
- forcing a service to expose protected streams it would not normally output.

Protected-service compatibility remains `EXTERNAL-PENDING` until tested end-to-end on supported hardware and services.

## Phase 20 — Physical Acceptance Plan

Before hardware purchase/freeze, define pass/fail tests for:

- eARC capture and reconnect;
- IEC 61937 identity/classification;
- live DD+ and DD+ JOC;
- IAMF/MPEG-H where the source path legitimately supplies them;
- 7.1.4/11.1.4 channel ordering;
- DAC loopback;
- end-to-end latency and jitter;
- clock drift and long-run sync;
- hotplug/recovery;
- wireless/multi-room sync;
- 24 h soak;
- analog frequency response;
- THD, THD+N, IMD, SNR/ENOB as applicable;
- mute/unmute and fault containment.

Use `gavv/signal-estimator` as an external latency/jitter/glitch measurement reference and `dgo42/Phonalyser` as an external electrical/analog measurement workbench where applicable.

## Phase 21 — Capability truth model

Every capability must be one of:

- `PROVEN-SOFTWARE`
- `REFERENCE-VALIDATED`
- `SIMULATED`
- `PHYSICAL-PENDING`
- `EXTERNAL-PENDING`
- `UNSUPPORTED`

Track codec support, metadata support, rendering support, network support, and physical support independently. Passing a simulator never upgrades a physical claim.

## Phase 22 — Placeholder/capability guard

CI must reject accidental support inflation: `todo!`, `unimplemented!`, fake decoder paths, dummy metadata, hard-coded PASS results, silent unsupported fallback, and unsupported capability claims must be explicitly prevented or allow-listed only for legitimate test fixtures.

## Hardware-freeze exit gate

Do not select/freeze the production board until all of the following are true:

1. PR #159 and subsequent software-validation lanes are green and truthful.
2. JOC and IAMF independent differentials pass for their declared subsets.
3. Renderer validation reaches 7.1.4 and 11.1.4 targets.
4. Upmix, room-correction, runtime-safety, soak, and recovery gates pass.
5. Network architecture has a measured prototype plan and fault model.
6. Hardware reference designs have been compared for license, cost, sourcing, channel count, clocks, Linux support, and manufacturing risk.
7. The legitimate eARC/protected-service path is defined without DRM/HDCP circumvention.
8. Physical acceptance tests exist before hardware is ordered.

Only after this gate does Aurora enter hardware selection and prototype implementation.
