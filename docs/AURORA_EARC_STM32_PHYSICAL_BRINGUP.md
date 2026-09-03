# Aurora eARC → realtime-MCU physical bring-up

Status: **physical bring-up contract, not physical acceptance evidence**.

The active realtime MCU is selected only by `config/aurora-hardware-target.env`. This document uses the stable symbolic role `AURORA_REALTIME_MCU_ROLE`; a concrete part number in any old directory name is not target truth.

This document defines the shortest measured path for getting live streaming-service Dolby Digital Plus / JOC into the existing Aurora S6 pipeline.

## Reference signal path

```text
Fire TV / TV streaming application
        -> TV HDMI/eARC output
        -> Lindy 38368 eARC extractor
        -> SiI9437 eARC receiver
        -> I2S SD0/BCLK/WS tap
        -> Aurora realtime MCU SAI/I2S slave RX + DMA
        -> Aurora realtime-MCU audio app core
        -> canonical S16_LE IEC61937
        -> Aurora USB ENCODED_IEC61937
        -> Galaxy S6 / aurora-live-ingest
        -> Omniphony streaming IEC61937 parser
        -> Harletty E-AC-3 JOC + OAMD
        -> Omniphony 7.1.4
        -> Aurora USB PCM_S32LE
        -> realtime-MCU audio app core / realtime output
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

| SiI9437 | Signal | realtime-MCU role |
|---|---|---|
| pin 10 | SCK / BCLK | audio serial clock input |
| pin 11 | WS / LRCK | audio frame-sync input |
| pin 12 | SD0 | serial audio data input |
| pin 7/19 or nearby ground pour | GND | local digital ground return |

The exact MCU package pins depend on the target selected in `config/aurora-hardware-target.env` and its eventual board/custom PCB. Pinmux remains unverified until checked against the selected part/package datasheet and actual board routing.

### Tap rules

The Lindy board routes SD0 and the separate S/PDIF/DSDR2 signal through a downstream 74HC4052 mux. **Tap SD0 at the SiI9437 side, before that mux.** The Vibesbox board trace documents the downstream signal identity as mode-dependent.

Use:

- one dedicated ground return routed next to the signals;
- approximately 330 Ω series resistance on SCK, WS and SD0, preferably near the SiI9437 source;
- short wiring, target ≤15–20 cm;
- SCK paired/twisted with a ground return where practical;
- mechanical strain relief at the 0.4 mm-pitch QFN tap point.

Do not connect the SiI9437 3.3 V power pin to an MCU GPIO supply. Only signals and common ground are required between independently powered boards.

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

The selected realtime MCU must be verified against these clock/serial-audio requirements before `AURORA_REALTIME_MCU_PINMUX_STATUS` can leave `unverified`. The manifest declares required capabilities; it does not replace the selected part's datasheet timing check.

## Serial-audio capture configuration contract

The physical HAL implementation must configure one receive sub-block as:

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

## One HAL boundary — no parallel capture/transport path

The portable firmware provides one integration owner:

```c
struct aurora_stm32_audio_app
```

The concrete struct name is a legacy implementation identifier for the current STM32-family portable layer; it is not the target-selection mechanism. Vendor HAL callbacks must terminate at this app core instead of independently calling lower-level capture and transport modules.

The canonical hardware-facing entry points are:

```text
aurora_stm32_audio_app_init(...)
aurora_stm32_audio_app_usb_reset(...)
aurora_stm32_audio_app_usb_receive(...)
aurora_stm32_audio_app_earc_dma_s32_high_words(...)
aurora_stm32_audio_app_send_clock_report(...)
aurora_stm32_audio_app_playback_xrun(...)
```

This gives one owner for USB protocol state, PCM fail-closed state, eARC carrier continuity and XRUN recovery. The lower-level `aurora_transport_*` and `aurora_iec61937_capture_*` APIs remain testable implementation primitives, not a second application path.

## DMA callback contract

The real audio RX DMA half/full callbacks supply the raw S32 DMA slots, the first carrier-frame counter and the current measured/selected carrier rate to:

```text
aurora_stm32_audio_app_earc_dma_s32_high_words(...)
```

Through the stateful capture core this path:

1. discards each S32 low half;
2. preserves the high 16-bit IEC61937 word in canonical S16_LE byte order;
3. converts the carrier counter to Aurora's 48 kHz PTS domain;
4. automatically marks supported physical carrier-rate changes as `AURORA_USB_FLAG_DISCONTINUITY`;
5. sends the byte stream through the one embedded `aurora_transport` instance toward the S6.

Burst boundaries are intentionally **not** parsed on the realtime MCU. A DD+ burst may cross DMA and USB boundaries. Omniphony v0.5.2 remains the single persistent IEC61937 parser on S6. Same-carrier-rate codec/data-type changes must come from a real HAL/receiver event or the S6 parser boundary later; the realtime MCU must not duplicate Dolby/IEC payload parsing just to infer them.

For a DMA block containing `N` stereo carrier frames:

```text
slot_count = 2 * N
first_carrier_frame = running_carrier_frame_counter
running_carrier_frame_counter += N
```

For the DD+ reference path, the measured reference carrier is `192000` Hz.

## USB host and playback callback contract

The realtime-MCU USB Host implementation must feed arbitrary received Aurora protocol bytes into:

```text
aurora_stm32_audio_app_usb_receive(...)
```

USB attach/reset/disconnect must call:

```text
aurora_stm32_audio_app_usb_reset(...)
```

This resets both transport framing/configuration and eARC carrier-continuity state together. Do not reset only one of those modules.

The `aurora_transport_io` callbacks supplied at app initialization remain the hardware boundary for:

- USB send toward the S6;
- queueing exactly one 40-frame × 12-channel S32LE playback period;
- hardware amplifier mute;
- sink sample counter;
- source sample counter;
- queued playback frame count.

The playback DMA implementation must call `aurora_stm32_audio_app_playback_xrun(...)` immediately on underrun. Periodic telemetry must use `aurora_stm32_audio_app_send_clock_report(...)` so the S6 drives one shared 12-channel ASRC ratio from the realtime-MCU-owned physical clock.

## Clock loss and source changes

A source or eARC clock interruption must not allow stale decoded PCM to reappear.

Required behavior:

1. stop/abort the affected audio RX DMA stream;
2. reset/re-anchor the capture epoch as appropriate and mark the next valid encoded block discontinuous;
3. re-anchor `first_carrier_frame`/PTS to the new capture epoch;
4. let `aurora-live-ingest` restart Omniphony/Harletty state;
5. keep amplifier output fail-closed until the valid configured PCM path recovers.

A supported physical carrier-rate transition is already detected by the stateful portable capture core. Same-rate codec/data-type transitions are still an explicit integration item and must not be claimed as detected automatically.

The S6 live-ingest regression suite covers discontinuity and USB-bridge reconnect behavior in software. Physical eARC clock-loss behavior is still a mandatory hardware test.

## Current proof boundary

Software execution proves the portable chain through the integrated realtime-MCU app core, including USB protocol handling, eARC word normalization, carrier-rate transition signaling, clock reports and XRUN fail-closed state. It does **not** prove a compiled target-specific HAL firmware image or physical peripheral callbacks.

The current selected target and its status are read from `config/aurora-hardware-target.env`. Pinmux/HAL/physical status must remain honest there.

## First physical measurements

Before running Netflix/Prime/Disney, verify the link with an oscilloscope/logic analyzer:

1. SCK present and approximately 12.288 MHz during DD+ playback;
2. WS approximately 192 kHz;
3. SD0 active;
4. realtime-MCU DMA progresses without overrun;
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
-> realtime-MCU audio RX DMA
-> integrated realtime-MCU audio app core
-> Aurora USB IEC61937
-> real Harletty JOC + OAMD
-> real Omniphony object render
-> 7.1.4 including height activity
-> Aurora postprocessor
-> Aurora USB PCM
-> integrated realtime-MCU audio app core
-> physical multichannel output
```

A local file or synthetic IEC61937 generator remains useful for bring-up but cannot satisfy the live-streaming product acceptance gate.
