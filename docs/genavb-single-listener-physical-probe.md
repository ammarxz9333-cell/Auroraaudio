# GenAVB single-listener physical probe

Status: hardware-ready validation harness. This document does **not** claim physical interoperability until evidence from a real NXP GenAVB host and a real ESP AVB listener is captured.

## Purpose

This is the first physical network gate after the software-only AVDECC/GenAVB lifecycle and fault/recovery work.

The gate deliberately starts with one wired listener before six endpoints or Wi-Fi are introduced:

`NXP Linux GenAVB host -> Ethernet AVB network -> one ESP32-P4 AVB listener`

The first listener should use the pinned Scramble Tools `esp_avb`/`esp_ptp` software and a wired ESP32-P4 endpoint. A wired P4 keeps RF, Wi-Fi bridging and software-clock discipline out of the first interoperability measurement.

Pinned references:

- NXP GenAVB/TSN 7.3.2: `6b962d6c34b0c3f142295a213dfa70bda193b23d`
- Scramble Tools `esp_avb`: `5e75bd3ed91b5407a254a5e49bfc18fc35e6cbb9`
- Scramble Tools `esp_ptp`: `5b7eec233a93733ae954beefb6df3bb9c12dc901`

The pinned NXP API supplies `genavb_stream_params` to a media application through `GENAVB_MSG_MEDIA_STACK_CONNECT` on `GENAVB_CTRL_AVDECC_MEDIA_STACK`. Aurora therefore uses the ACMP/AVDECC-owned stream identity and does not invent a stream ID, destination MAC, port or stream class.

## Test epoch

Every physical evidence source in one run must use the same operator/controller-supplied `epoch_id`. The allowed form is 1–64 ASCII alphanumeric characters plus `-`, `_` or `.`. A recommended value is a UTC timestamp plus a short run suffix, for example `20260916T193000Z-run01`.

The host probe prints an `ARMED` line containing the epoch before it waits for AVDECC CONNECT. Use that exact epoch in the NXP gPTP snapshot and the ESP listener before/after evidence. Evidence with a missing or different epoch must fail closed even when individual counters look healthy.

## Host probe

The Linux-only example is:

```text
adapters/aurora-network-genavb/examples/physical_single_listener_probe.rs
```

Invocation:

```bash
cargo run --release \
  --manifest-path adapters/aurora-network-genavb/Cargo.toml \
  --example physical_single_listener_probe -- \
  /path/to/libaurora-genavb-shim.so \
  <stream-output-index> \
  <epoch-id> \
  [duration-ms=5000] \
  [target-latency-frames=480] \
  [connect-timeout-ms=30000]
```

The native shim must be built for the target NXP Linux system against the same pinned GenAVB/TSN API used by Aurora's exact-pin CI. The NXP GenAVB service and AVDECC stack must already be running.

The probe:

1. validates and records the explicit `epoch_id`;
2. opens `GENAVB_CTRL_AVDECC_MEDIA_STACK` through Aurora's shared native shim and prints `ARMED`;
3. polls the real GenAVB control file descriptor;
4. waits for a supported AVDECC CONNECT for the selected Stream Output descriptor index;
5. records the AVDECC-owned stream ID, destination MAC, port and stream class;
6. prepares the talker from the exact cached CONNECT parameters;
7. starts a 48 kHz / stereo / 24-bit AAF stream with 48-frame blocks;
8. sends a deterministic 997 Hz antiphase stereo signal at 0.10 full-scale amplitude for the requested duration;
9. continuously checks the control channel and fails if the selected stream disconnects while sending;
10. stops the talker and emits machine-readable host evidence.

The host JSON schema is:

```text
aurora.genavb.single-listener-host-evidence.v1
```

It contains the epoch plus host/connect/send Unix-millisecond timestamps, AVDECC stream identity and media/send parameters. A successful summary line is:

```text
aurora-genavb-single-listener-host: PASS epoch_id=<epoch> connect=avdecc prepare=from-acmp send=aaf24-48k-stereo listener-rx=required gptp-lock=required
```

`HOST_PASS` is intentionally **not** a complete physical verdict and the JSON remains `physical_complete:false`.

## Existing ESP listener telemetry

The pinned `esp_avb` source already contains listener-side evidence primitives, so the first physical gate does not require an Aurora-specific firmware fork merely to prove receive activity:

- `avb_stream_in_last_rx_us(state, index)` reports the most recent input-stream frame arrival in the ESP timer domain;
- listener state maintains monotonic `stream_bytes_received` data for the media-clock/reference path;
- `avb_get_stream_in_counters(...)` exposes STREAM_INPUT counters through the existing ATDECC/AECP implementation;
- the built-in periodic diagnostics call `avb_stream_in_print_diag()` and retain network/PTP receive diagnostics separately.

For the first wired P4 test, prefer controller-visible ATDECC/AECP STREAM_INPUT counters when available, with a raw serial diagnostic capture retained as corroborating evidence. The useful assertion is a **before/after delta during the exact host test epoch**, not merely a non-zero lifetime counter.

Do not accept audible output, an LED, a stale lifetime counter or raw Ethernet packet presence by itself as listener receive proof. The evidence must bind the active STREAM_INPUT/stream identity to increasing receive activity during the host probe.

## Required evidence for a complete one-listener physical gate

A complete one-listener evidence bundle must contain all of the following from the same `epoch_id`:

1. **NXP host / AVDECC evidence**
   - `HOST_PASS` for the common epoch;
   - real CONNECT received for the selected Stream Output index;
   - stream ID, destination MAC, port and class recorded from the CONNECT;
   - talker prepared from those parameters;
   - continuous successful AAF submits for the declared duration;
   - no target DISCONNECT during the send interval.

2. **NXP gPTP evidence**
   - the same `epoch_id`;
   - the relevant NXP network port is link-up and gPTP-capable;
   - the GenAVB clock used by the stream is synchronized for the test epoch;
   - the raw NXP gPTP status/log excerpt is retained in the evidence bundle.

3. **ESP listener evidence**
   - the same `epoch_id`;
   - the real ESP32-P4 ATDECC entity is discovered;
   - ACMP reports the intended listener connection;
   - the listener reports the same stream identity advertised in the host CONNECT;
   - STREAM_INPUT/AAF receive activity increases between before/after samples bracketing the host send interval;
   - listener-side gPTP/clock state is retained for the same epoch.

4. **Identity and time correlation**
   - all evidence objects carry the identical `epoch_id`;
   - Stream ID and connection identity match across host AVDECC evidence and listener evidence;
   - listener before/after samples bracket or overlap the host `send_started_unix_ms`–`send_ended_unix_ms` interval;
   - unrelated prior connections/counters cannot satisfy the gate.

Only when all four evidence groups pass may this gate be labelled `PHYSICAL-PASS: one-listener AVDECC/ACMP + AAF + gPTP`.

## Negative physical checks

The same hardware setup must later prove:

- listener unplug/disconnect is observed and host sending fails closed;
- reconnect does not reuse stale prepared state;
- a fresh CONNECT and prepare/start epoch is required before audio resumes;
- a mismatched `epoch_id` or unexpected stream identity cannot satisfy the evidence correlator;
- loss of acceptable gPTP state invalidates the physical gate even if packets still move.

## Scope boundary

This gate does not prove:

- six-listener synchronization;
- Wi-Fi/C6 behavior or RF resilience;
- measured inter-speaker skew;
- physical loopback latency;
- eARC/JOC end-to-end playback;
- Milan certification or conformance;
- acoustic performance.

Those remain separate physical acceptance gates.