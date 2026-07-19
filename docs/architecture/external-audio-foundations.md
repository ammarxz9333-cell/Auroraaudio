# External Audio Foundations Adoption Plan

## Status

- authorization state: PLANNED
- implementation state: NOT_STARTED
- scope: dependency and integration policy only
- production capability claim: none

This document defines how Aurora may reuse selected open-source audio projects without weakening Aurora-owned contracts, realtime guarantees, or licensing control. It does not authorize immediate implementation before the current evaluation, structural-cleanup, and integrated-runtime checkpoints are complete.

## Decision summary

| Project | Aurora decision | Integration form | Earliest checkpoint | License posture |
| --- | --- | --- | --- | --- |
| Symphonia | ADOPT | direct Rust dependency behind Aurora PCM contracts | after structural cleanup, before broad file-ingestion product work | MPL-2.0; enable only reviewed codec/format features |
| CamillaDSP | RETAIN AS OPTIONAL ADAPTER | separate external process controlled outside callbacks | usable for offline/prototyping paths now; production review after integrated runtime | prefer MPL-2.0 distribution path; ASIO feature prohibited because it makes the binary GPLv3-only |
| Roc Toolkit | EVALUATE, DO NOT ADOPT YET | isolated native adapter or external process, never a core dependency | after the local integrated-runtime vertical slice and transport requirements are measured | MPL-2.0; native C/C++ and FFI review required |
| Snapcast | REFERENCE ONLY | architecture study and black-box interoperability tests | during later multiroom design | GPL-3.0; no code copying, linking, vendoring, or Aurora binary dependency |

## 1. Symphonia: canonical media-ingestion candidate

### Purpose

Symphonia is the preferred candidate for demuxing and decoding ordinary audio files into Aurora-owned PCM buffers. It supports a broad set of formats and codecs while remaining pure Rust and using an MPL-2.0 license.

### Aurora integration boundary

Symphonia must remain behind an Aurora-owned ingestion contract. Symphonia packet, codec, sample-buffer, channel, and error types must not leak into renderer, DSP, realtime-engine, configuration, or CLI public contracts.

Canonical flow:

```text
file or seekable source
    -> control-thread format probe
    -> Symphonia demux and decode
    -> validated Aurora PCM description
    -> bounded caller-owned PCM blocks
    -> renderer / DSP / offline or realtime feeder
```

### Placement

The first implementation should extend `aurora-audio-io` rather than create another permanent crate. A new crate is justified only if file ingestion develops a durable responsibility that cannot remain cleanly separated inside `aurora-audio-io`.

### Realtime rule

No format probing, file access, metadata parsing, decoder construction, or unbounded decode operation may execute in an audio callback. Decoding must run on a control or feeder thread and communicate through preallocated bounded buffers.

### Initial feature scope

The first reviewed implementation should enable only the minimum formats needed by the integrated product path:

1. WAV/PCM regression compatibility;
2. FLAC;
3. Ogg/Vorbis if required by a concrete fixture or product use case.

MP3, AAC, MP4, and additional codecs must be enabled only after a codec-by-codec technical, patent, distribution, test-corpus, and platform review. Symphonia's `all` feature must not be enabled by default.

### Acceptance gates

- deterministic decode fixtures and checksums;
- malformed and truncated input tests;
- channel-layout conversion tests;
- sample-format conversion bounds;
- bounded feeder queue and backpressure policy;
- no callback-reachable Symphonia code;
- MSRV, Linux, Windows, rustdoc, and license inventory checks;
- existing WAV output and semantic-channel behavior preserved.

## 2. CamillaDSP: optional external DSP engine

### Purpose

CamillaDSP may accelerate experimentation and selected deployments for IIR/FIR filters, crossovers, delays, mixers, and room-correction filter execution. It must not replace Aurora's DSP contracts or become mandatory for the first integrated runtime.

### Integration form

CamillaDSP remains an external process controlled from a non-realtime thread. Aurora may generate validated configuration, launch or connect to the process, exchange bounded control information, and ingest process status. Aurora callbacks must never perform process, filesystem, YAML, websocket, or JSON work.

Canonical flow:

```text
Aurora validated DSP intent
    -> control-thread CamillaDSP configuration materialization
    -> external CamillaDSP process
    -> explicit process and stream lifecycle
    -> bounded status projection
```

### License policy

CamillaDSP offers GPLv3 or MPL-2.0 licensing. Aurora's approved path is the MPL-2.0 option. The optional CamillaDSP ASIO backend is prohibited in Aurora-managed distributions because its ASIO SDK dependency makes the resulting binary GPLv3-only.

Aurora must not copy CamillaDSP source into Aurora crates. Packaging CamillaDSP with a product requires a recorded third-party notice, source-availability procedure where applicable, exact build-feature inventory, and legal review.

### Product boundary

Aurora must retain an in-process basic DSP path that can execute without CamillaDSP. Configuration semantics should be Aurora-owned and materialized to CamillaDSP only by an adapter. CamillaDSP-specific keys must not become the canonical product configuration model.

### Acceptance gates

- explicit executable discovery and version reporting;
- configuration-schema translation tests;
- timeout, crash, restart, and malformed-status handling;
- no callback process control;
- deterministic offline fixtures;
- feature inventory proving ASIO is disabled;
- clear diagnostics when CamillaDSP is absent;
- equivalent bypass path using Aurora-owned DSP primitives.

## 3. Roc Toolkit: deferred transport candidate

### Purpose

Roc Toolkit provides real-time network audio, packet-loss recovery, clock-domain conversion, and latency profiles. These capabilities overlap strongly with Aurora's future synchronized transport and existing drift-control foundations.

### Current decision

Do not integrate Roc Toolkit now. Aurora must first complete the local `aurora run --config ...` vertical slice and measure its actual transport requirements. Adopting Roc before those requirements are fixed would introduce a native C/C++ dependency, FFI, another resampler and clock-control policy, packaging work, and duplicated ownership.

### Evaluation checkpoint

After the local runtime is accepted, create a dedicated transport decision record comparing:

- Aurora-owned packet transport over existing realtime-engine contracts;
- an isolated Roc adapter;
- Roc as an external process;
- interoperability rather than embedding.

The comparison must measure latency, jitter recovery, loss recovery, drift convergence, CPU, memory, reconnection, endpoint portability, protocol extensibility, and operational packaging.

### FFI boundary if selected

If Roc is selected later:

- place all unsafe/native code in one small adapter crate;
- expose only Aurora-owned safe Rust contracts;
- prohibit Roc calls from callback paths unless bounded behavior is demonstrated;
- pin and audit the native library version;
- document transitive native dependencies;
- provide simulated and physical network evidence;
- retain a transport capability registry so unavailable adapters are reported honestly.

### License posture

Roc Toolkit is MPL-2.0. This is compatible with evaluation and potentially with an isolated adapter, but it does not remove the need for file-level source obligations, third-party notices, native dependency review, and packaging analysis.

## 4. Snapcast: reference and interoperability target only

### Purpose

Snapcast is useful for studying mature multiroom concepts such as server/client topology, buffered playback, groups, zones, control APIs, and synchronization behavior.

### Prohibition

Snapcast is GPL-3.0. Aurora must not:

- copy or translate Snapcast implementation code;
- link Aurora binaries against Snapcast code;
- vendor Snapcast source into the Aurora workspace;
- use Snapcast as an Aurora library dependency;
- claim Snapcast implementation behavior as independently derived without a clean design record.

Allowed uses are limited to public documentation study, black-box behavior measurement, protocol/interoperability research where legally permitted, and independent Aurora design based on requirements rather than copied implementation.

## 5. Revised execution order

External foundations do not change the current priority order:

1. make the unified evaluation PR green and merge it;
2. complete and validate structural cleanup;
3. fix CLI feature combinations and decompose the CLI without behavior changes;
4. complete the local integrated-runtime vertical slice using existing WAV/test-signal inputs, CPAL, Aurora renderers, and Aurora DSP;
5. add Symphonia ingestion behind Aurora PCM contracts;
6. harden the optional CamillaDSP adapter without making it mandatory;
7. define measured multiroom transport requirements;
8. evaluate Roc Toolkit against an Aurora-owned transport design;
9. use Snapcast only as a reference and interoperability target;
10. begin physical multiroom validation only after deterministic simulation gates pass.

## 6. Fast vertical slice after cleanup

The first fast path remains intentionally small:

```text
WAV or generated test signal
    -> Aurora PCM
    -> Aurora renderer
    -> Aurora basic DSP
    -> preallocated realtime engine
    -> CPAL physical output
```

Symphonia broadens the source side only after this path is accepted:

```text
FLAC / Ogg / reviewed codec
    -> Symphonia on feeder thread
    -> bounded Aurora PCM queue
    -> existing accepted Aurora runtime path
```

CamillaDSP may be selected as an optional external DSP branch:

```text
Aurora PCM
    -> validated adapter configuration
    -> external CamillaDSP process
    -> physical output
```

Roc and multiroom are not part of the first vertical slice.

## 7. Non-goals

This plan does not authorize:

- replacing Aurora renderer or realtime-engine contracts;
- decoding inside callbacks;
- enabling every Symphonia codec;
- embedding Snapcast;
- adopting Roc before a measured decision record;
- making CamillaDSP mandatory;
- claiming Dolby, DTS, Atmos, HRTF, room correction, multiroom, or physical synchronization readiness;
- adding new product capabilities before current CI and PR-stack cleanup is complete.

## 8. Required implementation PR separation

Each adoption must use a separate PR:

1. Symphonia dependency and minimal WAV/FLAC ingestion;
2. bounded feeder integration and runtime use;
3. CamillaDSP adapter hardening and license manifest;
4. Roc evaluation fixtures and decision record;
5. any later Roc adapter implementation;
6. Snapcast interoperability tests, if ever required.

No PR may combine codec ingestion, DSP behavior, network transport, and structural refactoring.