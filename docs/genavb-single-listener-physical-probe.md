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

The host probe prints an `ARMED` line containing the epoch before it waits for AVDECC CONNECT. Use that exact epoch in the NXP gPTP evidence and the ESP listener before/after evidence. Evidence with a missing or different epoch must fail closed even when individual counters look healthy.

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

## NXP gPTP exact-API evidence

Aurora includes a one-shot NXP snapshot collector:

```text
adapters/aurora-network-genavb/native/genavb_gptp_snapshot.c
```

It uses only the pinned public GenAVB/TSN control API:

- `GENAVB_CTRL_GPTP` + `GENAVB_MSG_GM_GET_STATUS` -> `GENAVB_MSG_GM_STATUS` for the grandmaster identity;
- `GENAVB_CTRL_CLOCK_DOMAIN` + `GENAVB_MSG_CLOCK_DOMAIN_GET_STATUS` -> `GENAVB_MSG_CLOCK_DOMAIN_STATUS` for media clock state.

A snapshot is `PASS` only when all of these are true:

- the grandmaster identity is non-zero;
- the requested clock domain reports `GENAVB_CLOCK_DOMAIN_STATUS_LOCKED`;
- its source type is `GENAVB_CLOCK_SOURCE_TYPE_INTERNAL`;
- its local source is `GENAVB_CLOCK_SOURCE_PTP_CLK`.

`FREE_WHEELING`, `UNLOCKED`, an audio-clock source, a zero GM identity, malformed responses or control-query failures fail closed. Merely observing a GM identity is not treated as clock lock.

Build the collector on the target against the same GenAVB headers/library used by the deployment, then capture a snapshot before and after the host send interval. The defaults are gPTP domain `0` and `GENAVB_CLOCK_DOMAIN_0`; alternate domains must be passed explicitly and remain identical across both snapshots.

The snapshot schema is:

```text
aurora.genavb.nxp-gptp-snapshot.v1
```

Two snapshots are converted to the correlator input with:

```bash
python validation/physical/aurora_genavb_nxp_gptp_evidence.py build \
  --before nxp-before.json \
  --after nxp-after.json \
  --epoch-id <same-epoch-id> \
  --output nxp-gptp-evidence.json
```

The output schema is:

```text
aurora.genavb.nxp-gptp-evidence.v1
```

The bundler requires both snapshots to be locked to the same non-zero GM, use the same gPTP and clock domains, and have strictly increasing capture times. Its `PASS` is NXP clock evidence only, not a complete physical verdict.

## ESP listener exact-pin evidence

The pinned `esp_avb` and `esp_ptp` sources contain the state needed for a strict listener proof, but not all of it is exposed through their public status APIs. Aurora therefore uses **narrow validation-firmware instrumentation patches against the exact pinned commits**, rather than pretending controller-visible or production APIs exist upstream.

The relevant existing pinned primitives are:

- `avb_listener_stream_s` owns the ACMP listener connection and active `stream_id`;
- `avb_stream_in_last_rx_us(state, 0)` reports the most recent real audio input frame arrival;
- `avb_get_stream_in_counters(...)` already produces the Milan STREAM_INPUT counters internally; its `frames_rx` value comes from the live stream RX context packet counter;
- `esp_ptp` status exposes the active PTP profile, whether a remote source is selected, selected-source timing data and best-time-clock data;
- the `esp_ptp` daemon has a separate internal servo-stability gate that prints `clock is stabilized` only after the existing stability condition is satisfied for consecutive samples.

Two source-level distinctions are important for the physical proof:

1. `clock_source_valid` means a remote source is selected; it is **not** by itself proof that the local clock servo has converged.
2. `clock_source_info.id` is the selected announce sender, while `clock_source_info.btc_id` carries the propagated best-time-clock / grandmaster identity. The latter is what Aurora correlates with NXP GM evidence, including topologies containing a bridge.

At the exact `esp_avb` pin, the ATDECC GET_COUNTERS command/response/unsolicited functions remain explicit `not implemented` stubs. Aurora **does not** claim AECP GET_COUNTERS support and does not use those stubs as evidence.

Aurora's exact-pin validation instrumentation tools are:

```text
validation/physical/aurora_patch_esp_ptp_lock_evidence.py
validation/physical/aurora_patch_esp_avb_listener_evidence.py
```

The `esp_ptp` patch applies only to pinned commit `5b7eec233a93733ae954beefb6df3bb9c12dc901`. It persists the daemon's existing servo-stability decision into validation status, resets the validation stability counter when the profile/source changes, and leaves the existing stability thresholds unchanged.

The `esp_avb` patch applies only to pinned commit `5e75bd3ed91b5407a254a5e49bfc18fc35e6cbb9`. It extends the existing serialized `avb_status()` request path with a read-only validation payload containing:

- gPTP profile state and the separately instrumented servo-stable state;
- propagated BTC/grandmaster identity from `clock_source_info.btc_id`;
- listener-present and ACMP-connected state;
- the ACMP-owned stream ID;
- AAF sample rate, channel count and bit depth;
- the existing internal `frames_rx` counter;
- `last_rx_us`.

These are deliberately physical-validation firmware patches, not new production APIs or upstream compatibility claims. Patched and unpatched status layouts must not be mixed across separately compiled binaries.

The snapshot helper intended to be compiled into the wired ESP32-P4 test firmware is:

```text
validation/physical/esp_avb_aurora_snapshot.c
```

It emits one JSON object with schema:

```text
aurora.esp-avb.listener-snapshot.v1
```

A snapshot reports `gptp_locked:true` only when the endpoint is in the gPTP profile, has a selected remote source, and the instrumented daemon servo-stability gate is true. The reported `grandmaster_id` is the propagated BTC/GM identity, not merely the adjacent announce sender.

The snapshot caller supplies the exact `epoch_id` from the NXP host probe and a host/controller `capture_unix_ms`. Aurora deliberately does not derive that Unix timestamp from the ESP PTP clock because the physical test must not assume an unverified PTP-timescale-to-UTC conversion.

Capture one ESP snapshot before the NXP send interval and one after it. Convert them into the schema consumed by the final correlator with:

```bash
python validation/physical/aurora_esp_avb_listener_evidence.py build \
  --before esp-before.json \
  --after esp-after.json \
  --output esp-listener-evidence.json
```

The output schema is:

```text
aurora.genavb.esp-listener-evidence.v1
```

The bundler fails closed if the epoch, stream identity, 48 kHz/stereo/24-bit contract or grandmaster changes; if ACMP/gPTP lock evidence is not valid in either snapshot; if the receive counter does not increase; if no post-send RX arrival is observed; or if capture times are not strictly ordered. Its `PASS` remains `physical_complete:false` until the NXP host, NXP gPTP and ESP evidence are correlated together.

Do not accept audible output, an LED, a stale lifetime counter, source selection alone or raw Ethernet packet presence by itself as listener receive/clock proof. The evidence must bind the active ACMP stream identity to increasing real STREAM_INPUT receive activity while retaining a stable gPTP servo and one GM during the host probe.

## Pinned ESP32-P4 validation-firmware build gate

Before flashing hardware, Aurora compiles the exact validation firmware composition in CI. The build reference is:

```text
config/esp-avb-p4-build-reference-v1.json
```

The gate pins the upstream `ESP-AVB-Endpoint` application, Aurora's exact `esp_avb`/`esp_ptp` pins, `esp_ptp_rpc`, the `esp32p4` target and an ESP-IDF 6.0.2 build image. It applies both validation patches, compiles `esp_avb_aurora_snapshot.c` into the endpoint application, runs `idf.py set-target esp32p4` plus `idf.py build`, and verifies that the snapshot translation unit produced a build object.

A green firmware build is only source/API/toolchain compatibility evidence. It is **not** a physical AVDECC, AAF, gPTP, RX, synchronization or latency result.

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
   - valid before/after snapshots from the public NXP gPTP and clock-domain control APIs;
   - `LOCKED` PTP clock source in both snapshots;
   - one stable non-zero grandmaster identity across the epoch;
   - before/after timestamps bracket the host send interval when evaluated by the final correlator.

3. **ESP listener evidence**
   - the same `epoch_id`;
   - the real ESP32-P4 ATDECC entity is discovered;
   - ACMP reports the intended listener connection;
   - the listener reports the same stream identity advertised in the host CONNECT;
   - real STREAM_INPUT/AAF receive activity increases between before/after samples bracketing the host send interval;
   - gPTP profile/source plus the instrumented servo-stability gate remain valid in both snapshots;
   - the propagated BTC/GM identity matches the NXP GM evidence.

4. **Identity and time correlation**
   - all evidence objects carry the identical `epoch_id`;
   - Stream ID and connection identity match across host AVDECC evidence and listener evidence;
   - NXP and ESP before/after samples bracket the host `send_started_unix_ms`–`send_ended_unix_ms` interval;
   - NXP and ESP remain on the same grandmaster across the epoch;
   - unrelated prior connections/counters cannot satisfy the gate.

The final correlator is:

```text
validation/physical/aurora_genavb_single_listener_evidence.py
```

Only when all evidence groups pass may it emit `PHYSICAL-PASS: one-listener AVDECC/ACMP + AAF + gPTP`. CI fixture success remains validator/software proof only.

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
