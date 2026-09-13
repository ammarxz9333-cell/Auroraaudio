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

## Current reference evidence

`validation/open-immersive/test-oar-reference.sh` clones the exact commit, requires both `LICENSE` and `PATENTS`, builds the reference with CMake in Release mode, enables the upstream examples and binaural renderer, runs CTest, and emits `oar-reference-evidence.json` through `oar_reference_evidence.py`.

First CI evidence on 2026-09-12:

- expected commit = actual commit = `5601d50c05a5e71cac7e80babeff7dd2a53b2060`;
- `LICENSE` present;
- `PATENTS` present;
- upstream tests: **6/6 passed**;
- covered upstream test executables: audio-element types, channel-based rendering, scene/HOA rendering, object-based rendering, metadata-unit processing, and element removal/re-addition;
- evidence verdict: `pass`.

This establishes that the pinned OAR tree is reproducibly usable as an external reference on the CI host. It does **not** establish Aurora IAMF rendering correctness.

## Focused stereo differential slice

The first Aurora-vs-OAR differential slice is `stereo-object-position-gain-v1`. It deliberately compares shared semantics instead of requiring unrelated render algorithms to emit identical PCM.

Both renderers process the same four semantic cases at 48 kHz with 256 frames per case:

1. left object at unity gain;
2. centered object at unity gain;
3. right object at unity gain;
4. centered object at -6 dB.

The channel contract is explicitly `[FL, FR]`. Coordinate-sign differences are normalized at the semantic boundary: Aurora's fixtures use negative azimuth for left and positive for right, while the pinned OAR stereo layout uses positive azimuth for left and negative for right. Therefore the differential probes use Aurora `-45/0/+45` degrees and OAR `+45/0/-45` degrees for left/center/right respectively.

`validation/open-immersive/oar_differential.py` checks:

- exact sample-rate and frame accounting;
- finite output and non-silent semantic cases;
- exact FL/FR channel ordering;
- left and right channel dominance with a configured margin;
- center balance within a configured tolerance;
- normalized FL/FR spatial-distribution agreement within a configured tolerance;
- -6 dB object-gain semantics independently in each renderer and across the two renderers.

The numerical limits are machine-readable in `config/oar-evaluation-v1.json`. The OAR probe is injected only into the temporary pinned checkout and is linked against OAR's existing test helper/library targets; it is not registered as an upstream CTest, so the original upstream test count remains independently visible. The Aurora probe uses Aurora's actual `VbapRenderer` and renderer API.

This slice is intentionally narrower than IAMF acceptance. It bypasses Aurora's IAMF adapter and does not prove IAMF bitstream ingestion, IAMF decoding, 5.1/7.1.4 differential equivalence, scene/HOA equivalence, binaural behavior, or raw-PCM identity.

## Promotion rule

`iamf-oar-open-rendering` in `config/simulation-coverage-v1.json` remains `planned`. A stereo renderer-semantic comparison is useful independent evidence, but it is not sufficient to promote the IAMF capability. Promotion requires executable IAMF decode/render evidence through Aurora's own IAMF boundary plus the applicable layout and failure-mode coverage.

Follow-on differential work should extend cleanly overlapping semantics to common multichannel layouts such as 5.1 and 7.1.4, then scene/HOA behavior where Aurora has a matching representation. Binaural/head-rotation invariants belong only after Aurora has a real matching path.

Do not compare raw PCM hashes across unrelated render algorithms as a correctness requirement unless both paths are intentionally expected to be bit-identical. Prefer invariant- and tolerance-based differential metrics.

## License and patent boundary

OAR's source license is BSD-3-Clause-Clear, and the repository also carries an Alliance for Open Media `PATENTS` license. Aurora's current lane does not vendor or link OAR into the Rust core. Any future distribution, static/dynamic linking, or production implementation decision requires review of both upstream files and the exact artifacts being shipped.

## Truth boundary

OAR reference or focused differential PASS does not prove:

- IAMF integration into Aurora;
- equivalence to Dolby Atmos or any proprietary renderer;
- authored-position equivalence across formats beyond the explicitly mapped stereo semantic cases;
- physical eARC/UAC2/TDM/DAC behavior;
- acoustic performance;
- protected streaming-service compatibility;
- certification or conformance by Dolby, AOMedia, or any other organization.
