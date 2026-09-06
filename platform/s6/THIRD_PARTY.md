# AuroraOS-S6 third-party boundary

AuroraOS-S6 keeps format decoders, spatial renderers, and music servers outside Aurora-owned codec/rendering code.

## Harletty Bridge

- Pinned source: `harletty/harletty-bridge` tag `v0.7.4`, source commit `10943821cca7e6886c11f45d2267b06d76e6db7c`.
- Crate license declared by the bridge: Apache-2.0.
- Role: optional runtime bridge loaded by Omniphony.
- Aurora does not copy Harletty codec implementation into this repository.
- Redistribution/use of codec functionality remains subject to separate legal review, including any patent/trademark/licensing obligations applicable in the deployment jurisdiction. A public Aurora release must not claim licensed Dolby compatibility merely because this optional adapter is present.

## Omniphony

- Pinned source: `mgth/Omniphony` tag `v0.5.2` (resolved source commit `f9a79721af64ad9c39042d4deded158b568fc598`).
- `omniphony-renderer` declares GPL-3.0-or-later.
- Role: optional external spatial renderer/host process.
- AuroraOS-S6 applies the retained source patch `platform/s6/patches/omniphony-v0.5.2-low-latency-stdout.patch` before building. The appliance uses Omniphony's 12-channel 7.1.4 raw-f32 stdout path; the patch reduces the generic `FileAudioWriter` buffer from 64 KiB to `40 × 12 × 4 = 1,920` bytes. At 48 kHz / 12 channels / f32, this matches one 40-frame render quantum (0.833 ms of audio) instead of 64 KiB (28.44 ms capacity). This smaller buffer also applies if the same patched build is used with Omniphony's file/FIFO/CAF sink, so those non-appliance uses may perform more write syscalls but are not used by Aurora's live runtime path.
- The builder is fail-closed: the patch is accepted only for the pinned `v0.5.2` source and must pass `git apply --check`; a changed/upgraded upstream tree requires explicit review rather than silently losing the latency fix. S6 host CI additionally clones that exact upstream release, applies the retained patch, compiles `file_sink.rs` tests with Rust 1.78 and executes them.
- If Aurora distributes a binary containing or accompanied by GPL-covered Omniphony artifacts, the release process must satisfy the corresponding source and license obligations, including making the applicable modified source/patch available as required. Aurora-owned libraries remain separated by process/plugin boundaries.

## Navidrome

- Pinned source: `navidrome/navidrome` tag `v0.63.2`.
- License: GPL-3.0.
- Role: music catalog/library/API backend. It does not own the realtime Aurora audio device.
- The official Linux ARM64 release archive is downloaded unmodified and checksum-verified by the builder.

## Samsung/Broadcom device firmware

Wi-Fi/Bluetooth firmware captured from an owner's existing Galaxy S6 installation is not stored in this repository and is not redistributed by Aurora. `capture-stock-firmware.sh` places those owner-supplied blobs only in the local build output used to prepare that owner's image.

## Release rule

The one-file appliance target is a packaging goal, not a license shortcut. A public downloadable image may include only artifacts whose redistribution terms have been reviewed and satisfied. Optional adapters can instead be fetched/built by the owner during image preparation where appropriate.


## Source identity

The native builder checks both external tags against their pinned commit IDs
and records those IDs in `BUILD-MANIFEST.txt`. Its Harletty default is v0.7.4,
matching this document (previously the script still selected v0.7.3).
These source identities do not establish decoder correctness, ABI/load success,
real-time throughput, patent clearance, or physical live-service acceptance.
The external projects use Rust edition 2024; their native build requires an
edition-2024-capable toolchain independently of Aurora's own Rust 1.78 MSRV.

## FFmpeg surround-upmix adapter

The explicit `surround-upmix` mode executes the system FFmpeg binary as a separate
process. FFmpeg decodes the IEC61937 channel bed; a filter graph preserves the
normalized 7.1 bed and derives synthetic height ambience. It does not recover JOC
objects or advertise licensed Dolby Surround/Atmos processing. No FFmpeg source
is copied. FFmpeg is already an Alpine rootfs dependency; its applicable build,
license and redistribution obligations remain part of image release review.

Local software tests used Ubuntu FFmpeg 6.1.1 with real generated E-AC-3/AC-3
streams, not an Atmos sample or protected streaming-service media.
