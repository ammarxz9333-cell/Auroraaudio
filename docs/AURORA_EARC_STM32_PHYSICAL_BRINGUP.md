# Aurora eARC → STM32H753 physical bring-up

Status: **physical bring-up contract, not physical acceptance evidence**.

This document defines the shortest measured path for getting live streaming-service Dolby Digital Plus / JOC into the existing Aurora S6 pipeline.

## Reference signal path

```text
Fire TV / TV streaming application
        -> TV HDMI/eARC output
        -> Lindy 38368 eARC extractor
        -> SiI9437 eARC receiver
        -> I2S SD0/BCLK/WS tap
        -> STM32H753 SAI slave RX + DMA
        -> canonical S16_LE IEC61937
        -> Aurora USB ENCODED_IEC61937
        -> Galaxy S6 / aurora-live-ingest
        -> Omniphony streaming IEC61937 parser
        -> Harletty E-AC-3 JOC + OAMD
        -> Omniphony 7.1.4
        -> Aurora USB PCM_S32LE
        -> STM32 realtime output
```

The streaming device/application retains responsibility for account authentication, DRM, HDCP and licensed playback. Aurora taps the already-authorized eARC audio output exactly downstream of the TV.

## Why the SiI9437/Lindy path is the reference

The open Vibesbox implementation has physically measured this exact receiver/tap path. Its published hardware measurements report:

- Lindy 38368 contains a SiI9437 eARC receiver;
- compressed DD+ reaches the tap as IEC61937 on SD0;
- measured DD+ carrier rate is 192 kHz;
- the capture is stereo carrier framing with 2 × 32-bit slots (64-bit frame);
- sample slots are S32 with the useful IEC61937 16-bit word in the high half;
- DD+ uses IEC61937 data type `0x15`;
- a Chromecast/Google TV → TV → eARC → Lindy chain delivered E-AC-3/DD+ intact on hardware.

References:

- https://github.com/sofianchitac/VibesboxSRC/blob/main/config/overlays/README.md
- https://github.com/sofianchitac/VibesboxSRC/blob/main/scripts/earc-bitstream-bridge.sh
- https://github.com/sofianchitac/VibesboxSRC/blob/main/docs/Lattice/earc-i2s-tap-pinout.md

Lattice publicly documents the SiI9437 as an eARC receiver with four-lane I2S output, S/PDIF output, 192 kHz support and Dolby Digital Plus / Dolby Atmos format support:

- https://www.latticesemi.com/Products/ASSPs/HDMI21eARC

## Minimum v1 tap wiring

For the compressed DD+ / JOC route, Aurora needs only SD0 plus the two external clocks. SD1-SD3 are useful later for multichannel LPCM but are not required to prove streaming Atmos v1.

| SiI9437 | Signal | STM32H753 role |
|---|---|---|
| pin 10 | SCK / BCLK | `SAIx_SCK` input |
| pin 11 | WS / LRCK | `SAIx_FS` input |
| pin 12 | SD0 | `SAIx_SD` input |
| pin 7/19 or nearby ground pour | GND | local digital ground return |

The exact STM32 package pins depend on the selected H753 board/custom PCB and must be chosen from a valid SAI alternate-function set.

### Tap rules

The Lindy board routes SD0 and the separate S/PDIF/DSDR2 signal through a downstream 74HC4052 mux. **Tap SD0 at the SiI9437 side, before that mux.** The Vibesbox board trace documents the downstream signal identity as mode-dependent.

Use:

- one dedicated ground return routed next to the signals;
- approximately 330 Ω series resistance on SCK, WS and SD0, preferably near the SiI9437 source;
- short wiring, target ≤15–20 cm;
- SCK paired/twisted with a ground return where practical;
- mechanical strain relief at the 0.4 mm-pitch QFN tap point.

Do not connect the SiI9437 3.3 V power pin to an STM32 GPIO supply. Only signals and common ground are required between independently powered boards.

## Expected DD+ electrical framing

For the measured DD+ path:

```text
carrier sample rate = 192000 frames/s
slots per frame     = 2 (L,R)
slot width          = 32 bits
frame width         = 64 bits
BCLK                = 192000 * 64 = 12.288 MHz
WS/LRCK              = 192 kHz
```

The STM32H753 datasheet specifies SAI slave operation with 32-bit data and a maximum SAI clock of up to `128 × Fs` at 192 kHz. This path uses `64 × Fs`, so the measured 12.288 MHz BCLK is inside the documented timing envelope.

ST references:

- STM32H753 datasheet, SAI characteristics
- RM0433, Serial Audio Interface (SAI), slave mode and DMA

## STM32 SAI configuration contract

The physical HAL implementation must configure one SAI receive sub-block as:

- **slave receiver**;
- asynchronous/external SCK + FS inputs;
- MCLK output disabled/not used;
- I2S-compatible frame format;
- 32-bit data/slot width;
- 2 slots per frame for the compressed SD0 carrier path;
- DMA receive enabled;
- circular or ping-pong DMA with fixed, even slot counts;
- no heap allocation in DMA callbacks.

The SiI9437 is the BCLK/WS master. Aurora must never synthesize a competing SCK/FS on those wires.

## DMA callback contract

The portable source already implements:

```text
aurora_iec61937_capture_forward_s32_high_words(...)
```

The hardware callback supplies the raw S32 DMA slots, first carrier-frame counter and measured/selected carrier rate. The function:

1. discards each S32 low half;
2. preserves the high 16-bit IEC61937 word in canonical S16_LE byte order;
3. converts the carrier counter to Aurora's 48 kHz PTS domain;
4. sends the byte stream through `aurora_transport_send_iec61937()`.

Burst boundaries are intentionally **not** parsed on STM32. A DD+ burst may cross DMA and USB boundaries. Omniphony v0.5.2 owns the persistent IEC61937 parser on S6.

For a DMA block containing `N` stereo carrier frames:

```text
slot_count = 2 * N
first_carrier_frame = running_carrier_frame_counter
running_carrier_frame_counter += N
```

For the DD+ reference path, pass `carrier_rate_hz = 192000`.

## Clock loss and source changes

A source or eARC clock interruption must not allow stale decoded PCM to reappear.

Required behavior:

1. stop/abort the affected SAI DMA stream;
2. mark the next valid encoded block with `AURORA_USB_FLAG_DISCONTINUITY`;
3. re-anchor `first_carrier_frame`/PTS to the new capture epoch;
4. let `aurora-live-ingest` restart Omniphony/Harletty state;
5. keep amplifier output fail-closed until the valid configured PCM path recovers.

The S6 live-ingest regression suite already covers discontinuity and USB-bridge reconnect behavior in software. Physical SAI/eARC clock-loss behavior is still a mandatory hardware test.

## First physical measurements

Before running Netflix/Prime/Disney, verify the link with an oscilloscope/logic analyzer:

1. SCK present and approximately 12.288 MHz during DD+ playback;
2. WS approximately 192 kHz;
3. SD0 active;
4. STM32 DMA progresses without overrun;
5. normalized bytes contain repeated IEC61937 sync words `72 f8 1f 4e`;
6. `Pc & 0x1f == 0x15` is observed for DD+ segments;
7. S6 receives continuous `ENCODED_IEC61937` frames.

Do not promote the path from hardware-blocked based only on clock presence or sync words. Final acceptance still requires real JOC/OAMD/object/height evidence and the long realtime runs defined in `AURORA_LIVE_STREAMING_ATMOS_ACCEPTANCE.md`.

## Product acceptance target

The physical test is successful only when the exact chain proves:

```text
commercial streaming service
-> DD+ JOC over TV eARC
-> SiI9437/Lindy tap
-> STM32 SAI/DMA
-> Aurora USB IEC61937
-> real Harletty JOC + OAMD
-> real Omniphony object render
-> 7.1.4 including height activity
-> STM32 output
```

A local file or synthetic IEC61937 generator remains useful for bring-up but cannot satisfy the live-streaming product acceptance gate.
