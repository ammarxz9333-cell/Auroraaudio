# Health Monitoring

## Status

- `authorization_state`: `PROPOSED`
- `execution_state`: `NOT_STARTED`

## Purpose

Health monitoring shall expose whether an Aurora endpoint is capable of delivering correct, synchronized, uninterrupted audio. Generic CPU and memory telemetry alone is insufficient.

## Health domains

Each endpoint shall report bounded telemetry for:

### Runtime

- process state and restart count;
- software and configuration versions;
- uptime and monotonic-time anomalies;
- CPU load, scheduling pressure, temperature, and thermal throttling;
- memory use, allocation growth, queue depth, and storage pressure.

### Audio path

- active sample rate, block size, channel count, and route;
- callback duration percentiles and deadline misses;
- buffer occupancy, underruns, overruns, discontinuities, and muted frames;
- clipping, invalid samples, non-finite values, and unexpected silence;
- DSP graph state and bounded processing latency.

### Synchronization

- local clock estimate and drift;
- synchronization error and correction rate;
- ASRC ratio, controller saturation, and stability indicators;
- inter-endpoint skew and resynchronization events.

### Network

- round-trip and one-way latency where measurable;
- jitter, packet loss, reordering, duplication, and burst loss;
- jitter-buffer occupancy and late-packet rate;
- link changes and controller reachability.

### Hardware

- audio-device presence and negotiated format;
- device resets, USB/I2S errors, and output availability;
- amplifier or transducer telemetry only when supported and traceable.

## Health state model

A device shall expose one primary state:

- `HEALTHY`;
- `DEGRADED`;
- `UNSYNCHRONIZED`;
- `AUDIO_PATH_FAILED`;
- `CONTROL_PLANE_DISCONNECTED`;
- `MAINTENANCE`;
- `UNKNOWN`.

State transitions must be based on versioned rules with hysteresis to prevent alert flapping.

## Alerts and diagnostics

Every alert shall include:

- timestamp and device identity;
- triggering metric and threshold;
- duration and recurrence count;
- affected zone or channels;
- suspected subsystem;
- bounded diagnostic actions;
- exact software/configuration revision.

Monitoring must not record user audio content by default. Diagnostic captures require explicit authorization, strict retention, and visible evidence labeling.

## Validation

The Validation Lab shall inject deadline misses, drift, packet bursts, thermal pressure, device loss, invalid samples, queue growth, and controller disconnection to verify state transitions and alert correctness.
