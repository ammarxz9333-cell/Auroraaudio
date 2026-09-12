# Aurora modular upgrade and plugin architecture

Aurora must remain replaceable by subsystem. Adding or upgrading a media service, decoder, renderer, DSP implementation, hardware adapter, or UI must not require redesigning the realtime core.

## Architectural rule

Aurora has two extension classes and they must not be conflated:

1. **Realtime/component adapters** — decoder, renderer, DSP, audio I/O, transport, hardware adapters. These use dedicated Aurora-owned API crates and are selected during runtime-plan construction. They may participate in realtime processing only after their realtime contract is explicitly validated.
2. **Application plugins** — music libraries, provider integrations, Spotify/YouTube control, URL resolvers, metadata, lyrics, artwork, automation, and UI/control integrations. These run **out of process** behind the Aurora Plugin Host and never execute inside the audio callback.

This separation lets Aurora upgrade user-facing integrations frequently without destabilizing audio, while keeping high-performance audio components replaceable behind narrow interfaces.

## Stable dependency direction

```text
                      +----------------------+
                      |   application plugin |
                      | Spotify / YouTube /  |
                      | local music / lyrics |
                      +----------+-----------+
                                 |
                    versioned JSONL protocol
                                 |
                      +----------v-----------+
                      | Aurora Plugin Host   |
                      | permissions/health/  |
                      | restart/rollback     |
                      +----------+-----------+
                                 |
                    typed control/source APIs
                                 |
+---------+    +---------+    +--v------+    +---------+    +---------+
| input   +--->+ decoder +--->+ scene / +--->+ renderer+--->+   DSP   |
| adapter |    |  API    |    | audio   |    |   API   |    |   API   |
+---------+    +---------+    +---------+    +---------+    +----+----+
                                                                   |
                                                         +---------v--------+
                                                         | generic audio I/O |
                                                         +------------------+
```

Application plugins do not import `aurora-realtime-engine`, hardware SDKs, renderer internals, DSP internals, or device memory.

## Versioning policy

Every replaceable boundary has an independent version. The version of the whole Aurora repository is not used as an implicit compatibility contract.

For the application plugin protocol:

- `major` changes mean a breaking host/plugin contract change;
- `minor` changes are backward-compatible additions within a major generation;
- every plugin declares the exact host major and accepted minor range;
- incompatible plugins fail closed before process activation;
- unknown manifest schemas fail closed;
- a plugin package version is independent from the Aurora host API version;
- the host may support multiple API major generations in the future through explicit protocol adapters, never through silent coercion.

For realtime adapters, the same principle applies through their dedicated API crates. New implementations must depend on the API contract, not on internal implementation crates.

## Plugin v1 execution model

Plugin v1 is intentionally out of process. The first transport is newline-delimited JSON over stdin/stdout (`json_lines_stdio_v1`).

The host owns:

- process spawn/stop/restart;
- handshake and API compatibility validation;
- permissions;
- credential brokerage;
- bounded IPC queues and request deadlines;
- health telemetry;
- crash/restart budget and quarantine;
- atomic package update and rollback;
- routing plugin requests through Aurora control/source APIs.

The plugin owns only its provider/application logic.

A crashed or hung plugin must not interrupt current local playback or block the realtime callback.

## Permission model

Plugin manifests request only named capabilities. Plugin v1 deliberately has **no permission** for:

- realtime callback execution;
- arbitrary pointers/shared realtime memory;
- direct amplifier/mute control;
- direct MCU access;
- unrestricted raw hardware access;
- bypassing Source Manager or Aurora safety policy.

Network, local-media, library, credential, storage, playback-control, and notification access are explicit permissions and can be denied independently.

Credentials should be obtained through a host-owned credential broker. A provider integration should not become Aurora's general-purpose secrets store.

## Media-service examples

### Spotify

A future Spotify plugin may expose browse/search/playback-control/streaming-control capabilities using supported provider APIs and authorization. The plugin is independently updateable and can be replaced without changing the renderer, DSP, realtime engine, or hardware adapter.

### YouTube

A future YouTube plugin may expose search/browse/control and lawful URL-resolution capabilities using supported APIs. The plugin boundary does not authorize DRM bypass, extraction of protected media, or provider restriction circumvention.

### Local music player

A local-library plugin may scan approved media paths, expose albums/artists/playlists, and submit playable local media references to Source Manager. Decoding and realtime playback remain Aurora services rather than code running inside the library plugin.

## Upgrade rules

An upgrade is accepted only when the changed component proves its own boundary and does not force unrelated layers to change.

Examples:

- Spotify plugin v2: replace plugin package only; Aurora core unchanged.
- New decoder backend: implement `aurora-decoder-api`; renderer and DSP unchanged.
- New renderer: implement `aurora-renderer-api`; decoder and I/O unchanged.
- New DSP backend: implement `aurora-dsp-api`; source/plugin layer unchanged.
- New SBC/MCU/DAC: implement hardware/audio adapters; core scene/renderer/plugin contracts unchanged.
- Plugin protocol v2: add a protocol adapter/host generation; do not rewrite media plugins and realtime code together.

## Current migration debt

The long-term rule above is stricter than the current implementation in one known area. `aurora-realtime-engine` still constructs/imports `BasicRenderer` and `BasicRendererMode`, and its compatibility constructor still creates the basic `DelayProcessor`. However, the callback-facing delay path now stores and invokes `RealtimeDelayProcessor`, and realtime-engine no longer exposes `BasicDspError` or calls the concrete `DelayProcessor` callback methods directly. DSP callback dispatch is therefore contract-based, while concrete DSP construction is still temporary migration debt.

Issue #118 owns the remaining migration. Completion requires moving concrete renderer/DSP construction into an appropriate runtime materialization/component-assembly layer and making the realtime engine consume only caller-supplied prepared contract implementations. Until #118 is accepted, Aurora must not claim that renderer or DSP backends can be replaced with zero realtime-engine integration work.

This debt is intentionally documented rather than hidden behind the plugin architecture. Application-plugin isolation in this document remains independent of that migration.

## Anti-coupling rules

A change must be rejected or split if it:

- makes Aurora Core import a provider-specific SDK;
- makes the realtime callback depend on plugin IPC;
- makes a Spotify/YouTube/local-library plugin depend on renderer or hardware internals;
- requires a renderer upgrade to change decoder or DSP public contracts without a demonstrated contract defect;
- passes unversioned opaque JSON between core subsystems;
- silently falls back to a different capability when a component is incompatible;
- ties the canonical architecture to one SBC, MCU, DAC, operating system, streaming provider, or external project.

## Planned implementation slices

1. `aurora-plugin-api`: stable manifest, permissions, capabilities, API compatibility, handshake, and health types.
2. Plugin package validator and install registry.
3. Out-of-process Plugin Host with bounded JSONL IPC and handshake timeout.
4. Process health/restart/quarantine policy.
5. Atomic install/update/rollback with package hashes and last-known-good version.
6. Source Manager/control API bridge for media plugins.
7. First reference plugin: local music library/player integration.
8. Provider plugins such as Spotify/YouTube only through supported/legal provider interfaces.

Each slice gets its own tests and capability evidence. None of these steps may weaken realtime isolation.
