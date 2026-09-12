# AOMedia OAR external reference lane

Aurora evaluates the Alliance for Open Media Open Audio Renderer (OAR) as an **external reference oracle**, not as an Aurora production renderer.

## Pin and provenance

The evaluation contract is `config/oar-evaluation-v1.json`.

Pinned reference:

- upstream: `https://github.com/AOMediaCodec/oar`
- version: `1.0.0`
- commit: `5601d50c05a5e71cac7e80babeff7dd2a53b2060`
- source license: BSD-3-Clause-Clear
- upstream `PATENTS` file: required and independently checked by the lane

The runner never follows upstream `main` implicitly.

## Current evidence

`validation/open-immersive/test-oar-reference.sh` clones the exact commit, requires both `LICENSE` and `PATENTS`, builds the reference with CMake in Release mode, enables the upstream examples and binaural renderer, runs CTest, and then emits `oar-reference-evidence.json` through `oar_reference_evidence.py`.

First CI evidence on 2026-09-12:

- expected commit = actual commit = `5601d50c05a5e71cac7e80babeff7dd2a53b2060`;
- `LICENSE` present;
- `PATENTS` present;
- upstream tests: **6/6 passed**;
- covered upstream test executables: audio-element types, channel-based rendering, scene/HOA rendering, object-based rendering, metadata-unit processing, and element removal/re-addition;
- evidence verdict: `pass`.

This establishes that the pinned OAR tree is reproducibly usable as an external reference on the CI host. It does **not** establish Aurora IAMF rendering correctness.

## Promotion rule

`iamf-oar-open-rendering` in `config/simulation-coverage-v1.json` must remain `planned` until Aurora has executable differential evidence against the pinned OAR reference.

The next reference milestone should compare deterministic semantics that overlap cleanly between Aurora and OAR, beginning with:

1. object-position and gain trajectories;
2. channel/layout conversion behavior for common layouts such as stereo, 5.1 and 7.1.4;
3. frame accounting, finite output and channel ordering;
4. scene/HOA behavior where Aurora gains a matching scene representation;
5. binaural/head-rotation invariants only after Aurora has a real binaural path.

Do not compare raw PCM hashes across unrelated render algorithms as a correctness requirement unless both paths are intentionally expected to be bit-identical. Prefer invariant- and tolerance-based differential metrics.

## License and patent boundary

OAR's source license is BSD-3-Clause-Clear, and the repository also carries an Alliance for Open Media `PATENTS` license. Aurora's current lane does not vendor or link OAR into the Rust core. Any future distribution, static/dynamic linking, or production implementation decision requires review of both upstream files and the exact artifacts being shipped.

## Truth boundary

OAR reference PASS does not prove:

- IAMF integration into Aurora;
- equivalence to Dolby Atmos or any proprietary renderer;
- authored-position equivalence across formats;
- physical eARC/UAC2/TDM/DAC behavior;
- acoustic performance;
- protected streaming-service compatibility;
- certification or conformance by Dolby, AOMedia, or any other organization.
