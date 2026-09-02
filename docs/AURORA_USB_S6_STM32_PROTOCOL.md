# Aurora USB transport: Galaxy S6 ↔ STM32H753

Status: protocol contract for implementation and bench validation. It is not a claim of validated hardware operation.

## Roles

- Galaxy S6 / AuroraOS-S6: USB **device/gadget** using Linux FunctionFS.
- STM32H753 + external ULPI PHY: USB 2.0 High-Speed **host**.
- STM32 owns the physical audio sample clock and realtime I/O deadlines.
- S6 owns decode, object rendering, and DSP.

This role split intentionally avoids depending on Galaxy S6 USB-host + simultaneous-charge behavior.

## USB interface

Use one vendor-specific FunctionFS interface.

| Endpoint | USB direction | Type | Purpose |
|---|---|---|---|
| EP1 OUT | STM32 → S6 | Bulk HS | encoded IEC61937/E-AC-3 JOC input, clock reports, control |
| EP2 IN | S6 → STM32 | Bulk HS | rendered multichannel PCM, acknowledgements, diagnostics |

High-Speed bulk max packet size: 512 bytes. Application transfers SHOULD be submitted as multi-packet buffers rather than one USB packet at a time.

No USB Audio Class dependency is required for the Aurora realtime path.

## Framing

Every application message starts with this fixed 32-byte little-endian header:

```text
Offset  Size  Field
0       4     magic = "AUR0"
4       2     protocol_version = 1
6       2     kind
8       4     flags
12      4     sequence
16      8     pts_48k
24      4     payload_len
28      4     aux
```

`pts_48k` uses the STM32 48 kHz sample-clock domain whenever `PTS_VALID` is set. It is a sample counter, not wall-clock time.

### Kinds

```text
1  ENCODED_IEC61937
2  PCM_S32LE
3  CLOCK_REPORT
4  CONFIG
5  ACK
6  ERROR
7  PING
8  PONG
```

### Flags

```text
bit 0  PTS_VALID
bit 1  DISCONTINUITY
bit 2  END_OF_STREAM
bit 3  XRUN_RECOVERY
```

Unknown flag bits MUST be ignored on receive and preserved only when explicitly forwarded.

## Payload contracts

### ENCODED_IEC61937

The payload is complete IEC61937 data as captured by the STM32-side input path. Aurora must not assume every USB transfer equals one codec frame; framing is defined by this Aurora header.

`aux = 0` for protocol version 1.

### PCM_S32LE

- interleaved signed little-endian 32-bit samples;
- nominal sample rate: 48,000 Hz;
- initial production layout target: 7.1.4 = 12 channels;
- one sample occupies four bytes even when source precision is lower.

`aux` packs:

```text
bits 31..16  channel_count
bits 15..0   frame_count
```

Initial realtime period: **256 frames**. A 12-channel period is therefore 12,288 payload bytes. This is deliberately much larger than one 512-byte USB packet.

Channel order is fixed by the active Aurora speaker-layout manifest; both peers must reject a configuration hash mismatch before enabling amplifiers.

### CLOCK_REPORT

STM32 sends clock reports periodically so the S6 can correct long-term drift without making the phone the hardware audio clock master.

Version-1 payload, little endian:

```text
u64 sink_sample_counter
u64 source_sample_counter
u32 queued_playback_frames
u32 capture_flags
```

The S6 renderer/DSP may use adaptive resampling against `sink_sample_counter`. A clock discontinuity must set `DISCONTINUITY`.

### CONFIG

Control payloads are UTF-8 JSON in version 1. Required startup fields:

```json
{
  "sample_rate": 48000,
  "period_frames": 256,
  "pcm_format": "s32le",
  "channels": 12,
  "layout": "7.1.4",
  "layout_hash": "..."
}
```

The STM32 must not enable speaker outputs until CONFIG is accepted and an ACK is received.

## Sequencing and recovery

- `sequence` increments independently per USB direction and wraps as u32.
- A sequence gap is diagnostic; it does not by itself define codec packet loss.
- On USB reset, both directions restart with sequence 0 and require CONFIG negotiation again.
- On PCM underrun, STM32 outputs silence, sets an xrun counter, and sends a CLOCK_REPORT with `XRUN_RECOVERY`.
- On render overrun, S6 drops no partial PCM frame. It reports ERROR and restarts at a period boundary.
- Amplifier mute is the safe state for protocol-version mismatch, layout mismatch, repeated malformed headers, or loss of CONFIG state.

## Bandwidth budget

7.1.4 PCM at 48 kHz / s32le:

```text
48,000 frames/s × 12 ch × 4 B = 2,304,000 B/s ≈ 18.4 Mbit/s
```

This is far below USB 2.0 High-Speed raw capacity; remaining validation is latency/jitter/CPU behavior, not nominal bandwidth.

## Validation gates

The transport is not called production-ready until all pass on physical hardware:

1. STM32H753 + ULPI enumerates the S6 FunctionFS gadget repeatedly after cold boot and USB reset.
2. 8-hour bidirectional soak with sequence checking and no malformed frames.
3. 12-channel 48 kHz PCM with zero audible underruns under sustained S6 decode/render load.
4. Clock-drift correction remains bounded without periodic buffer growth/shrink.
5. Cable unplug/replug returns to mute → CONFIG → stream without reboot.
6. Thermal test on S6 confirms no sustained throttling causes xruns.
