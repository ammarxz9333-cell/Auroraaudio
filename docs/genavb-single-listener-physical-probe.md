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
  [duration-ms=5000] \
  [target-latency-frames=480] \
  [connect-timeout-ms=30000]
```

The native shim must be built for the target NXP Linux system against the same pinned GenAVB/TSN API used by Aurora's exact-pin CI. The NXP GenAVB service and AVDECC stack must already be running.

The probe:

1. opens `GENAVB_CTRL_AVDECC_MEDIA_STACK` through Aurora's shared native shim;
2. polls the real GenAVB control file descriptor;
3. waits for a supported AVDECC CONNECT for the selected Stream Output descriptor index;
4. records the AVDECC-owned stream ID, destination MAC, port and stream class;
5. prepares the talker from the exact cached CONNECT parameters;
6. starts a 48 kHz / stereo / 24-bit AAF stream with 48-frame blocks;
7. sends a deterministic 997 Hz antiphase stereo signal at 0.10 full-scale amplitude for the requested duration;
8. continuously checks the control channel and fails if the selected stream disconnects while sending;
9. stops the talker and emits machine-readable host evidence.

A successful host run ends with a JSON record whose schema is:

```text
aurora.genavb.single-listener-host-evidence.v1
```

and a stable summary line:

```text
aurora-genavb-single-listener-host: PASS connect=avdecc prepare=from-acmp send=aaf24-48k-stereo listener-rx=required gptp-lock=required
```

`HOST_PASS` is intentionally **not** a complete physical verdict.

## Required evidence for a complete one-listener physical gate

A complete one-listener evidence bundle must contain all of the following from the same test epoch:

1. **NXP host / AVDECC evidence**
   - real CONNECT received for the selected Stream Output index;
   - stream ID, destination MAC, port and class recorded from the CONNECT;
   - talker prepared from those parameters;
   - continuous successful AAF submits for the declared duration;
   - no target DISCONNECT during the send interval.

2. **NXP gPTP evidence**
   - the relevant NXP network port is link-up and gPTP-capable;
   - the GenAVB clock used by the stream is synchronized for the test epoch;
   - the raw NXP gPTP status/log excerpt is retained in the evidence bundle.

3. **ESP listener evidence**
   - the real ESP32-P4 ATDECC entity is discovered;
   - ACMP reports the intended listener connection;
   - the listener receives the same stream identity advertised in the host CONNECT;
   - AAF receive counters/audio activity increase during the host test signal;
   - listener-side gPTP/clock state is retained for the same epoch.

4. **Identity correlation**
   - Stream ID and connection identity must match across host AVDECC evidence and listener evidence;
   - timestamps/logs must identify one test epoch so unrelated prior connections cannot satisfy the gate.

Only when all four evidence groups pass may this gate be labelled `PHYSICAL-PASS: one-listener AVDECC/ACMP + AAF + gPTP`.

## Negative physical checks

The same hardware setup must later prove:

- listener unplug/disconnect is observed and host sending fails closed;
- reconnect does not reuse stale prepared state;
- a fresh CONNECT and prepare/start epoch is required before audio resumes;
- an unexpected stream identity cannot satisfy the evidence correlator;
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