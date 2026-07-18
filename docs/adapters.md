# Third-Party Adapter Layer

Aurora keeps optional third-party integrations behind Aurora-owned interfaces. Adapter code is admitted only when it supports the active product path, has a concrete consumer, and has a reviewable licensing and deployment posture.

## Adapter Policy

- Adapters implement Aurora-owned traits; third-party types do not cross product boundaries.
- External tools run out of process when that materially reduces unsafe, licensing, or lifecycle risk.
- Adapter-specific Cargo features are disabled by default.
- `aurora-core` and the production processing crates must compile without optional adapters.
- Every adapter must have a concrete product consumer, bounded failure behavior, and explicit maturity classification.
- Placeholder crates that only return `Unavailable` are not retained in the active workspace.
- Commercial use requires separate license, patent, and redistribution review.

## Active Adapters

### CamillaDSP

- Crate: `aurora-dsp-camilladsp`
- Aurora boundary: DSP control and offline processing owned by Aurora.
- Cargo feature: `camilladsp-process`.
- Integration style: generated CamillaDSP configuration plus offline file processing through an external process.
- Maturity: functional offline adapter, not production-ready.
- Source policy: do not fork or vendor CamillaDSP source into Aurora.
- Executable discovery order: explicit `--camilladsp-path`, `AURORA_CAMILLADSP_PATH`, then `PATH`.
- Process safety: no shell concatenation, bounded timeout, captured output, and output-WAV validation.

### IAMF / libiamf

- Crate: `aurora-decoder-iamf`.
- Aurora boundary: `aurora_decoder_api::Decoder`.
- Cargo feature: `libiamf-process`.
- Integration style: future external libiamf-compatible process for open immersive input.
- Maturity: planned adapter boundary; no production decoding claim.
- Source policy: do not copy libiamf source into Aurora.
- Product use requires codec, patent, distribution, and conformance review.

## Retired Experiments

The former `aurora-decoder-truehdd` and `aurora-renderer-cavern` placeholder crates were removed from the active workspace and repository during the structural cleanup. They had no functional integration and only returned unavailable/disabled results. Keeping them as buildable crates falsely suggested product capability and increased maintenance surface.

Aurora does not retain placeholder packages for speculative proprietary integrations. A future integration may be reconsidered only through a new architecture decision that identifies:

1. a concrete product requirement;
2. a lawful and commercially acceptable licensing path;
3. an Aurora-owned boundary;
4. a functional implementation plan rather than an unavailable stub;
5. deterministic software tests and, where relevant, physical validation gates.

Removal of a placeholder does not erase repository history. Historical commits remain available through Git, but they are not part of the current architecture or product roadmap.
