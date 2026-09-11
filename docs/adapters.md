# Third-party and platform adapters

Aurora keeps third-party projects and platform integrations behind explicit adapter boundaries.

## Adapter classes

- **Decoder adapters** translate encoded/structured inputs into Aurora channel/object data.
- **Renderer adapters** translate Aurora scene/audio data into an external rendering engine and return PCM.
- **DSP adapters** pass PCM/configuration through an external processing engine.
- **Audio I/O adapters** expose host or platform audio endpoints through Aurora's generic interfaces.

## Rules

1. No adapter may redefine Aurora's core scene/channel semantics.
2. External versions and licenses must be pinned or documented.
3. Adapter failure must be explicit; no silent substitution with a weaker mode.
4. Core crates may not import device-specific SDKs or filesystem layouts.
5. A platform adapter must be removable without breaking the software core.
6. Protected-media access and DRM circumvention are out of scope.

The current immersive validation pins Harletty and Omniphony through `config/external-components-v1.json`; this is a validation/integration boundary, not a claim that either project is part of Aurora's core.
