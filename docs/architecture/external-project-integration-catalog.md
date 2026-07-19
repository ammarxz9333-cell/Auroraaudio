# External Project Integration Catalog

## Status and purpose

- research state: ACTIVE
- implementation state: NOT_STARTED unless an existing Aurora issue or accepted PR says otherwise
- production capability claim: none
- governing rule: reuse commodity capability behind Aurora-owned contracts; do not couple Aurora Core to third-party data models, processes, protocols, or licenses

This catalog records open-source projects that can reduce implementation work. Inclusion is not authorization to add a dependency. Every production adoption requires a separate technical, license, maintenance, security, realtime, packaging, and evidence review.

## Integration architecture

```text
sources and metadata
    -> Aurora ingestion contracts
    -> Aurora scene/channel model
    -> Aurora renderer contracts
    -> Aurora DSP intent
    -> Aurora realtime engine
    -> Aurora output/transport contracts
    -> local or network receiver adapters

control plane
    -> process adapters
    -> discovery/control adapters
    -> calibration workflow
    -> bounded status and capability registry
```

Third-party types stop at their adapter boundary. File parsing, process control, JSON/YAML, network discovery, allocation-heavy analysis, filter design, and calibration computation are forbidden in the realtime callback.

## Candidate matrix

| Capability | Candidate | Decision | Preferred integration | Why it saves work |
| --- | --- | --- | --- | --- |
| ordinary media decode | Symphonia | ADOPT LATER | direct Rust dependency behind `aurora-audio-io` PCM contracts | demux, decode, sample conversion, metadata probing |
| realtime FIR/IIR, mixers, delays, crossovers | CamillaDSP | OPTIONAL ADAPTER | external process controlled outside callbacks | mature DSP engine, multiple host backends, websocket control ecosystem |
| synchronized network audio | Roc Toolkit | EVALUATE | isolated adapter or external process after measured transport requirements | packet loss recovery, clock-domain conversion, jitter handling |
| multiroom behavior reference | Snapcast | REFERENCE / BLACK-BOX INTEROP ONLY | no linking, vendoring, or code copying | mature grouping, buffered playback, synchronization and control concepts |
| Linux graph routing | PipeWire | PLATFORM ADAPTER | Linux-only backend behind Aurora device/output contracts | graph routing, clocks, device/session integration |
| low-level Linux devices | ALSA | PLATFORM ADAPTER | contained Linux backend | stable hardware PCM and timing access |
| ADM parsing, validation, conversion and rendering | EBU ADM Toolbox / libadm / libear | EVALUATE | offline tool or isolated native adapter | implements standards-oriented ADM processing and BS.2127 rendering foundations |
| ADM reference renderer | EBU ADM Renderer | REFERENCE / TEST ORACLE | offline comparison tool | reference implementation and conformance-oriented fixtures |
| binaural ADM rendering | Binaural NGA Renderer | REFERENCE / OFFLINE ORACLE | external offline test path | combines ADM rendering with virtual loudspeaker binauralization |
| object, channel and Ambisonic binaural rendering | Google Open Binaural Renderer | EVALUATE | isolated native adapter or differential test oracle | object, channel and Ambisonic paths with FIR-based binaural output |
| object, speaker, HOA and binaural rendering | libspatialaudio | EVALUATE | isolated C++ adapter after Aurora 3D renderer baseline | unified renderer useful for ADM/IAMF-oriented comparisons |
| SOFA HRTF loading | libmysofa | PLANNED CANDIDATE | small reviewed native adapter crate | avoids implementing SOFA parsing and interpolation infrastructure |
| IAMF reference tooling | AOMedia iamf-tools / libiamf | PLANNED CANDIDATE | offline conformance first; runtime adapter only after format scope is fixed | parser, metadata and reference-format behavior |
| room simulation | pyroomacoustics | DEVELOPMENT TOOL | Python offline simulator only | image-source/RIR simulation and algorithm evaluation |
| acoustic source mapping and array analysis | Acoular | DEVELOPMENT TOOL | Python offline analysis only | beamforming, acoustic maps, microphone-array analysis |
| realtime source localization and tracking | ODAS | EXPERIMENTAL ADAPTER | external process with bounded JSON/status ingestion | direction finding, tracking, separation and beamforming foundations |
| measurement and automatic FIR generation | HiFiScan | DEVELOPMENT PROTOTYPE | offline measurement/filter-generation adapter | sweep measurement and exportable correction FIR generation |
| correction-filter generation | CamillaFIR / DRC-FIR | DEVELOPMENT TOOL | offline subprocess producing reviewed filter artifacts | avoids writing initial room-correction filter design from scratch |
| rendering filter execution | Aurora DSP or CamillaDSP | RETAIN DUAL PATH | Aurora-owned basic path plus optional external process | preserves ownership while accelerating advanced deployments |
| HDMI-CEC control | Linux CEC framework, `cec-ctl`, libCEC | PLATFORM ADAPTER | non-realtime control service | power, active-source and volume-control plumbing without protocol reimplementation |
| discovery | mDNS/DNS-SD implementation selected later | EVALUATE | replaceable discovery adapter | receiver/service discovery without embedding topology assumptions in Core |
| AirPlay receiver | Shairport Sync | OPTIONAL EXTERNAL SERVICE | separate process, never canonical transport | mature AirPlay timing and source integration |
| Spotify Connect source | librespot | OPTIONAL EXTERNAL SERVICE | separate source process feeding Aurora PCM | avoids implementing proprietary service behavior in Aurora Core |
| low-cost stereo network endpoint hardware | Sonocotta ESParagus Media Center | HARDWARE / FIRMWARE REFERENCE; EVALUATE | isolated Aurora endpoint experiment using the open ESP32 hardware as-is or through a separately reviewed receiver adapter | ready-made ESP32, DAC/amplifier, Wi-Fi/Bluetooth and optional Ethernet designs reduce endpoint hardware bring-up work |
| broad media pipeline and test ingestion | GStreamer | DEVELOPMENT / EDGE ADAPTER | external pipeline or isolated adapter when required | device, container, RTP and diagnostic pipeline coverage |
| resampling | rubato | RETAIN | direct Rust dependency behind Aurora resampler contract | high-quality synchronous/asynchronous resampling primitives |
| FFT | RustFFT | RETAIN | direct Rust dependency behind Aurora DSP/math contracts | avoids custom FFT implementation |

## Smooth composition rules

### One canonical model

Aurora owns exactly one canonical model for:

- PCM format and semantic channels;
- source/object positions and timestamps;
- loudspeaker layout and capabilities;
- DSP graph intent;
- clock, latency and synchronization observations;
- health, diagnostics and lifecycle state.

Adapters translate at the edge. CamillaDSP YAML, Snapcast JSON, ADM classes, ODAS coordinates, PipeWire node types, ALSA handles, SOFA handles, or GStreamer objects must never become canonical Aurora APIs.

### Separate runtime roles

```text
Aurora Core process
    - renderer
    - basic DSP
    - realtime engine
    - bounded queues
    - capability and lifecycle state

optional external services
    - CamillaDSP
    - ODAS
    - Shairport Sync
    - librespot
    - future transport service

control/orchestration service
    - configuration materialization
    - process supervision
    - discovery
    - calibration workflow
    - status aggregation
```

A failed optional service must degrade one advertised capability, not terminate unrelated local playback.

### Artifact-based calibration boundary

```text
measurement capture
    -> immutable measurement artifact
    -> offline analysis/filter design
    -> validation report
    -> approved FIR/IIR artifact
    -> Aurora DSP intent
    -> Aurora DSP or CamillaDSP execution
```

Aurora must preserve raw measurements, microphone calibration identity, sample rate, sweep definition, speaker identity, listener/microphone position, generated filter checksum, target curve, limits, and validation results. A filter generator is not trusted merely because it produces a file.

### Speaker localization boundary

ODAS and Acoular are candidates for experiments, not proof of automatic speaker geometry. Speaker discovery requires Aurora to emit a known signal from one endpoint at a time, associate the measurement with the commanded endpoint, estimate direction/time-of-flight with calibrated synchronized microphones, reject ambiguous reflections, and report uncertainty. Manual confirmation remains mandatory until physical evidence proves reliability.

### Low-cost endpoint hardware boundary

The Sonocotta ESParagus Media Center is an open ESP32 hardware and firmware family suitable for evaluation as a low-cost stereo endpoint reference. Its published variants provide either line-level stereo output, a small integrated stereo amplifier, or a higher-power stereo Class-D amplifier with Ethernet. The supplied firmware is based on `squeezelite-esp32` and supports ordinary streaming-source and smart-home use cases.

For Aurora, ESParagus is not an eARC, HDMI, LPCM 5.1/7.1, object-audio, or cinema-rendering solution. It must not be presented as a finished Aurora receiver. Any experiment must preserve these boundaries:

- Aurora remains responsible for endpoint identity, assignment, clock and latency observations, capability reporting, synchronization evidence, health, reconnect behavior and safe degradation;
- the endpoint is initially stereo-only and may be evaluated for multiroom music or a single two-channel output role, not as proof of wireless-surround suitability;
- upstream firmware protocols and configuration models remain outside Aurora Core and require an isolated adapter or a deliberately replaced firmware image;
- hardware schematics, PCB files, firmware dependencies, component availability, security/update behavior and all license obligations require a pinned-version review before reuse or distribution;
- synchronization accuracy, jitter tolerance, buffer bounds, startup latency, reconnect time, Wi-Fi interference behavior, Ethernet behavior where available, DAC/amplifier noise and thermal limits require physical measurement;
- adoption must include a bypass/replacement path so Aurora is not coupled to one ESP32 board family.

A successful ESParagus experiment may reduce the cost and schedule of Aurora endpoint prototyping. It does not alter the hardware gate for HDMI/eARC acquisition or the evidence required for cinema-grade distributed playback.

### Rendering adoption boundary

External renderers first serve as differential oracles against Aurora fixtures. They do not enter the realtime product path before:

- exact supported input semantics are documented;
- layout and coordinate conversions are tested;
- latency and memory bounds are measured;
- malformed metadata behavior is defined;
- license and native packaging are approved;
- Aurora can bypass the adapter and report capability absence honestly.

## Problems these projects do not solve completely

The following remain Aurora-owned product work:

1. cinema-session orchestration and deterministic mode switching;
2. one semantic channel/object model across local, wireless and binaural outputs;
3. receiver assignment, topology, health and recovery;
4. measured latency compensation across independent local and network clock domains;
5. automatic calibration workflow, evidence, rollback and safety limits;
6. user-facing setup and diagnostics;
7. HDMI/eARC capture hardware qualification and lawful codec/input boundaries;
8. resource admission, graceful degradation and capability honesty;
9. end-to-end tests spanning source, renderer, DSP, transport and receiver.

## HDMI/eARC finding

No open-source software project discovered so far turns arbitrary commodity Raspberry Pi hardware into a reliable universal eARC multichannel capture front end. CEC control is separable and comparatively mature, but eARC physical/link capture, EDID negotiation, protected/compressed formats, driver support and stable multichannel LPCM acquisition depend on hardware and kernel/driver capability.

Therefore Aurora must model HDMI/eARC as a hardware-gated input adapter. The first product path remains generated/file PCM and ordinary supported audio devices. Any HDMI/eARC claim requires a named capture device, exact formats, EDID behavior, channel mapping, measured latency, reconnect tests and legal review.

## Recommended adoption sequence

1. finish PR #58 evaluation and the stacked structural cleanup;
2. complete the local Aurora runtime vertical slice;
3. add Symphonia minimal ingestion;
4. implement capability registry and process-supervision seams;
5. finish Aurora offline 3D renderer baseline;
6. evaluate libmysofa, Google OBR, libspatialaudio and EBU renderers as separate differential-oracle experiments;
7. implement artifact-based calibration using existing measurement/filter tools before inventing new filter design;
8. evaluate ODAS/Acoular only in simulation and controlled microphone-array experiments;
9. qualify Linux CEC independently from HDMI/eARC audio capture;
10. specify transport and endpoint requirements, then compare Aurora-owned transport, Roc, black-box Snapcast behavior and an isolated ESParagus stereo endpoint experiment;
11. perform physical receiver and multiroom validation only after deterministic simulation gates pass.

## Per-project adoption checklist

Each project requires its own PR or decision record containing:

- upstream repository and pinned version/commit;
- license and redistribution obligations;
- maintenance activity and supported platforms;
- dependency and native/unsafe inventory;
- exact adapter boundary and forbidden dependency directions;
- realtime reachability analysis;
- CPU, memory, latency and failure evidence;
- malformed-input and restart behavior;
- deterministic fixtures and differential tests;
- capability reporting when absent or failed;
- uninstall/bypass path;
- third-party notices and supply-chain update procedure.

## Explicit non-decisions

This catalog does not authorize copying third-party source, importing all candidates, combining them into one process, replacing Aurora-owned contracts, or claiming Atmos/Dolby/DTS compatibility. Projects are selected only when they remove measured work without creating a larger long-term ownership, licensing, realtime, or packaging problem.
