# Aurora eARC → realtime-MCU physical bring-up

Status: **physical bring-up contract, not physical acceptance evidence**.

The active realtime MCU is selected only by `config/aurora-hardware-target.env`. This document uses the stable symbolic role `AURORA_REALTIME_MCU_ROLE`; concrete part/package identity is target data, not architecture naming.

This document defines the shortest measured path for getting live streaming-service Dolby Digital Plus / JOC into the existing Aurora S6 pipeline.

## Reference signal path

```text
Fire TV / TV streaming application
        -> TV HDMI/eARC output
        -> Lindy 38368 eARC extractor
        -> SiI9437 eARC receiver
        -> I2S SD0/BCLK/WS tap
        -> Aurora realtime MCU serial-audio slave RX + DMA
        -> Aurora realtime-MCU app core
        -> canonical S16_LE IEC61937
        -> Aurora USB ENCODED_IEC61937
        -> Galaxy S6 / aurora-live-ingest
        -> Omniphony streaming IEC61937 parser
        -> Harletty E-AC-3 JOC + OAMD
        -> Omniphony 7.1.4
        -> Aurora USB PCM_S32LE
        -> realtime-MCU app core / realtime output
```

The streaming device/application retains responsibility for account authentication, DRM, HDCP and licensed playback. Aurora taps the already-authorized eARC audio output downstream of the TV.

## Why the SiI9437/Lindy path is the reference

The open Vibesbox implementation has physically measured this receiver/tap path. Its published hardware measurements report:

- Lindy 38368 contains a SiI9437 eARC receiver;
- compressed DD+ reaches the tap as IEC61937 on SD0;
- measured DD+ carrier rate is 192 kHz;
- capture is stereo carrier framing with 2 × 32-bit slots (64-bit frame);
- sample slots are S32 with the useful IEC61937 16-bit word in the high half;
- DD+ uses IEC61937 data type `0x15`;
- a Chromecast/Google TV → TV → eARC → Lindy chain delivered E-AC-3/DD+ intact on hardware.

References:

- https://github.com/sofianchitac/VibesboxSRC/blob/main/config/overlays/README.md
- https://github.com/sofianchitac/VibesboxSRC/blob/main/scripts/earc-bitstream-bridge.sh
- https://github.com/sofianchitac/VibesboxSRC/blob/main/docs/Lattice/earc-i2s-tap-pinout.md
- https://www.latticesemi.com/Products/ASSPs/HDMI21eARC

## Minimum v1 tap wiring

For compressed DD+ / JOC, Aurora needs SD0 plus the two external clocks. SD1-SD3 are optional future multichannel-LPCM paths.

| SiI9437 | Signal | realtime-MCU role |
|---|---|---|
| pin 10 | SCK / BCLK | audio serial clock input |
| pin 11 | WS / LRCK | audio frame-sync input |
| pin 12 | SD0 | serial audio data input |
| pin 7/19 or nearby ground pour | GND | local digital ground return |

The active package/pinmux is read from `config/aurora-hardware-target.env`; the current pin assignment is datasheet-verified there but remains physically unmeasured until board bring-up.

### Tap rules

The Lindy board routes SD0 and the separate S/PDIF/DSDR2 signal through a downstream 74HC4052 mux. **Tap SD0 at the SiI9437 side, before that mux.**

Use:

- one dedicated ground return routed next to the signals;
- approximately 330 Ω series resistance on SCK, WS and SD0, preferably near the SiI9437 source;
- short wiring, target ≤15–20 cm;
- SCK paired/twisted with a ground return where practical;
- mechanical strain relief at the 0.4 mm-pitch QFN tap point.

Do not connect the SiI9437 3.3 V power pin to an MCU GPIO supply. Only signals and common ground are required between independently powered boards.

## Expected DD+ electrical framing

```text
carrier sample rate = 192000 frames/s
slots per frame     = 2 (L,R)
slot width          = 32 bits
frame width         = 64 bits
BCLK                = 192000 * 64 = 12.288 MHz
WS/LRCK             = 192 kHz
```

The active target manifest carries the datasheet proof identifier and required serial-audio capabilities. Physical timing and signal integrity still require measurement.

## Serial-audio capture configuration contract

The target-specific HAL must configure one receive sub-block as:

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

Portable firmware has one integration owner:

```c
struct aurora_realtime_mcu_app
```

Target-specific vendor callbacks terminate at this app core instead of calling lower-level capture and transport modules independently.

Canonical application entry points:

```text
aurora_realtime_mcu_app_init(...)
aurora_realtime_mcu_app_usb_reset(...)
aurora_realtime_mcu_app_usb_receive(...)
aurora_realtime_mcu_app_earc_dma_s32_high_words(...)
aurora_realtime_mcu_app_send_clock_report(...)
aurora_realtime_mcu_app_playback_xrun(...)
```

The target-independent `aurora_realtime_mcu_hal_*` layer adds USB-session, eARC-lock, VBUS-fault and same-rate relock recovery semantics above this app core. `aurora_transport_*` and `aurora_iec61937_capture_*` remain testable implementation primitives, not second application paths.

## DMA callback contract

Audio RX DMA half/full callbacks supply raw S32 DMA slots to:

```text
aurora_realtime_mcu_hal_earc_dma_s32_high_words(...)
```

The HAL owns the carrier-frame counter and current locked carrier rate, then calls the one `aurora_realtime_mcu_app` path. That path:

1. discards each S32 low half;
2. preserves the high 16-bit IEC61937 word in canonical S16_LE byte order;
3. converts the carrier counter to Aurora's 48 kHz PTS domain;
4. marks carrier-rate changes and lock-loss recovery as `AURORA_USB_FLAG_DISCONTINUITY`;
5. sends the byte stream through the one embedded `aurora_transport` instance toward the S6.

Burst boundaries are intentionally **not** parsed on the realtime MCU. Omniphony remains the single persistent IEC61937 parser on S6. Same-carrier-rate codec/data-type changes must come from a receiver/HAL event or the S6 parser boundary later; the realtime MCU must not duplicate Dolby/IEC payload parsing to infer them.

For a DMA block containing `N` stereo carrier frames:

```text
slot_count = 2 * N
running_carrier_frame_counter += N
```

The measured DD+ reference carrier is 192 kHz.

## USB host and playback callback contract

USB Host attach/reset/disconnect maps to:

```text
aurora_realtime_mcu_hal_usb_session_begin(...)
aurora_realtime_mcu_hal_usb_session_end(...)
```

Arbitrary received Aurora protocol bytes feed:

```text
aurora_realtime_mcu_hal_usb_receive(...)
```

The HAL/application boundary owns:

- USB send toward S6;
- exactly one canonical 40-frame × 12-channel S32LE playback-period queue;
- hardware amplifier mute;
- sink sample counter;
- source sample counter;
- queued playback-frame count.

Playback underrun must call:

```text
aurora_realtime_mcu_hal_playback_xrun(...)
```

Periodic clock telemetry must call:

```text
aurora_realtime_mcu_hal_clock_tick(...)
```

so the S6 controls one shared 12-channel ASRC ratio from the realtime-MCU-owned physical clock.

## Clock loss, source changes and VBUS faults

A source/eARC clock interruption must not allow stale decoded PCM to reappear.

Required behavior:

1. stop/abort the affected RX DMA stream;
2. call `aurora_realtime_mcu_hal_earc_unlock(...)`;
3. on relock, call `aurora_realtime_mcu_hal_earc_lock(...)` with the measured carrier rate;
4. the first valid encoded block is marked discontinuous even if the rate relocks to the same value;
5. `aurora-live-ingest` restarts decoder/renderer stream state;
6. output remains fail-closed until the configured valid PCM path recovers.

A protected VBUS fault calls `aurora_realtime_mcu_hal_vbus_fault(...)`, which kills the USB session and resets protocol/capture state. Fault clearance does not silently resume a previous session; a fresh session begin is required.

Same-rate codec/data-type transitions remain an explicit integration item and are not claimed as detected automatically.

## Current proof boundary

Host software execution proves the portable transport/capture/app/HAL recovery chain. It does **not** prove a compiled target-vendor firmware image or real peripheral callbacks.

Current target identity, pinmux proof, vendor-stack pin, HAL status and physical status are read only from `config/aurora-hardware-target.env`.

## First physical measurements

Before Netflix/Prime/Disney acceptance, verify with an oscilloscope/logic analyzer:

1. SCK approximately 12.288 MHz during DD+ playback;
2. WS approximately 192 kHz;
3. SD0 active;
4. RX DMA progresses without overrun;
5. normalized bytes contain repeated IEC61937 sync words `72 f8 1f 4e`;
6. `Pc & 0x1f == 0x15` during DD+ segments;
7. S6 receives continuous `ENCODED_IEC61937` frames;
8. USB disconnect/reconnect performs mute → CONFIG → ACK → stream;
9. protected VBUS fault leaves output muted and requires a new session;
10. TDM output channel order matches the canonical 7.1.4 manifest.

Final acceptance still requires real JOC/OAMD/object/height evidence plus the long realtime runs defined in `AURORA_LIVE_STREAMING_ATMOS_ACCEPTANCE.md`.

## Product acceptance target

```text
commercial streaming service
-> DD+ JOC over TV eARC
-> SiI9437/Lindy tap
-> realtime-MCU audio RX DMA
-> one realtime-MCU HAL/app core
-> Aurora USB IEC61937
-> real Harletty JOC + OAMD
-> real Omniphony object render
-> 7.1.4 including height activity
-> Aurora postprocessor
-> Aurora USB PCM
-> one realtime-MCU HAL/app core
-> physical multichannel output
```

A local file or synthetic IEC61937 generator remains useful for bring-up but cannot satisfy the live-streaming product acceptance gate.
