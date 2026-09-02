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
| EP1 OUT | STM32 → S6 | Bulk HS | encoded IEC61937/E-AC-3 JOC input, clock reports, ACK/error/control |
| EP2 IN | S6 → STM32 | Bulk HS | rendered multichannel PCM, CONFIG and control |

High-Speed bulk max packet size is 512 bytes. This is a USB packet size, **not** an Aurora application-frame boundary.

**Framing rule:** both peers treat USB Bulk as a byte stream. Reads may split one Aurora frame or coalesce several frames. The receiver reconstructs frames from the fixed 32-byte header and `payload_len`. Protocol v1 limits one complete Aurora frame to 256 KiB.

No USB Audio Class dependency is required for the realtime path.

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

`pts_48k` is a sample counter in the STM32 48 kHz clock domain when `PTS_VALID` is set. It is not wall-clock time.

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

## Payload contracts

### ENCODED_IEC61937

Direction: STM32 → S6.

Payload is IEC61937 data captured by the STM32-side input path. Application framing comes from the Aurora header, never from individual 512-byte USB packets.

`aux = 0` in protocol v1.

### PCM_S32LE

Direction: S6 → STM32.

- interleaved signed little-endian 32-bit samples;
- sample rate: 48,000 Hz;
- initial layout: 7.1.4 = 12 channels;
- one sample occupies four bytes.

`aux` packs:

```text
bits 31..16  channel_count
bits 15..0   frame_count
```

Initial realtime period is **256 frames**. One 12-channel period is therefore 12,288 payload bytes and 12,320 bytes including the Aurora header.

### CLOCK_REPORT

Direction: STM32 → S6.

Version-1 payload is exactly 24 bytes:

```text
Offset  Size  Field
0       8     sink_sample_counter
8       8     source_sample_counter
16      4     queued_playback_frames
20      4     capture_flags
```

The S6 can use `sink_sample_counter` for adaptive drift correction. A playback underrun sets `XRUN_RECOVERY` and returns the STM32 to a muted recovery state.

### CONFIG

Direction: S6 → STM32.

CONFIG is deliberately fixed binary rather than JSON so the MCU can validate it deterministically with no dynamic parser or heap allocation.

Version-1 payload is exactly 48 bytes:

```text
Offset  Size  Field
0       4     sample_rate = 48000
4       2     period_frames = 256
6       2     channels = 12
8       2     pcm_format = 1 (S32LE)
10      2     layout_id = 1 (7.1.4)
12      4     reserved = 0
16      32    layout_hash = raw SHA-256 of canonical channel-layout manifest
```

Startup handshake:

1. S6 sends CONFIG while STM32 amplifier outputs are muted.
2. STM32 checks all numeric fields and the expected layout hash.
3. Valid CONFIG → STM32 sends ACK(CONFIG) and enters `ARMED_MUTED`.
4. S6 may then send PCM.
5. STM32 unmutes only after it has accepted and queued the first valid PCM period.
6. Any mismatch stays fail-closed/muted.

### ACK / ERROR

Protocol-v1 STM32 ACK and ERROR payloads are four bytes:

```text
ACK:
u16 acknowledged_kind
u16 status = 0

ERROR:
u16 offending_kind
u16 error_code
```

The Linux bridge may also emit short UTF-8 diagnostic ERROR payloads during early transport bring-up; the production backend must not rely on human-readable text for state transitions.

## Sequencing and recovery

- `sequence` increments independently per USB direction and wraps as u32.
- USB reset/disconnect clears CONFIG state and returns STM32 to muted `WAIT_CONFIG`.
- A malformed stream resets the reassembler and leaves outputs muted.
- On PCM underrun, STM32 outputs silence, mutes, increments its xrun counter and sends an XRUN clock report.
- On a `DISCONTINUITY` PCM period, STM32 mutes before re-queueing and only unmutes after the valid period is accepted.
- Protocol/layout mismatch never enables amplifier output.

## Local S6 handoff

`aurora-ffs-daemon` owns FunctionFS and exposes `/run/aurora/usb-bridge.sock` as Unix `SOCK_SEQPACKET`.

The USB side is byte-stream reassembled first; one local socket message then equals exactly one complete Aurora frame. This isolates Harletty/Omniphony/Aurora DSP from USB reset and packet-fragment details.

PING/PONG is handled directly by the FunctionFS daemon, so basic transport health can be tested before the audio backend starts.

## Bandwidth budget

7.1.4 PCM at 48 kHz / S32LE:

```text
48,000 × 12 × 4 = 2,304,000 B/s ≈ 18.4 Mbit/s
```

Nominal bandwidth is therefore not the limiting issue on USB 2.0 High-Speed. Validation focuses on latency, scheduling, buffering, drift, resets and thermals.

## Validation gates

The transport is not production-ready until physical hardware passes:

1. STM32H753 + ULPI repeatedly enumerates the S6 FunctionFS gadget after cold boot/reset.
2. PING/PONG works before the audio backend starts.
3. CONFIG mismatch always leaves amplifiers muted.
4. 8-hour bidirectional soak shows no framing/sequence corruption.
5. 12-channel 48 kHz playback has no USB-induced underruns under sustained decode/render load.
6. Clock-drift correction remains bounded without periodic buffer growth/shrink.
7. Cable unplug/replug returns through mute → CONFIG → stream without reboot.
8. Thermal load on S6 does not create sustained xruns.
