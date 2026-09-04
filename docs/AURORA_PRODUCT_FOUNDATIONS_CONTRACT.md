# Aurora Product Foundations Contract

Status: product requirement. This document defines mandatory capabilities for the final Aurora appliance, but it does not override the active execution roadmap or permit networking/hardware work before earlier acceptance gates pass.

## 1. Safe updates, snapshots, and rollback

Aurora must never require a risky in-place update of the only working system state.

Required behavior:
- every OS/core/plugin update is staged and integrity-checked before activation;
- keep a last-known-good system state and configuration snapshot;
- failed boot, failed health check, or repeated service crash after an update triggers automatic rollback;
- user can manually roll back from the S6 UI and authenticated web UI;
- plugin updates are isolated from OS/core updates and can be rolled back independently;
- music library data and user playlists are never destroyed by a system rollback;
- update metadata records version, hash, source, timestamp, compatibility requirements, and result;
- no update may unmute the audio path or bypass STM32 fail-closed safety state.

Acceptance before production-ready:
- successful update preserves settings and library;
- corrupt package is rejected before activation;
- simulated interrupted update recovers to a bootable state;
- intentionally broken new release automatically rolls back to last-known-good;
- plugin rollback works without rebooting the audio core;
- 20 consecutive update/rollback cycles complete without filesystem corruption.

## 2. Plugin Manager

Aurora extensions must run outside the realtime audio callback and outside the trusted hardware-control boundary.

Required behavior:
- versioned plugin API and manifest;
- declared permissions/capabilities;
- lifecycle states: installed, enabled, disabled, failed, quarantined;
- compatibility check against Aurora API and hardware capabilities;
- per-plugin CPU/memory/process health telemetry;
- crash isolation and restart limits;
- install/update/remove/rollback from authenticated web UI and S6 settings UI;
- optional signed/trusted catalog plus explicit local/developer install path;
- plugins cannot directly access STM32 amplifier mute, raw realtime callback memory, or privileged device nodes unless a narrowly scoped reviewed broker permission exists;
- audio-source plugins submit through Aurora Source Manager; control plugins submit through Aurora Control API.

Initial plugin families:
- Bluetooth input/output;
- Home Assistant/MQTT;
- Alexa control;
- Google Home/Nest control;
- official Cast integration when available and compliant;
- Spotify Connect/AirPlay/UPnP-DLNA where licensing and implementation permit;
- Music Hub URL resolvers, metadata, lyrics, artwork, and lawful media fetchers;
- diagnostics/export adapters.

Acceptance before production-ready:
- malformed manifest rejected;
- incompatible API version rejected;
- plugin crash does not interrupt the active realtime audio stream;
- runaway plugin is rate/resource limited and may be quarantined;
- rollback restores previous working plugin version;
- disabling/removing a plugin leaves no stale active source ownership.

## 3. Home Assistant and MQTT integration

Home automation is an optional plugin, never an Aurora Core dependency.

Required MQTT/Home Assistant surface:
- read-only state: power, active source, play state, volume, mute, current track, current scene, layout, temperatures, xrun count, USB state, rear-node health, network health;
- commands: power/standby policy, source selection, play/pause/next/previous where valid, volume, mute, scene selection, approved playlist/queue actions;
- discovery support where practical;
- authenticated broker configuration; TLS supported for remote brokers;
- command authorization and rate limiting;
- no MQTT command may bypass Source Manager, safety mute, or hardware limits.

Acceptance before production-ready:
- reconnect after broker restart;
- duplicate/reordered command handling is deterministic;
- unavailable network does not affect local playback;
- invalid command cannot crash or block Aurora;
- state converges correctly after reconnect.

## 4. Unified diagnostics and self-healing

Diagnostics must describe the entire appliance as one product, not isolated logs.

Required dashboard/state model:
- S6: CPU per cluster/core, frequency, temperature, memory, storage health, Wi-Fi RSSI/rate, service state;
- audio core: source, sample rate, block size, renderer/DSP CPU, xruns, underruns, queue depth, processing deadline margin;
- USB S6↔STM32: attach/reset count, sequence/framing errors, throughput, clock reports, queue depth, xrun recovery state;
- STM32: transport state, sink/source sample counters, TDM/SAI DMA health, mute state, amplifier fault summary;
- each wireless rear node: online state, RSSI, packet loss, late packets, jitter-buffer occupancy, clock drift/ASRC correction, temperature if available, amplifier/DAC faults;
- plugins: health, restart count, CPU/memory, last error;
- Music Hub: import queue, active fingerprint/metadata jobs, failures, storage usage.

Self-healing policy:
- restart a failed noncritical service independently;
- quarantine repeatedly crashing plugins;
- USB transport failure forces mute and handshake recovery rather than uncontrolled playback;
- rear-node loss follows explicit theater policy (mute/fallback/notify) and never silently remaps channels;
- repeated critical failure raises a visible degraded-state alert instead of hiding the fault.

Diagnostics bundle:
- one-button export from S6/web UI;
- contains versions, capability registry, configuration hashes, sanitized logs, temperatures, network/USB/audio counters, and recent failures;
- excludes music files, passwords, auth tokens, and private account credentials.

Acceptance before production-ready:
- fault-injection tests for service crash, plugin crash, USB reset, rear disconnect, Wi-Fi loss, storage pressure, and malformed control input;
- recovery behavior must match documented state machine;
- exported diagnostic bundle must pass privacy-content checks.

## 5. Capability Registry integration

Existing issue #45 remains the implementation source of truth for the machine-readable capability registry.

The registry must eventually expose product-level capabilities in addition to renderer status, including examples such as:
- `layout.7_1_4`;
- `usb.stm32_transport_v1`;
- `rear_wireless.auroralink`;
- `music_hub`;
- `lyrics.synchronized`;
- `bluetooth.input` / `bluetooth.output`;
- `home_assistant` / `mqtt`;
- `alexa_control`;
- `google_home_control`;
- `safe_update.rollback`;
- `plugin_manager`;
- `diagnostics.bundle`.

Each entry must carry an honest state such as placeholder, experimental, host-tested, hardware-validated, accepted, or production-ready plus evidence references. UI surfaces must query this registry so unavailable features are hidden/disabled instead of hard-coded.

## 6. Final-release rule

The final Aurora appliance may be labeled production-ready only when these foundations and all hardware/audio acceptance gates have executable tests, reproducible evidence, and successful validation on the exact supported hardware revision. Documentation or compilation alone is not acceptance evidence.
