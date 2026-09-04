# Aurora live streaming Atmos acceptance

Status: **mandatory product acceptance contract** for the Galaxy S6 + Aurora realtime-MCU appliance path.

The concrete realtime MCU is selected only by `config/aurora-hardware-target.env`; this acceptance contract intentionally names the stable role rather than a part number.

This contract exists to prevent a local-file or synthetic-fixture success from being reported as streaming-service support.

## Product requirement

Aurora v1 must accept immersive audio produced during normal playback by commercial streaming applications and render it in realtime to the configured 7.1.4 speaker layout.

The streaming application/device owns service authentication, DRM and licensed video playback. Aurora does **not** extract or decrypt Netflix/Prime/Disney application media. Aurora receives the already-authorized HDMI/eARC audio output exactly as an AVR/soundbar-class downstream audio device would.

Mandatory v1 immersive transport:

```text
streaming service/application
        -> HDMI/eARC source output
        -> E-AC-3 / Dolby Digital Plus + JOC
        -> IEC61937 carrier
        -> realtime-MCU capture + Aurora USB ENCODED_IEC61937
        -> Galaxy S6 AuroraOS
        -> Omniphony streaming IEC61937 parser
        -> Harletty E-AC-3 JOC decode + OAMD metadata
        -> Omniphony object renderer
        -> 7.1.4 PCM
        -> Aurora USB PCM_S32LE
        -> realtime-MCU realtime output
```

A file copied to the S6 is not a substitute for this test.

## Software ownership boundary

`aurora-live-ingest` must not implement a second Dolby/JOC decoder or a second IEC61937 demultiplexer.

Its job is limited to:

1. connect to `/run/aurora/usb-bridge.sock`;
2. receive complete `ENCODED_IEC61937` Aurora frames from the realtime-MCU side;
3. preserve the IEC61937 byte stream and forward it continuously to the external Omniphony process;
4. receive rendered 7.1.4 raw-f32 PCM from Omniphony;
5. convert/packetize it into **40-frame, 12-channel** `PCM_S32LE` Aurora frames, matching protocol v1;
6. preserve mute/config/discontinuity fail-closed behavior.

Omniphony v0.5.2 owns streaming IEC61937 framing/demultiplexing and passes typed IEC61937 packets to the Harletty bridge. Harletty owns E-AC-3/JOC decode and OAMD extraction. Aurora-owned code remains format-agnostic outside the external-adapter boundary.

## Mandatory live-service proof

Streaming support is **not accepted** until all of the following are captured from physical hardware:

### 1. Real service playback

Use at least two independent commercial streaming services that currently expose an Atmos/DD+ title on the chosen source device. At least one test must use Netflix or Prime Video when available on the device/account/region under test.

The source must be a normal released application on a TV/streaming box. No locally sideloaded elementary stream may satisfy this gate.

### 2. Encoded transport proof

During playback, realtime-MCU capture diagnostics must show continuous `ENCODED_IEC61937` traffic.

For the Atmos segment, the external renderer/bridge telemetry must prove that the stream was recognized as E-AC-3/DD+ with JOC/object metadata. A plain AC-3 or channel-only DD+ stream does not satisfy this gate.

### 3. Object proof

At least one known object-bearing segment must produce live object telemetry after Harletty decode.

Acceptance evidence must include:

- JOC/object mode active;
- OAMD/spatial metadata present;
- nonzero dynamic object count for at least part of the segment;
- no fallback to a flat 5.1 decode for the accepted segment.

### 4. Height/render proof

For known height-bearing content, rendered output must show substantial, time-correlated activity in one or more of:

```text
TFL TFR TBL TBR
```

Merely duplicating front/rear channels into height outputs is a failure. The existing Atmos Object/Height validation method may be used when the source trajectory is known.

### 5. Realtime continuity

For each accepted service, perform at least a 60-minute continuous playback run after warm-up.

Required result:

- zero USB framing corruption;
- zero lost CONFIG state;
- zero sustained PCM underruns/xruns;
- zero decoder/renderer crashes;
- zero uncontrolled amplifier unmute after discontinuity;
- no progressive A/V drift attributable to Aurora;
- S6 thermals remain inside the measured operating budget without sustained realtime deadline misses.

### 6. Service/source transitions

The following transitions must be exercised without rebooting Aurora:

```text
stereo -> DD+ 5.1 -> DD+ JOC Atmos -> stereo
```

and at least one application/title change.

Expected behavior is controlled mute/reconfigure/restart where necessary, then automatic recovery to valid playback. Stale decoded audio must never be emitted after a discontinuity.

### 7. Long soak

After the per-service runs pass, perform an 8-hour mixed-source soak containing at least one Atmos service segment and repeated pause/resume/title/application transitions.

Any repeatable xrun, deadlock, renderer restart loop, USB corruption, thermal collapse or wrong-channel output blocks promotion.

## Non-goals for v1

The v1 acceptance target is streaming Atmos carried as **Dolby Digital Plus / E-AC-3 JOC over IEC61937**.

Dolby MAT and TrueHD may be added later, but their absence must not be hidden by advertising generic support for every Atmos transport.

## Promotion rule

The project may say **live streaming Atmos validated** only after the physical evidence above is archived for the exact S6 variant, exact hardware-target manifest/revision, realtime-MCU firmware, HDMI/eARC front-end, source device, AuroraOS build, Harletty version and Omniphony version under test.

Replacing the selected realtime MCU, its package, directly coupled USB PHY/power path, or relevant pinmux invalidates the hardware-dependent portion of this acceptance until re-tested.

Until then, repository status must distinguish:

- host-compiled/live-ingest implementation;
- local/fixture JOC validation;
- physical live-service validation.
