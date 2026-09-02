# AuroraOS-S6 component status

Target: **SM-G920F / zerofltexx**. This document distinguishes source completeness, host validation, and physical-hardware validation.

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
| STM32 portable transport core | **HOST-PASS** | Host tests cover fail-closed mute, CONFIG/layout hash, split USB reads, PCM queueing, XRUN recovery and USB reset. No STM32 HAL is claimed. |
| Alpine ARM64 rootfs assembly | **BUILD-SCRIPT** | Builds a minimal Alpine appliance and stages Aurora/external binaries. Requires native AArch64 Alpine builder and owner-supplied S6 firmware blobs. |
| Custom Exynos7420 kernel | **BUILD-SCRIPT** | Pinned kernel build/config logic exists for PREEMPT, DWC3, FunctionFS, display/input and Broadcom Wi-Fi. Not boot-tested here. |
| Samsung DTBH / BOOT image | **BUILD-SCRIPT** | Samsung tooling and universal7420 DTBH constants are pinned; partition-size checks are fail-closed. Not accepted by a physical S6 bootloader yet. |
| AuroraOS SYSTEM ext4 image | **BUILD-SCRIPT** | Image builder enforces the physical SYSTEM partition limit and does not modify USERDATA automatically. |
| Single `.tar.zst` delivery bundle | **BUILD-SCRIPT** | Packages BOOT + SYSTEM + manifests as one delivery archive and deliberately retains `NOT_FLASH_READY`. |
| Harletty ARM64 adapter | **STAGED** | Pinned external source build is part of userspace staging. No claim of Dolby codec implementation by Aurora; runtime S6/JOC soak remains Gate 6/7. |
| Omniphony 7.1.4 adapter | **STAGED** | Pinned external build and 7.1.4 layout are staged. End-to-end S6 realtime validation remains physical work. |
| Navidrome backend binary | **STAGED** | ARM64 release is checksum-pinned and staged. Appliance service/control integration is not complete. |
| Aurora source manager appliance service | **NOT IMPLEMENTED** | Source priority policy is documented, but the S6 appliance supervisor/service wiring is not complete. |
| LVGL touchscreen UI | **NOT IMPLEMENTED** | Required screens: settings, Now Playing, album art, synced lyrics and moving clock/screensaver. No production LVGL app is present yet. |
| STM32H753 USB Host / ULPI HAL | **HW-BLOCKED** | Portable protocol/state machine exists; Cube/HAL USBH integration, enumeration and endpoint scheduling require the hardware phase. |
| STM32 SAI/TDM/DMA realtime output | **HW-BLOCKED** | Queue callback contract exists; actual SAI/TDM DMA and DAC/amp control require hardware bring-up. |
| ESP32-C5 AuroraLink rear nodes | **HW-BLOCKED** | 5 GHz rear-speaker design is selected, but packetized IP audio/network receiver work is later in the authoritative roadmap. |
| TAS5825M rear amplifier control | **HW-BLOCKED** | Hardware integration follows ESP32-C5 receiver validation. |
| Physical SM-G920F boot/display/touch/Wi-Fi | **HW-BLOCKED** | Must pass `FLASH_GATES.md`; host CI is not hardware evidence. |
| Full JOC → 7.1.4 → DSP → STM32 thermal soak | **HW-BLOCKED** | Final acceptance requires the real S6 + STM32 chain and measured xruns/thermals. |

## Promotion rule

No file, release, README, UI or installer may describe the appliance as **flash-ready**, **plug-and-play**, **production-ready**, or **100% validated** while `NOT_FLASH_READY` is present or any required physical gate is incomplete.

The intended final user experience remains one delivery bundle, but the S6 bootloader still requires separate BOOT and SYSTEM partition writes internally. A future validated installer may automate those writes only after device identity, backups, recovery and all physical gates are enforced.
