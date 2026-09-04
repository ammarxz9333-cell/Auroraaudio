# Aurora USB transport: Galaxy S6 ↔ Aurora realtime MCU

Status: protocol contract for implementation and bench validation. It is not a claim of validated hardware operation.

The concrete realtime MCU is selected only by `config/aurora-hardware-target.env`. This protocol uses the stable symbolic role `AURORA_REALTIME_MCU_ROLE`; changing the selected part must not change protocol-v1 unless a protocol requirement itself changes.

## Roles

- Galaxy S6 / AuroraOS-S6: USB **device/gadget** using Linux FunctionFS.
- Aurora realtime MCU + required external HS PHY: USB 2.0 High-Speed **host**.
- Realtime MCU owns the physical audio sample clock and realtime I/O deadlines.
- S6 owns decode, object rendering, and DSP.

This role split intentionally avoids depending on Galaxy S6 USB-host + simultaneous-charge behavior.

## USB interface

Use one vendor-specific FunctionFS interface.

| Endpoint | USB direction | Type | Purpose |
|---|---|---|---|
| EP1 OUT | realtime MCU → S6 | Bulk HS | encoded IEC61937/E-AC-3 JOC input, clock reports, ACK/error/control |
| EP2 IN | S6 → realtime MCU | Bulk HS | rendered multichannel PCM, CONFIG and control |

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

`pts_48k` is a sample counter in the realtime-MCU 48 kHz clock domain when `PTS_VALID` is set. It is not wall-clock time.

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

Direction: realtime MCU → S6.

Payload is a **canonical 16-bit little-endian IEC61937 word stream** captured by the realtime-MCU HDMI/eARC input path.

The front-end must normalize the physical receiver representation before USB transport. If the HDMI/eARC receiver exposes IEC words in a 24- or 32-bit slot, firmware extracts the actual 16-bit IEC word and sends it as S16_LE bytes. Padding/unused slot bits are never forwarded to the S6.

Canonical IEC61937 preamble bytes on the Aurora wire are therefore:

```text
Pa = 0xF872 -> 72 F8
Pb = 0x4E1F -> 1F 4E
Pc             little-endian u16
Pd             little-endian u16
payload ...
```

For streaming Dolby Digital Plus / E-AC-3, the relevant IEC61937 data type is `0x15`. Dolby Atmos carried as DD+ JOC uses the same E-AC-3 IEC61937 type; JOC/object presence is established later by the decoder metadata, not by inventing a separate Aurora transport kind.

Application framing comes from the Aurora header, never from individual 512-byte USB packets. One `ENCODED_IEC61937` Aurora frame does **not** need to equal one IEC61937 burst: the local S6 consumer must preserve byte order across frame boundaries.

`aux = 0` in protocol v1.

### PCM_S32LE

Direction: S6 → realtime MCU.

- interleaved signed little-endian 32-bit samples;
- sample rate: 48,000 Hz;
- initial layout: 7.1.4 = 12 channels;
- one sample occupies four bytes.

Canonical protocol-v1 channel order is:

```text
FL FR C LFE BL BR SL SR TFL TFR TBL TBR
```

`aux` packs:

```text
bits 31..16  channel_count
bits 15..0   frame_count
```

Initial realtime period is **40 frames**, matching the current Omniphony render quantum. One 12-channel period is therefore 1,920 payload bytes and 1,952 bytes including the Aurora header. At 48 kHz this period represents approximately **0.833 ms** of audio. This is a software transport quantum, not a claim of measured physical end-to-end latency.

### CLOCK_REPORT

Direction: realtime MCU → S6.

Version-1 payload is exactly 24 bytes:

```text
Offset  Size  Field
0       8     sink_sample_counter
8       8     source_sample_counter
16      4     queued_playback_frames
20      4     capture_flags
```

The S6 can use `sink_sample_counter` for adaptive drift correction. A playback underrun sets `XRUN_RECOVERY` and returns the realtime MCU to a muted recovery state.

### CONFIG

Direction: S6 → realtime MCU.

CONFIG is deliberately fixed binary rather than JSON so the MCU can validate it deterministically with no dynamic parser or heap allocation.

Version-1 payload is exactly 48 bytes:

```text
Offset  Size  Field
0       4     sample_rate = 48000
4       2     period_frames = 40
6       2     channels = 12
8       2     pcm_format = 1 (S32LE)
10      2     layout_id = 1 (7.1.4)
12      4     reserved = 0
16      32    layout_hash = raw SHA-256 of canonical channel-layout manifest
```

The protocol-v1 canonical layout manifest is the exact UTF-8 byte sequence:

```text
AURORA_LAYOUT_V1;id=1;rate=48000;format=S32LE;period=40;channels=FL,FR,C,LFE,BL,BR,SL,SR,TFL,TFR,TBL,TBR\n
```

Its SHA-256 is fixed to:

```text
05063560d6c5c1b7d3709656cd8c644a6d2b52f5e81383771f26323442d0a244
```

Both S6 and realtime MCU must use these exact 32 raw hash bytes. A different channel order, period, sample format or spelling requires a new layout manifest/hash and must fail closed against protocol-v1 configuration expecting the value above.

Startup handshake:

1. S6 sends CONFIG while realtime-MCU amplifier outputs are muted.
2. Realtime MCU checks all numeric fields and the expected layout hash.
3. Valid CONFIG → realtime MCU sends ACK(CONFIG) and enters `ARMED_MUTED`.
4. S6 may then send PCM.
5. Realtime MCU unmutes only after it has accepted and queued the first valid PCM period.
6. Any mismatch stays fail-closed/muted.

### ACK / ERROR

Protocol-v1 MCU ACK and ERROR payloads are four bytes:

```text
ACK:
u16 acknowledged_kind
u16 status = 0

ERROR:
u16 offending_kind
u16 error_code
```

The Linux bridge may also emit short UTF-8 diagnostic ERROR payloads during early transport bring-up; the production backend must not rely on human-readable text for state transitions.

## Local S6 handoff and live immersive path

`aurora-ffs-daemon` owns FunctionFS and exposes `/run/aurora/usb-bridge.sock` as Unix `SOCK_SEQPACKET`.

The USB side is byte-stream reassembled first; one local socket message then equals exactly one complete Aurora frame. This isolates the decoder/renderer/Aurora DSP layer from USB reset and packet-fragment details.

PING/PONG is handled directly by the FunctionFS daemon, so basic transport health can be tested before the audio backend starts.

For live immersive input, `aurora-live-ingest` connects to this socket and forwards the payload bytes of successive `ENCODED_IEC61937` frames unchanged to Omniphony stdin. It deliberately does not duplicate IEC61937 demultiplexing. Omniphony's streaming parser owns IEC61937 burst reassembly and supplies the resulting typed packet to the configured Harletty bridge. Rendered 7.1.4 raw-f32 output is converted to protocol-v1 40-frame `PCM_S32LE` periods and returned through the same socket.

## Sequencing and recovery

- `sequence` increments independently per USB direction and wraps as u32.
- USB reset/disconnect clears CONFIG state and returns realtime MCU to muted `WAIT_CONFIG`.
- A malformed Aurora USB stream resets the reassembler and leaves outputs muted.
- On PCM underrun, realtime MCU outputs silence, mutes, increments its xrun counter and sends an XRUN clock report.
- On a `DISCONTINUITY` encoded-input frame, the S6 live-ingest service resets/restarts decoder-renderer stream state before accepting new-program audio.
- On a `DISCONTINUITY` PCM period, realtime MCU mutes before re-queueing and only unmutes after the valid period is accepted.
- Protocol/layout mismatch never enables amplifier output.

## Bandwidth budget

7.1.4 PCM at 48 kHz / S32LE:

```text
48,000 × 12 × 4 = 2,304,000 B/s ≈ 18.4 Mbit/s
```

The 40-frame period increases application-frame cadence to 1,200 PCM periods/s but does not change the PCM payload data rate. Nominal bandwidth is therefore not the limiting issue on USB 2.0 High-Speed. Validation focuses on latency, scheduling, buffering, drift, resets, xruns and thermals.

## Hardware-target indirection

The concrete part/package/source location and current validation state are read only from:

```text
config/aurora-hardware-target.env
```

Build/CI must use `AURORA_REALTIME_MCU_SOURCE_DIR` and capability fields. A part-number change does not require editing this protocol. See `docs/AURORA_HARDWARE_TARGET_CONTRACT.md`.

## Validation gates

The transport is not production-ready until the realtime MCU selected by the hardware-target manifest passes physical hardware validation:

1. Selected MCU + declared HS PHY repeatedly enumerates the S6 FunctionFS gadget after cold boot/reset.
2. PING/PONG works before the audio backend starts.
3. CONFIG mismatch always leaves amplifiers muted.
4. Canonical IEC61937 preambles survive the physical HDMI/eARC receiver → realtime-MCU normalization → USB path byte-for-byte.
5. 8-hour bidirectional soak shows no framing/sequence corruption.
6. 12-channel 48 kHz playback has no USB-induced underruns under sustained decode/render load, including the 40-frame / 1,200-periods-per-second configuration.
7. Clock-drift correction remains bounded without periodic buffer growth/shrink.
8. Cable unplug/replug returns through mute → CONFIG → stream without reboot.
9. Thermal load on S6 does not create sustained xruns.
10. Live streaming-service Atmos acceptance additionally satisfies `docs/AURORA_LIVE_STREAMING_ATMOS_ACCEPTANCE.md`.

The manifest's `PINMUX_STATUS`, `HAL_STATUS`, and `PHYSICAL_STATUS` fields must remain honest until these corresponding gates are actually satisfied.
