# Aurora Plugin System — Product Requirement and Runtime Contract

Status: product requirement, implementation gated by the Aurora execution roadmap.

## 1. Goal

Aurora must remain a stable audio appliance while allowing optional integrations to be installed, removed, upgraded, or disabled without changing the realtime audio core.

Initial plugin families include:

- Bluetooth audio input from phones/tablets (including arbitrary app audio such as YouTube playback);
- Bluetooth audio output to optional speakers/headphones;
- Alexa control integration;
- Google Home / Nest control integration;
- official Google Cast receiver integration if/when the required Google device/SDK/certification path is available for the target product;
- Spotify Connect, AirPlay, UPnP/DLNA, internet radio, or other lawful source adapters;
- music-library/catalog adapters;
- optional DSP processors;
- optional UI panels and diagnostics.

Plugins are never required for core Aurora playback.

## 2. Non-negotiable isolation rules

1. Plugins run out-of-process. Do not `dlopen()` third-party plugins into the realtime process.
2. A plugin crash, restart, network timeout, OAuth failure, or malformed response must not stop the active Aurora audio path.
3. Plugins do not access STM32H753, amplifier mute GPIOs, TDM/I2S, or Aurora USB endpoints directly.
4. Only `Aurora Source Manager` may select or switch the active audio source.
5. Only Aurora-owned realtime code may write the final 48 kHz multichannel stream to the hardware transport.
6. Plugin control messages must not execute in the realtime audio callback.
7. Any audio plugin path must use bounded buffers and explicit backpressure; no unbounded queues.
8. Network/cloud plugins must be optional and disabled safely when credentials or network access are unavailable.

## 3. Plugin classes

### `source`
Produces audio for Aurora.

Examples:

- Bluetooth A2DP input;
- Spotify Connect adapter;
- AirPlay adapter;
- UPnP/DLNA renderer input;
- future AuroraLink sender app;
- internet radio.

Source plugins hand decoded PCM or an explicitly supported encoded stream to Aurora. Aurora owns resampling, channel policy, source arbitration, DSP, and output timing.

### `sink`
Consumes Aurora audio as an optional output.

Examples:

- Bluetooth speaker/headphone output;
- diagnostic file sink;
- future network receiver output.

A sink plugin is never allowed to become the master clock for the primary wired theater path unless explicitly promoted by a hardware-validated transport mode.

### `control`
Controls Aurora but does not own audio buffers.

Examples:

- Alexa;
- Google Home / Nest;
- Home Assistant / MQTT;
- remote-control applications.

Typical permissions: play, pause, previous/next, volume, mute, source selection, playlist selection, status query.

### `catalog`
Provides music metadata, search, playlists, artwork, and lyrics.

Navidrome remains the default local catalog backend, but the interface must permit additional providers.

### `dsp`
Optional external DSP process. It must use Aurora-owned bounded shared buffers or a reviewed external-process adapter. Failure must fall back to the configured safe Aurora DSP path.

### `ui`
Adds non-realtime pages/cards/actions to the LVGL UI. UI plugins cannot directly control audio hardware.

## 4. Runtime transport

Control plane:

- versioned local Unix-domain socket API;
- request/response plus events;
- explicit capability negotiation;
- bounded message size;
- monotonic request IDs;
- heartbeat and health state.

Audio plane:

- no JSON audio payloads;
- fixed-format shared-memory/ring-buffer transport or another measured bounded IPC mechanism;
- timestamps expressed in Aurora's 48 kHz sample-time domain after admission to the core;
- explicit format, channel count, period size, and discontinuity flags;
- ASRC/resampling remains Aurora-owned unless a plugin has a separately accepted realtime implementation.

The exact binary IPC layout is implemented only after the capability registry/evaluation gates required by the main roadmap.

## 5. Manifest and permissions

Every plugin ships a machine-readable manifest containing at least:

- plugin ID and human-readable name;
- plugin version;
- Aurora plugin API version range;
- plugin class(es);
- executable path;
- requested permissions;
- supported audio formats/rates/channels when applicable;
- required external services;
- credential requirements;
- license and redistribution metadata;
- capability state: `placeholder`, `experimental`, `accepted`, or `production-ready`.

Permission vocabulary must include at least:

- `audio.source`;
- `audio.sink`;
- `playback.control`;
- `volume.control`;
- `source.select`;
- `catalog.read`;
- `catalog.write`;
- `network.client`;
- `network.listen`;
- `bluetooth.control`;
- `secrets.read:<namespace>`;
- `ui.panel`;
- `diagnostics.read`.

Unknown permissions are rejected by default.

## 6. Process security

- Plugins run as dedicated unprivileged users where practical.
- Secrets are scoped per plugin; one cloud plugin cannot read another plugin's credentials.
- Plugins receive only the filesystem paths and sockets they need.
- Root execution is forbidden by default.
- Old Exynos7420 kernel limitations must be considered before relying on newer sandboxing primitives; isolation must not depend on Landlock or another feature absent from the selected kernel.
- Package integrity is recorded with SHA-256; signed-plugin support is desirable for distributable releases.

## 7. Bluetooth plugin requirements

Aurora must support two distinct roles when the Linux Bluetooth stack has passed hardware validation:

1. `bluetooth-input`: Aurora behaves as an A2DP/LE Audio sink so a phone can play arbitrary system/app audio (for example YouTube) into Aurora.
2. `bluetooth-output`: Aurora behaves as an A2DP/LE Audio source toward optional Bluetooth speakers/headphones.

Bluetooth output is an optional convenience path and is not the preferred synchronized theater-rear transport because Bluetooth latency and independent receiver clocks are not equivalent to AuroraLink theater synchronization.

The Bluetooth plugin runs outside the critical Harletty/Omniphony/DSP/STM32 path. PipeWire/BlueZ may be used by this plugin even if the primary Aurora transport bypasses PipeWire.

## 8. Alexa plugin requirements

Alexa integration is a control-plane plugin. Target controls include:

- play/pause/stop;
- volume/mute;
- optional input/source selection;
- now-playing/status query when supported by the selected Alexa integration.

For a distributable product, Amazon's current Smart Home/Add-on, Works with Alexa, AVS, Music, or Matter requirements must be reviewed at implementation time. Certification-dependent Amazon functionality must not be represented as open or license-free.

Alexa integration failure must have no effect on local playback.

## 9. Google Home / Nest plugin requirements

Google Home integration is a control-plane plugin with target capabilities including:

- volume/mute;
- play/pause/stop/next/previous where the selected Google device model supports them;
- media/playback state reporting;
- power or source controls when applicable.

Google Cloud-to-cloud or another officially supported Google Home integration may be used. Matter may be evaluated for local control if the applicable media/speaker device capabilities and certification requirements fit Aurora at implementation time.

## 10. Google Cast / YouTube requirement

"Play YouTube from another device" must have at least one supported path that does not depend on scraping or downloading YouTube content:

- baseline: Bluetooth A2DP input from the phone/tablet, which carries the device's audio into Aurora;
- optional: an official Google Cast receiver integration if Google permits/certifies the target receiver implementation;
- optional future: Aurora-owned sender applications for platforms where lawful playback-audio capture is available.

Aurora must not ship an unofficial Cast clone and call it Google Cast compatibility. Cast capability remains disabled until the official receiver/device requirements are satisfied.

## 11. Plugin lifecycle

States:

`discovered -> validated -> stopped -> starting -> running -> degraded -> stopped/failed`

Rules:

- manifest/API mismatch: do not start;
- missing permission: do not start;
- repeated crash: quarantine with exponential restart backoff;
- heartbeat loss: remove plugin capabilities and keep Aurora core alive;
- plugin uninstall: revoke credentials and remove its local sockets/state without touching Aurora core configuration outside the plugin namespace.

## 12. Acceptance tests for the plugin runtime

Before the plugin runtime is called accepted:

1. malformed manifests are rejected deterministically;
2. unknown API versions and permissions are rejected;
3. plugin crash during active playback does not produce an xrun in the core path;
4. plugin restart cannot steal the active source without Source Manager arbitration;
5. message and audio buffers remain bounded under a stalled plugin;
6. credential isolation is tested;
7. CPU/RAM usage is reported per plugin;
8. plugin enable/disable/uninstall is deterministic across reboot;
9. an intentionally hostile/faulty test plugin cannot directly reach hardware transport endpoints;
10. CI artifacts record capability state and exact commit.

## 13. Product release requirement

The final Aurora appliance image is not considered complete until the plugin runtime itself is accepted and the following reference plugins have at least deterministic software tests:

- Bluetooth input;
- Bluetooth output;
- local music/Navidrome catalog bridge;
- generic remote-control plugin.

Alexa, Google Home, Google Cast, Spotify Connect, AirPlay, and other ecosystem integrations are optional plugins and are released only when their current external API/licensing/certification requirements are satisfied.
