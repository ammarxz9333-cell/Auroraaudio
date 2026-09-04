# AuroraOS-S6 component status

Target: **SM-G920F / zerofltexx**. This document distinguishes source completeness, host validation, and physical-hardware validation.

The concrete realtime MCU is selected only by `config/aurora-hardware-target.env`. Component names below use the stable `realtime MCU` role rather than a concrete part number.

## Status meanings

- **HOST-PASS** — deterministic host CI compiles/tests the component.
- **BUILD-SCRIPT** — reproducible build logic exists, but the produced S6 artifact has not been physically validated.
- **STAGED** — third-party/runtime payload is integrated into the appliance staging process, but not validated end-to-end on S6.
- **NOT IMPLEMENTED** — no production implementation exists yet.
- **HW-BLOCKED** — implementation/promotion is intentionally blocked until the repository roadmap permits hardware/network work and physical bring-up is available.

## Current matrix

| Component | Status | Current evidence / limitation |
|---|---|---|
| Aurora USB v1 Rust wire types | **HOST-PASS** | Rust 1.78 tests cover header, PCM shape, clock report and binary CONFIG. |
| Shared C stream parser | **HOST-PASS** | CI compiles with `-Werror` and executes framing/reassembly tests. |
| Galaxy S6 FunctionFS daemon | **HOST-PASS** | Compiles on Linux with `-Wall -Wextra -Werror`; physical DWC3 enumeration is still a flash gate. |
| Aurora source manager control plane | **HOST-PASS** | `aurora-source-manager` owns source priority (`HDMI/eARC > local music > Bluetooth > network`), REVOKE/QUIESCED handoff, a 250 ms stalled-source watchdog, persistent mute/gain/lip-sync/standby state and one-active-source arbitration. CI proves no new source is granted before the old source quiesces, proves a non-responsive source is disconnected before arbitration continues, and requires a deterministic STATUS acknowledgement for accepted control requests. This is control-plane proof only; local/Bluetooth/network audio adapters are not yet implemented. |
| Managed HDMI/eARC source gate | **HOST-PASS** | `aurora-source-gate` is the only live-ingest path to the real FunctionFS bridge. Host integration proves CONFIG/encoded traffic passes before grant, audible PCM is forced to zero before grant, post-grant PCM ramps in with discontinuity marking, REVOKE ramps to silence, and manager loss fails closed. Physical S6/USB timing is still unmeasured. |
| Source control CLI | **HOST-PASS** | `aurora-source-ctl` is staged into `/usr/local/bin` and host-tested for active-source status plus mute, gain, lip-sync and standby control. Each mutating command waits for a matching manager acknowledgement and returns failure when acknowledgement is withheld; no fixed sleep/drain timing is used. It is a diagnostic/UI bridge; the future LVGL UI should keep a persistent control connection rather than shelling out for each action. |
| Live IEC61937 immersive broker | **HOST-PASS** | `aurora-live-ingest` compiles with `-Wall -Wextra -Werror`; it forwards realtime-MCU IEC61937 bytes directly to Omniphony stdin and packetizes rendered 7.1.4 output back to 40-frame `PCM_S32LE` periods (0.833 ms of audio at 48 kHz). Host CI covers normal startup, encoded input before CONFIG ACK, discontinuity restart, USB-bridge reconnect without stale/early PCM, and operation through the managed HDMI source gate. Physical service/JOC realtime and xrun validation remains mandatory. |
| Realtime-MCU portable transport core | **HOST-PASS** | Host tests cover fail-closed mute, CONFIG/layout hash, split USB reads, PCM queueing, XRUN recovery and USB reset. No target-specific vendor HAL is claimed. |
| Realtime-MCU eARC carrier normalization + USB handoff | **HOST-PASS** | Portable code converts left-justified S32 carrier slots to canonical S16_LE IEC61937, converts 48/96/192 kHz carrier counters into the 48 kHz PTS domain, preserves complete L/R carrier frames and forwards them as `ENCODED_IEC61937`. Deterministic CI checks a DD+ type-0x15 preamble and 192 kHz capture PTS. Physical serial-audio/DMA input is not claimed. |
| Alpine ARM64 rootfs assembly | **BUILD-SCRIPT** | Builds a minimal Alpine appliance and stages Aurora/external binaries including FunctionFS, source manager, HDMI source gate, source control CLI and `aurora-live-ingest`. Requires native AArch64 Alpine builder and owner-supplied S6 firmware blobs. |
| Custom Exynos7420 kernel | **BUILD-SCRIPT** | Pinned kernel build/config logic exists for PREEMPT, DWC3, FunctionFS, display/input and Broadcom Wi-Fi. Not boot-tested here. |
| Samsung DTBH / BOOT image | **BUILD-SCRIPT** | Samsung tooling and universal7420 DTBH constants are pinned; partition-size checks are fail-closed. Not accepted by a physical S6 bootloader yet. |
| AuroraOS SYSTEM ext4 image | **BUILD-SCRIPT** | Image builder enforces the physical SYSTEM partition limit and does not modify USERDATA automatically. |
| Single `.tar.zst` delivery bundle | **BUILD-SCRIPT** | Packages BOOT + SYSTEM + manifests as one delivery archive and deliberately retains `NOT_FLASH_READY`. |
| Harletty ARM64 adapter | **STAGED** | Pinned external source build is part of userspace staging. No claim of Dolby codec implementation by Aurora; runtime S6/JOC soak remains a physical acceptance gate. |
| Omniphony 7.1.4 adapter | **STAGED** | Pinned external build and 7.1.4 layout are staged. Its streaming IEC61937 parser is the single demux owner for the live path. The retained Aurora patch aligns its raw-f32 writer buffer with the 40-frame render/transport quantum. End-to-end S6 realtime validation remains physical work. |
| Navidrome backend binary | **STAGED** | ARM64 release is checksum-pinned and staged. It provides the music library/backend only; no local-music audio source adapter to the source manager exists yet. |
| Local music source audio adapter | **NOT IMPLEMENTED** | Source ID, priority and arbitration behavior exist, but there is no production PCM path that registers local playback, obtains GRANT and feeds the final realtime output path. |
| Bluetooth source audio adapter | **NOT IMPLEMENTED** | BlueZ/PipeWire packages are staged, but no production source-manager adapter or PCM handoff exists yet. |
| Network / Multi-Room source audio adapter | **NOT IMPLEMENTED** | Source-manager role exists only. AuroraLink/network receive, clock discipline and final PCM handoff are later work. |
| LVGL touchscreen UI | **NOT IMPLEMENTED** | Required screens: settings, source/status, Now Playing, album art, synced lyrics and moving clock/screensaver. No production LVGL app is present yet. |
| Realtime-MCU USB Host / HS-PHY HAL | **HW-BLOCKED** | Portable protocol/state machine exists; target-vendor USB Host integration, enumeration and endpoint scheduling require the hardware phase. |
| Realtime-MCU HDMI/eARC IEC61937 capture HAL | **HW-BLOCKED** | Portable normalization/PTS/USB handoff is host-tested. Remaining work is physical SiI9437/Lindy wiring, target serial-audio slave-RX/DMA configuration, clock-loss handling and measured carrier validation described in `docs/AURORA_EARC_REALTIME_MCU_PHYSICAL_BRINGUP.md`. |
| Realtime-MCU TDM/DMA realtime output | **HW-BLOCKED** | Queue callback contract exists with 40-frame periods; actual TDM DMA and DAC/amp control require hardware bring-up and sustained xrun testing at the higher period rate. |
| ESP32-C5 AuroraLink rear nodes | **HW-BLOCKED** | 5 GHz rear-speaker design is selected, but packetized IP audio/network receiver work is later in the authoritative roadmap. |
| TAS5825M rear amplifier control | **HW-BLOCKED** | Hardware integration follows ESP32-C5 receiver validation. |
| Physical SM-G920F boot/display/touch/Wi-Fi | **HW-BLOCKED** | Must pass `FLASH_GATES.md`; host CI is not hardware evidence. |
| Live streaming service DD+ JOC → 7.1.4 | **HW-BLOCKED** | Mandatory acceptance is defined in `docs/AURORA_LIVE_STREAMING_ATMOS_ACCEPTANCE.md`; requires a real service/source, HDMI/eARC front-end, S6 + selected realtime MCU and measured JOC/OAMD/object/height/realtime evidence. |
| Full JOC → 7.1.4 → DSP → realtime-MCU thermal soak | **HW-BLOCKED** | Final acceptance requires the real S6 + selected realtime-MCU chain and measured xruns/thermals. |

## Promotion rule

No file, release, README, UI or installer may describe the appliance as **flash-ready**, **plug-and-play**, **production-ready**, **live streaming Atmos validated**, or **100% validated** while `NOT_FLASH_READY` is present or any required physical gate is incomplete.

The intended final user experience remains one delivery bundle, but the S6 bootloader still requires separate BOOT and SYSTEM partition writes internally. A future validated installer may automate those writes only after device identity, backups, recovery and all physical gates are enforced.
