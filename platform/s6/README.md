# AuroraOS-S6 platform

AuroraOS-S6 turns a Samsung Galaxy S6 board into the primary Aurora appliance controller.

## Fixed architecture

- Target CPU: Exynos 7420, AArch64 userspace only.
- Userland: Alpine Linux ARM64 minimal.
- Kernel: custom Exynos 7420 downstream kernel. postmarketOS/pmaports may be consulted only as a reference for device-specific patches; it is not the Aurora runtime OS.
- UI: LVGL directly on framebuffer/DRM plus evdev touch. No desktop environment, Chromium, Android, or phone shell.
- Music library backend: Navidrome. Navidrome never owns the realtime audio device.
- Immersive external adapters: Harletty and Omniphony remain separate third-party processes/adapters and are never copied into Aurora-owned codec code.
- Realtime I/O: STM32H753 over USB High-Speed custom transport.
- Rear speakers: ESP32-C5 5 GHz receiver nodes with timestamped AuroraLink audio and local I2S amplification.

## Runtime ownership

Critical services:

1. `aurora-core` owns source arbitration, realtime audio orchestration, DSP, output timing, and device health.
2. `aurora-usb` owns the S6 <-> STM32 transport.
3. Harletty/Omniphony are optional external adapters started only when configured and legally permitted.

Restartable/non-critical services:

- `aurora-ui` owns the touchscreen, clock/screensaver, settings, now-playing, album art, and synchronized lyric presentation.
- `navidrome` owns the music catalog/API only.

A UI or music-library crash must not stop the realtime audio service.

## Source priority

Default priority is:

1. HDMI/eARC immersive input
2. Local music
3. Bluetooth
4. Multi-room/network program

The source manager performs controlled fades and is the only component allowed to switch the active audio source.

## CPU policy

The intended policy on Exynos 7420 is:

- Cortex-A57 cluster: realtime decode/render/DSP/USB work.
- Cortex-A53 cluster: Alpine services, UI, Navidrome, Wi-Fi, and background work.

Exact affinity masks and realtime priorities remain configurable until measured on physical S6 hardware.

## Current truth

This directory is an appliance bootstrap, not proof of hardware readiness. The project must not claim a flash-and-play production image until the exact S6 variant, kernel, USB transport, thermals, display/touch stack, STM32 firmware, and rear-node synchronization have passed physical validation.

The initial supported hardware target for bring-up is `SM-G920F`/`zerofltexx`. Other S6 variants must be explicitly validated before flashing.
