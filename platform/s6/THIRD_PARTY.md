# AuroraOS-S6 third-party boundary

AuroraOS-S6 keeps format decoders, spatial renderers, and music servers outside Aurora-owned codec/rendering code.

## Harletty Bridge

- Pinned source: `harletty/harletty-bridge` tag `v0.7.4`.
- Crate license declared by the bridge: Apache-2.0.
- Role: optional runtime bridge loaded by Omniphony.
- Aurora does not copy Harletty codec implementation into this repository.
- Redistribution/use of codec functionality remains subject to separate legal review, including any patent/trademark/licensing obligations applicable in the deployment jurisdiction. A public Aurora release must not claim licensed Dolby compatibility merely because this optional adapter is present.

## Omniphony

- Pinned source: `mgth/Omniphony` tag `v0.5.2`.
- `omniphony-renderer` declares GPL-3.0-or-later.
- Role: optional external spatial renderer/host process.
- If Aurora distributes a binary containing or accompanied by GPL-covered Omniphony artifacts, the release process must satisfy the corresponding source and license obligations. Aurora-owned libraries remain separated by process/plugin boundaries.

## Navidrome

- Pinned source: `navidrome/navidrome` tag `v0.63.2`.
- License: GPL-3.0.
- Role: music catalog/library/API backend. It does not own the realtime Aurora audio device.
- The official Linux ARM64 release archive is downloaded unmodified and checksum-verified by the builder.

## Samsung/Broadcom device firmware

Wi-Fi/Bluetooth firmware captured from an owner's existing Galaxy S6 installation is not stored in this repository and is not redistributed by Aurora. `capture-stock-firmware.sh` places those owner-supplied blobs only in the local build output used to prepare that owner's image.

## Release rule

The one-file appliance target is a packaging goal, not a license shortcut. A public downloadable image may include only artifacts whose redistribution terms have been reviewed and satisfied. Optional adapters can instead be fetched/built by the owner during image preparation where appropriate.
