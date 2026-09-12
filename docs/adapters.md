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

The primary immersive validation pins Harletty and Omniphony through `config/external-components-v1.json`. Aurora also pins OpenJOC as an independent `evaluate-active` reference backend for fail-closed and differential JOC validation. These are validation/integration boundaries, not claims that any of those projects are part of Aurora's core or production-ready runtime dependencies.

OpenJOC remains outside the Aurora Rust workspace while its toolchain/MSRV requirements differ from Aurora's. Omniphony remains an isolated external process because of its GPL-3.0 licensing boundary. Exact third-party licensing posture is recorded in `THIRD_PARTY_LICENSES.md`.
