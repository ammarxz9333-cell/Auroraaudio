# Aurora Final Product Acceptance Contract

Aurora is delivered as a final consumer-style appliance only when every mandatory gate below passes on the exact hardware release target.

## A. Build and reproducibility

- one documented build command produces the appliance bundle;
- all source revisions and third-party versions are pinned;
- SHA-256 manifests exist for produced images and packaged third-party artifacts;
- CI is green for the exact release commit;
- rebuild from a clean environment is reproducible within documented deterministic limits.

## B. Galaxy S6 / AuroraOS-S6

- target identity is validated before flash;
- custom Exynos7420 kernel boots reliably;
- Alpine ARM64 reaches OpenRC reliably;
- display, touchscreen, UFS, 5 GHz Wi-Fi, Bluetooth, charging/power, and USB FunctionFS pass physical validation;
- ten cold boots complete without filesystem repair;
- recovery/rollback procedure is proven before release.

## C. Core audio

- 48 kHz master timing is stable;
- all configured channels are electrically/acoustically mapped correctly;
- source changes are click/pop safe;
- no unbounded allocation or blocking in realtime callbacks;
- realtime metrics expose xruns, buffer health, clock state, CPU, and thermal state.

## D. Immersive chain

For any optional proprietary/third-party decoder/renderer path enabled in a personal build:

- exact version and licensing boundary are documented;
- decoder/renderer crash is isolated from Aurora core;
- representative long-duration content is tested;
- 7.1.4 output map is verified;
- CPU and thermal soak pass on the S6 target.

Aurora-owned open-format and renderer capabilities follow the main execution-roadmap acceptance rules.

## E. STM32H753 realtime I/O

- USB HS/ULPI enumeration passes repeated cold-plug/reset tests;
- CONFIG mismatch keeps amplifiers muted;
- 12-channel S32LE 48 kHz transport runs for at least 8 hours without transport xruns;
- hardware mute, SAI/TDM DMA, clock counters, reset recovery, and unplug/replug are physically validated;
- power-on and failure states are fail-muted.

## F. Rear wireless system

Before ESP32-C5 rear nodes are promoted from experimental:

- 5 GHz transport passes impaired-network simulation first;
- two physical nodes with independent clocks pass synchronization tests;
- packet loss, reorder, jitter, disconnect, reconnect, and clock drift are measured;
- theater and multiroom latency profiles remain separate;
- inter-speaker skew and buffer targets are measured, not inferred;
- amplifier mute and reconnect behavior are transient-safe.

## G. UI and local user experience

Mandatory S6 touchscreen functions:

- Home/status screen;
- Now Playing;
- play/pause/next/previous and volume;
- music library browsing and playlists;
- album artwork;
- synchronized lyrics when metadata exists;
- speaker/audio settings;
- Wi-Fi/system status;
- diagnostics/temperature/transport health;
- configurable AMOLED-safe clock/screensaver with positional movement and auto-off;
- UI failure/restart does not interrupt active audio.

## H. Music backend

- Navidrome/local catalog starts independently from realtime audio;
- database scan cannot cause realtime xruns;
- playback is owned by Aurora Music Player/Source Manager, not by Navidrome's process;
- remote library access and local UI remain optional to one another;
- missing artwork/lyrics/network fail gracefully.

## I. Source Manager

- exactly one active source owns the primary playback path;
- HDMI/immersive, local music, Bluetooth, and future network sources obey explicit priority and user override rules;
- switching uses bounded fade/mute policy;
- a crashed source plugin cannot seize playback on restart;
- source state survives/reinitializes deterministically after reboot.

## J. Plugin system

The plugin runtime described in `docs/product/AURORA_PLUGIN_SYSTEM.md` must pass its acceptance tests.

Mandatory reference plugins for final appliance acceptance:

- Bluetooth audio input;
- Bluetooth audio output;
- local catalog bridge;
- generic remote-control adapter.

Optional ecosystem plugins are not blockers for the base product unless advertised as included in that release.

## K. Ecosystem integrations

### Bluetooth

- phone -> Aurora A2DP input is physically tested with Android and iOS;
- arbitrary application audio is accepted as a normal Bluetooth stream;
- Aurora -> Bluetooth output is tested separately;
- Bluetooth paths are clearly labeled as convenience paths, not synchronized theater rear transport.

### Alexa

If shipped:

- volume/mute/playback controls are tested against the current official Alexa interfaces;
- all current certification/cloud/account-linking requirements are documented;
- Alexa outage has zero effect on local Aurora operation.

### Google Home / Nest

If shipped:

- current official speaker/media traits are tested;
- cloud/local integration requirements are documented;
- Google service outage has zero effect on local operation.

### Google Cast

If shipped:

- the implementation uses an official supported Cast receiver/device route;
- registration/certification requirements are satisfied;
- Aurora does not claim Cast compatibility based on an unofficial protocol clone.

## L. Performance and soak

Mandatory release soak suite:

- >= 2 h worst-case S6 decode/render/DSP/UI/network load;
- >= 8 h USB transport soak;
- >= 8 h local music playback;
- repeated UI restart during audio;
- repeated Navidrome restart/scan during audio;
- Wi-Fi disconnect/reconnect;
- Bluetooth connect/disconnect when plugin exists;
- thermal monitoring throughout;
- zero unexplained xruns/dropouts for a production-ready release.

## M. Release artifact

A consumer-ready release must provide:

- one delivery archive;
- boot/system images appropriate to the S6 partition layout;
- STM32 firmware image;
- rear-node firmware images when rear wireless is included;
- version/commit/checksum manifest;
- plugin manifests;
- backup/recovery instructions;
- hardware wiring/board revision manifest;
- a release status file stating exactly which capabilities are `experimental`, `accepted`, or `production-ready`.

The `NOT_FLASH_READY` marker is removed only after all mandatory physical gates for that exact release have evidence.
