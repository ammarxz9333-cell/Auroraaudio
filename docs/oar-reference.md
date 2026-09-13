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

This slice is intentionally narrower than IAMF acceptance. It bypasses Aurora's IAMF adapter and does not prove IAMF bitstream ingestion, IAMF decoding, multichannel differential equivalence, scene/HOA equivalence, binaural behavior, or raw-PCM identity.

## Focused 5.1 differential slice

The next slice is `five-one-object-position-gain-v1`. It keeps the same invariant-based method but extends it to the explicit six-channel contract `[FL, FR, FC, LFE, SL, SR]` at 48 kHz and 256 frames per case.

The six cases are exact speaker-target semantics rather than arbitrary trajectory points:

1. front-left object at unity gain;
2. centered object at unity gain;
3. front-right object at unity gain;
4. surround-left object at unity gain;
5. surround-right object at unity gain;
6. centered object at -6 dB.

Aurora uses `-30/0/+30/-110/+110` degrees for FL/FC/FR/SL/SR. The pinned OAR layout uses the opposite azimuth sign, so its corresponding positions are `+30/0/-30/+110/-110` degrees. Both paths preserve the same semantic channel order.

The 5.1 slice also makes the point-object LFE rule explicit. OAR's object renderer obtains a speaker layout with LFE removed before directional gain calculation. Aurora therefore uses `ObjectVbapRenderer`, an allocation-free wrapper around the existing horizontal `VbapRenderer`: it removes enabled `LowFrequencyEffects` speakers from point-object panning, renders against the remaining directional speakers, then maps results back into the complete enabled output order with an explicit finite zero-gain LFE slot. This is an object-rendering semantic; it does not globally disable LFE for channel-based or separately authored effects content.

`validation/open-immersive/oar_5_1_differential.py` checks:

- exact 48 kHz sample rate and 256-frame accounting;
- finite output;
- exact `[FL, FR, FC, LFE, SL, SR]` ordering;
- concentration on the expected target speaker for each exact-position case;
- no point-object energy routed into LFE above the configured relative limit;
- normalized non-LFE spatial-distribution agreement across Aurora and OAR;
- -6 dB object-gain semantics independently and across the two renderers.

The OAR 5.1 probe is another temporary validation executable injected into the exact pinned checkout and is not registered with CTest. Therefore OAR's independent upstream reference count remains 6/6. The Aurora 5.1 probe uses the actual `ObjectVbapRenderer` implementation through the normal renderer API.

This evidence remains deliberately narrow. It does not prove IAMF bitstream ingestion/decoding, arbitrary between-speaker object trajectories, height rendering, 7.1.4 differential equivalence, scene/HOA behavior, physical output, or acoustic performance.

## Promotion rule

`iamf-oar-open-rendering` in `config/simulation-coverage-v1.json` remains `planned`. Stereo and 5.1 renderer-semantic comparisons are useful independent evidence, but they are not sufficient to promote the IAMF capability. Promotion requires executable IAMF decode/render evidence through Aurora's own IAMF boundary plus the applicable layout and failure-mode coverage.

Follow-on differential work should extend genuinely overlapping semantics to 7.1.4 only where both renderers expose an explicit comparable speaker contract, then scene/HOA behavior where Aurora has a matching representation. Binaural/head-rotation invariants belong only after Aurora has a real matching path.

Do not compare raw PCM hashes across unrelated render algorithms as a correctness requirement unless both paths are intentionally expected to be bit-identical. Prefer invariant- and tolerance-based differential metrics.

## License and patent boundary

OAR's source license is BSD-3-Clause-Clear, and the repository also carries an Alliance for Open Media `PATENTS` license. Aurora's current lane does not vendor or link OAR into the Rust core. Any future distribution, static/dynamic linking, or production implementation decision requires review of both upstream files and the exact artifacts being shipped.

## Truth boundary

OAR reference or focused differential PASS does not prove:

- IAMF integration into Aurora;
- equivalence to Dolby Atmos or any proprietary renderer;
- authored-position equivalence across formats beyond the explicitly mapped stereo and 5.1 semantic cases;
- arbitrary object trajectories or height rendering;
- physical eARC/UAC2/TDM/DAC behavior;
- acoustic performance;
- protected streaming-service compatibility;
- certification or conformance by Dolby, AOMedia, or any other organization.
