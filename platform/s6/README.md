# AuroraOS-S6 platform

AuroraOS-S6 turns a Samsung Galaxy S6 board into the primary Aurora appliance controller.

## Fixed architecture

- Target CPU: Exynos 7420, AArch64 userspace only.
- Userland: Alpine Linux ARM64 minimal.
- Kernel: custom Exynos 7420 downstream kernel. postmarketOS/pmaports may be consulted only as a reference for device-specific patches; it is not the Aurora runtime OS.
- UI: LVGL directly on framebuffer/DRM plus evdev touch. No desktop environment, Chromium, Android, or phone shell.
- Music library backend: Navidrome. Navidrome never owns the realtime audio device.
- Immersive external adapters: Harletty and Omniphony remain separate third-party processes/adapters and are never copied into Aurora-owned codec code.
- Realtime I/O: the Aurora realtime MCU selected by `config/aurora-hardware-target.env`, over the USB High-Speed custom transport.
- Live immersive input: HDMI/eARC audio is normalized by the realtime-MCU-side front-end to canonical IEC61937 and transported to the S6 as `ENCODED_IEC61937`.
- Rear speakers: ESP32-C5 5 GHz receiver nodes with timestamped AuroraLink audio and local I2S amplification.

## Runtime ownership

Critical services:

1. `aurora-source-manager` owns source priority, one-active-source arbitration, controlled REVOKE/QUIESCED handoff, persistent mute/gain/lip-sync/standby state, and the stalled-source quiesce watchdog.
2. `aurora-ffs-daemon` owns the S6 FunctionFS endpoint and the S6 <-> realtime-MCU application-frame handoff.
3. `aurora-source-gate` owns the only live HDMI/eARC PCM path from `aurora-live-ingest` to the real FunctionFS bridge. It fails closed before GRANT or when source-manager ownership is lost.
4. `aurora-live-ingest` owns the dedicated live HDMI/eARC immersive decode/render path: it forwards IEC61937 bytes to the external renderer and packetizes rendered 7.1.4 PCM into 40-frame transport periods.
5. `aurora-s6-postprocess` owns the shared post-render ASRC/drift, speaker crossover/bass management, limiter, master gain/mute/standby and lip-sync processing used by the live immersive path.
6. Harletty/Omniphony are external adapters started only when configured and legally permitted. Omniphony owns live IEC61937 demultiplexing; Harletty owns E-AC-3/JOC decode and OAMD extraction.

`aurora-source-ctl` is a host-tested diagnostic/control client for source status, mute, gain, lip-sync and standby. Each control command requires a matching acknowledgement from `aurora-source-manager`; it does not assume success from socket delivery alone. The future LVGL UI should use the same protocol through a persistent control connection rather than launching the CLI for every interaction.

Restartable/non-critical services:

- `aurora-ui` will own the touchscreen, clock/screensaver, settings, source/status, now-playing, album art, and synchronized lyric presentation.
- `navidrome` owns the music catalog/API only.

A UI or music-library crash must not stop the realtime HDMI/eARC audio service.

## Live streaming target

The v1 target is not "plays Atmos files". It is normal commercial streaming playback:

```text
Netflix / Prime Video / Disney+ / other service
        -> TV or streaming box handles authentication + DRM
        -> HDMI/eARC DD+ / E-AC-3 JOC
        -> Aurora realtime-MCU canonical IEC61937 capture
        -> Galaxy S6 AuroraOS
        -> Omniphony IEC61937 parser
        -> Harletty JOC + OAMD
        -> Omniphony 7.1.4 object render
        -> Aurora postprocessor
        -> managed HDMI source gate
        -> Aurora PCM_S32LE
        -> Aurora realtime-MCU speaker output
```

Aurora does not decrypt a streaming application's protected media. It consumes the authorized HDMI/eARC audio output as a downstream audio appliance.

`docs/AURORA_LIVE_STREAMING_ATMOS_ACCEPTANCE.md` is mandatory. A local file, synthetic fixture, or channel-only DD+ decode cannot satisfy the live-streaming claim.

## Source priority

Default priority is:

1. HDMI/eARC immersive input
2. Local music
3. Bluetooth
4. Multi-room/network program

The source-manager control plane and HDMI/eARC data gate are host-CI validated. Local music, Bluetooth and network source IDs already participate in the arbitration contract, but their production audio data adapters are not implemented yet. They must not bypass the source manager or create a second realtime output owner when added.

## CPU policy

The intended policy on Exynos 7420 is:

- Cortex-A57 cluster: realtime decode/render/DSP/USB work.
- Cortex-A53 cluster: Alpine services, UI, Navidrome, Wi-Fi, and background work.

Exact affinity masks and realtime priorities remain configurable until measured on physical S6 hardware.

## Current truth

The source manager, HDMI source gate, live-ingest broker, postprocessor integration and portable realtime-MCU protocol path are host-CI validated. This directory remains an appliance implementation under hardware bring-up, not proof of physical S6 readiness.

The project must not claim a flash-and-play production image or validated live streaming Atmos until the exact S6 variant, kernel, HDMI/eARC front-end, USB transport, selected realtime-MCU firmware/HAL, thermals, display/touch stack, real JOC service playback, output channel mapping and rear-node synchronization have passed their physical validation gates.

The initial supported hardware target for bring-up is `SM-G920F`/`zerofltexx`. Other S6 variants must be explicitly validated before flashing.
