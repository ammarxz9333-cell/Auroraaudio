# ADR-0019: Evaluate GStreamer as Aurora's Media Pipeline Layer

- **Status:** Candidate / Validation Required
- **Date:** 2026-07-19
- **Decision scope:** Media transport, pipeline orchestration, format handling, and integration boundaries

## Context

Aurora requires a reliable media-processing layer for audio capture, routing, format conversion, buffering, clock handling, and network transport. Reimplementing these facilities from scratch would increase engineering risk, maintenance burden, and validation cost.

GStreamer is a mature, modular multimedia framework with Linux and ALSA integration, pipeline composition, clocking, buffering, resampling, format conversion, RTP/network support, and a large plugin ecosystem. The `gst-plugins-rs` project additionally provides first-class Rust plugins and bindings that align with Aurora's Rust-oriented system architecture.

Aurora must nevertheless retain ownership of its product-defining behavior and must not become inseparably coupled to a single framework before real-time performance is demonstrated.

## Decision

Aurora will evaluate GStreamer as a **candidate media pipeline layer** between Linux hardware interfaces and Aurora-owned DSP/control components.

The proposed layered architecture is:

```text
Hardware
  -> Linux kernel drivers
  -> ALSA / DRM-KMS / network interfaces
  -> GStreamer media pipeline (candidate)
  -> Aurora-owned DSP, routing, synchronization, and control
  -> ALSA / network / hardware outputs
```

GStreamer is not, at this stage, a mandatory dependency of the Aurora real-time core. Adoption requires successful validation against Aurora's latency, determinism, reliability, and portability requirements.

## Responsibilities retained by Aurora

Aurora remains authoritative for:

- speaker topology and channel mapping;
- room correction;
- crossover and bass management;
- gain, delay, polarity, and routing policy;
- calibration workflows;
- wireless speaker synchronization policy;
- distributed clock-discipline logic specific to Aurora;
- presets, diagnostics, telemetry, and recovery behavior;
- product configuration and device orchestration;
- real-time DSP kernels and their ABI.

These components must remain Aurora-owned and testable independently of GStreamer.

## Responsibilities GStreamer may provide

Subject to validation, GStreamer may be used for:

- ALSA capture and playback integration;
- media graph and pipeline orchestration;
- buffer transport between components;
- sample-format conversion;
- channel-layout conversion where explicitly controlled;
- resampling outside latency-critical Aurora-owned paths;
- RTP and other network transports;
- source/sink abstraction;
- plugin loading and lifecycle management;
- integration with Rust through `gst-plugins-rs` and the GStreamer Rust bindings.

## Integration constraints

1. Aurora DSP kernels must not depend directly on GStreamer buffer ownership semantics.
2. An internal Aurora audio-block interface must isolate the DSP core from the pipeline framework.
3. The real-time path must avoid unbounded allocation, blocking I/O, locks with unbounded wait time, and uncontrolled queue growth.
4. Pipeline latency must be explicit, measurable, and configurable.
5. Automatic resampling, channel mixing, format negotiation, and buffering must not occur invisibly in production paths.
6. Clock selection and synchronization behavior must be observable through diagnostics.
7. Aurora must retain the option to replace GStreamer in specific paths if performance or maintainability requirements are not met.

## Validation requirements

GStreamer may move from Candidate to Adopted only after a reproducible evaluation demonstrates:

- stable multichannel operation;
- bounded end-to-end latency;
- acceptable jitter;
- zero underruns/overruns during long-duration stress tests;
- predictable CPU and memory consumption;
- correct channel order and sample-format handling;
- reliable device hot-plug and recovery behavior;
- deterministic restart behavior after pipeline failure;
- compatibility with Aurora's simulation and diagnostics framework;
- acceptable behavior on supported Linux targets, including Raspberry Pi-class hardware.

The evaluation must include at minimum:

- stereo, 5.1, and 7.1 PCM pipelines where hardware permits;
- 48 kHz as the primary cinema baseline;
- multiple period/buffer configurations;
- ALSA-to-Aurora-to-ALSA loopback testing;
- network transport tests with loss, jitter, and clock drift;
- long-duration soak testing;
- comparison with a minimal direct-ALSA reference path.

## Non-goals

This decision does not solve or authorize:

- HDMI or eARC input hardware support;
- HDCP handling;
- Dolby, DTS, or other licensed codec decoding;
- Atmos object decoding;
- replacement of ALSA or Linux kernel drivers;
- browser/WebRTC integration as a core Aurora requirement.

## Consequences

### Positive

- Aurora can reuse a mature media framework instead of rebuilding generic infrastructure.
- Rust integration is available through `gst-plugins-rs`.
- Transport and device support can be expanded with less custom code.
- Aurora's engineering effort remains focused on DSP, synchronization, calibration, and product behavior.

### Risks

- Hidden buffering or negotiation may increase latency.
- Framework-level abstractions may complicate deterministic real-time behavior.
- Plugin behavior may differ across GStreamer versions and Linux distributions.
- Excessive coupling could make later replacement expensive.

These risks are mitigated through a strict adapter boundary, explicit negotiation, measurable latency budgets, pinned dependency versions, and a direct-ALSA comparison implementation.

## Current disposition

GStreamer and `gst-plugins-rs` are approved for prototyping and controlled evaluation only. They are not yet approved as the mandatory production transport layer for Aurora.

## References

- GStreamer project documentation
- `gst-plugins-rs`: https://gitlab.freedesktop.org/gstreamer/gst-plugins-rs
- ADR-0018: Linux and ALSA hardware abstraction architecture
