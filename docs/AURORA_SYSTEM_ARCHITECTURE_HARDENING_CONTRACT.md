# Aurora System Architecture Hardening Contract

These requirements are mandatory for the final Aurora appliance. They are product architecture requirements, not permission to skip the active execution roadmap or physical validation gates.

## 1. Unified System Event Bus

Aurora services must communicate cross-domain state through a versioned event bus instead of arbitrary service-to-service coupling.

Required properties:
- typed, versioned events with schema validation;
- monotonic event IDs and timestamps;
- producer identity and capability/version metadata;
- bounded queues and explicit backpressure/drop policy;
- no unbounded allocation on realtime threads;
- realtime audio callbacks never block on the event bus;
- event replay is limited to explicitly persistent state events;
- invalid or unauthorized events are rejected and audited.

Initial event families include:
- `source.*`;
- `playback.*`;
- `audio.health.*`;
- `usb.*`;
- `speaker.*`;
- `rear_node.*`;
- `music_import.*`;
- `plugin.*`;
- `system.thermal.*`;
- `system.update.*`;
- `capability.*`.

## 2. Hardware Abstraction Layer

Aurora Core must not depend directly on Galaxy S6, a concrete realtime-MCU part number, ESP32-C5, or any future board-specific implementation.

The selected realtime MCU is resolved only through `config/aurora-hardware-target.env` under the symbolic role `AURORA_REALTIME_MCU_ROLE`. Concrete vendor/family/part/package names are target data, not Aurora Core API names. See `docs/AURORA_HARDWARE_TARGET_CONTRACT.md`.

Required boundaries:
- platform display/touch;
- platform storage and persistent state;
- network interfaces and Wi-Fi capabilities;
- Bluetooth transport capability;
- USB transport to realtime coprocessor;
- audio sink/source clock and channel transport;
- thermal sensors and power state;
- hardware mute/safety state;
- rear-node discovery and telemetry.

The initial production application platform is `s6-exynos7420`, but both the application platform and realtime-MCU target must be replaceable without changing Aurora Core APIs.

Hardware safety remains fail-closed. HAL failure cannot silently bypass amplifier mute, source arbitration, clock ownership, or channel mapping.

## 3. Thermal and Resource Budget Manager

Realtime audio has highest resource priority. Background work must yield before realtime audio quality degrades.

The manager must observe at least:
- per-service CPU use;
- memory pressure;
- storage I/O pressure;
- thermal zones and throttling state;
- audio deadline margin/xruns;
- USB transport health;
- Wi-Fi/rear-node packet health;
- battery/power state where applicable.

Required degradation order under pressure:
1. pause or throttle library fingerprinting/import jobs;
2. stop optional transcoding and artwork/background tasks;
3. reduce UI animation/redraw rate;
4. throttle nonessential plugins;
5. disable optional analysis/telemetry detail;
6. preserve Harletty/renderer/DSP/USB realtime path as long as thermally safe;
7. if safety or realtime cannot be maintained, mute safely and enter an explicit degraded/fault state.

No thermal policy may override hardware safety limits.

## 4. Safe / Recovery Mode

Aurora must boot into a minimal recoverable state when normal startup repeatedly fails or integrity/health checks fail.

Safe Mode contains only the minimum services required for:
- display/touch or authenticated local web recovery;
- network configuration where safe;
- diagnostics export;
- update rollback;
- configuration rollback/reset;
- plugin disable/quarantine;
- storage health checks;
- restoring a known-good Aurora configuration.

Safe Mode must not automatically start the full realtime audio chain or unmute amplifiers.

Triggers include:
- repeated failed boots;
- failed update health checks;
- corrupt critical configuration/database state;
- repeated core service crash loop;
- explicit user request.

Recovery must support scoped reset of network, plugins, audio configuration, Music Hub state, or full system settings without automatically deleting the user's music library.

## 5. Built-in Test and Benchmark Mode

Aurora must provide a deterministic built-in validation surface available from S6 UI and authenticated web UI.

### Functional channel test
- individually address every configured channel;
- test tone/noise generator;
- channel-map verification workflow;
- rear-node presence and routing verification;
- subwoofer and height-channel tests;
- amplifier mute/unmute safety checks where hardware permits.

### Transport/reliability test
- USB ping/config/clock health;
- packet sequence and malformed-frame counters;
- rear-node loss/jitter/drift telemetry;
- controlled reconnect tests;
- explicit xrun reporting.

### Performance benchmark
- representative decoder/render/DSP workload;
- CPU per critical service;
- memory peak;
- thermal trajectory;
- deadline margin/xruns;
- end-to-end latency estimate where measurable;
- configuration hash and software versions.

### Soak mode
- configurable long-run test;
- automatic report generation;
- no claim of physical validation unless run on physical hardware.

Test Mode must never modify calibration, EQ, library, playlists, or user data without explicit confirmation.

## 6. Cross-cutting requirements

These five foundations integrate with the existing Plugin Manager, Capability Registry, diagnostics, safe updates, Music Hub and Source Manager.

Production readiness requires:
- deterministic automated tests for software-only behavior;
- physical evidence for hardware-dependent claims;
- CI tied to the exact commit;
- explicit capability states (`not_implemented`, `experimental`, `accepted`, `production_ready`);
- no UI or documentation claim may exceed the Capability Registry state.
