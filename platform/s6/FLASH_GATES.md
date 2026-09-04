# AuroraOS-S6 physical flash gates

Initial application-platform target: **Samsung Galaxy S6 SM-G920F / zerofltexx only**.

The concrete realtime MCU is selected only by `config/aurora-hardware-target.env`. These gates refer to the stable realtime-MCU role; replacing the selected part requires re-running every MCU-dependent hardware gate.

A successful compiler/CI run is not permission to flash. The bundle remains marked `NOT_FLASH_READY` until the exact physical device passes the gates below.

## Gate 0 — identity and recovery

- Confirm model is exactly `SM-G920F` and device tree family is `zeroflte`.
- Confirm a working Download Mode path and a known-good recovery path before replacing BOOT or SYSTEM.
- Record current firmware/build identifiers.
- Keep a complete stock firmware package available off-device.
- Do not assume standard Android `fastboot`; the Galaxy S6 bring-up path is Samsung Download Mode tooling.

## Gate 1 — irreversible-data protection

Before any Aurora image replaces a partition, preserve owner data and device-specific partitions.

Required backups:

- BOOT
- SYSTEM
- EFS
- recovery image/path needed to restore the device
- owner-specific Wi-Fi/Bluetooth firmware captured by `capture-stock-firmware.sh`

Aurora build scripts never erase USERDATA automatically.

## Gate 2 — boot image only

First hardware bring-up validates the custom kernel + initramfs before committing to a complete Aurora SYSTEM replacement.

Pass criteria:

- bootloader accepts the Samsung-format image;
- Exynos7420 kernel reaches initramfs;
- UFS block devices appear consistently;
- initramfs identifies the expected SYSTEM partition;
- failure path leaves a usable recovery/download route.

If a safe temporary/test-boot mechanism is not available for this bootloader, use the smallest reversible BOOT-only write procedure supported by the chosen Samsung service tool and retain the stock BOOT image for immediate rollback.

## Gate 3 — Alpine root filesystem

After BOOT is proven:

- SYSTEM mounts as ext4 label `AURORAOS`;
- initramfs performs `switch_root` into Alpine;
- OpenRC reaches the default runlevel;
- USERDATA remains untouched;
- filesystem survives ten cold boots without repair errors.

## Gate 4 — S6 hardware essentials

Must work under AuroraOS, not Android:

- AMOLED framebuffer/DRM path;
- touchscreen via evdev;
- UFS storage;
- 5 GHz Wi-Fi with the owner-captured Broadcom firmware;
- Bluetooth needed by Aurora;
- USB gadget controller in FunctionFS mode;
- charging/power path needed by the final enclosure.

Camera, cellular modem, telephony, and Android services are not Aurora requirements.

## Gate 5 — FunctionFS ↔ realtime MCU

With the realtime MCU and USB HS PHY selected by the hardware-target manifest:

1. enumerate Aurora FunctionFS from at least 50 cold-plug/reset cycles;
2. PING/PONG succeeds before the audio backend starts;
3. CONFIG mismatch leaves all amplifiers muted;
4. run 8 hours bidirectional with sequence/malformed-frame counters at zero;
5. stream 12-channel 48 kHz S32LE periods without USB transport underruns;
6. unplug/replug returns through mute → CONFIG → stream without reboot.

Changing the selected MCU, package, USB PHY, vendor stack, or relevant pinmux invalidates this gate until it is re-run.

## Gate 6 — realtime audio

Run the actual Aurora chain:

`IEC61937/E-AC-3 JOC → Harletty → Omniphony 7.1.4 → Aurora DSP → USB → realtime MCU`

Pass criteria under representative movie material:

- no sustained xruns/dropouts;
- 48 kHz sink clock remains master;
- adaptive drift correction stays bounded;
- channel map is verified electrically/acoustically for all 12 outputs;
- source switching does not emit unsafe transients.

## Gate 7 — thermal soak

Because Exynos7420 was designed for a phone enclosure, sustained audio load must be measured rather than inferred from burst benchmarks.

Pass criteria:

- at least 2 hours of worst-case decode/render/DSP with the intended cooling solution;
- no thermal throttling that causes audio deadline misses;
- no repeated USB resets;
- storage and battery/power temperatures remain within the hardware's normal operating limits.

The selected realtime MCU and its power/PHY path must also remain within their datasheet limits throughout the same sustained run.

## Gate 8 — promote bundle

Only after Gates 0–7 pass on the exact application platform and selected realtime-MCU target may a validated release pipeline omit `NOT_FLASH_READY` and label the artifact `SM-G920F-HW-VALIDATED` together with the exact hardware-target manifest hash.

Validation of one S6 variant does not automatically validate another (`G920I`, `G920T`, Edge variants, etc.). Replacing the realtime MCU or directly coupled PHY/power hardware likewise requires fresh hardware acceptance.
