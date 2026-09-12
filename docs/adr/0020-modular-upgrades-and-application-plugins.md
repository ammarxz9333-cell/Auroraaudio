# ADR 0020: Modular upgrades and out-of-process application plugins

## Status

Accepted for the `main-v2` architecture once this ADR lands.

## Context

Aurora is expected to evolve for years across independent areas: immersive decoders, renderers, DSP, audio I/O, hardware transports, user interfaces, local music playback, metadata services, and provider integrations such as Spotify or YouTube.

If those features share implementation internals or execute arbitrary extension code in the realtime callback, upgrading one area will force broad rebuilds and raise the risk of audio regressions. Provider integrations also change much faster than the realtime audio core and may require network credentials, process isolation, and independent rollback.

## Decision

Aurora uses two distinct extension mechanisms.

### Realtime/component extension

Realtime-capable implementations use narrow Aurora-owned contracts such as `aurora-decoder-api`, `aurora-renderer-api`, `aurora-dsp-api`, and audio/hardware adapter interfaces. Implementations depend on those contracts, not on other implementation crates.

A component upgrade must preserve the public contract or explicitly introduce a versioned contract transition. Silent ABI/API reinterpretation is forbidden.

### Application plugin extension

Application/provider extensions use the versioned `aurora-plugin-api` manifest and protocol and run out of process behind the Aurora Plugin Host.

Application plugins may provide media-source, browse/search, queue, metadata, lyrics, artwork, URL-resolution, streaming-control, local-library, or control-surface capabilities. They do not execute on the realtime callback and do not receive direct hardware, amplifier, MCU, renderer-internal, DSP-internal, or realtime-memory access.

Plugin compatibility is negotiated before activation using an API major/minor range. Unknown manifest schemas, incompatible API ranges, undeclared permissions, or failed handshakes fail closed.

The initial IPC contract is newline-delimited JSON over stdin/stdout. A future protocol may replace it only through a new explicit protocol/version adapter.

## Upgrade semantics

- Plugin package versions are independent from Aurora host versions.
- Compatible plugin updates can be installed/restarted without replacing Aurora Core.
- Plugin installation/update/rollback is atomic and keeps a last-known-good package.
- Repeated plugin failure consumes a restart budget and can quarantine that plugin without stopping unrelated playback.
- Renderer/decoder/DSP/hardware upgrades remain independent of application-plugin package upgrades.
- A future host may support more than one plugin API major through explicit compatibility adapters.

## Source boundary

Provider and local-library plugins do not call the realtime engine directly. They submit source/control intents to a stable Source Manager/control boundary. The Source Manager owns source selection, lifecycle, routing, and handoff into Aurora media/decode services.

Until that boundary exists, provider plugins must not be wired directly to realtime internals as a shortcut.

## Security and safety consequences

Plugin permissions are allowlisted capabilities. The v1 permission model deliberately does not represent unrestricted raw hardware access, realtime callback execution, direct amplifier control, or direct MCU safety control.

Credentials are brokered by the host. Plugins are not the canonical Aurora credential store.

Provider integrations must use supported and lawful provider interfaces. This architecture does not authorize DRM bypass or circumvention of service restrictions.

## Consequences

Positive:
- independent upgrades and rollback;
- crash isolation for fast-moving provider integrations;
- stable realtime core;
- easier testing and capability gating;
- future plugin ecosystem without coupling Aurora to one provider or hardware platform.

Costs:
- IPC and lifecycle management are required;
- application plugins cannot use arbitrary in-process shortcuts;
- stable protocol evolution and permission review become explicit engineering work.

These costs are accepted because they protect the realtime path and long-term replaceability of Aurora.
