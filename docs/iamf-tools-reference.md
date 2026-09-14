# AOMedia iamf-tools validation reference

Aurora evaluates `AOMediaCodec/iamf-tools` as an independent external IAMF encoder, decoder, and probe reference. It is not linked into Aurora core and is not a selected production runtime backend.

## Pin

- Upstream: `https://github.com/AOMediaCodec/iamf-tools`
- Commit: `c6b11b5ea47d7da00fbac9654acc96aa4ced2388`
- Upstream Bazel version: `8.5.1`
- Aurora CI Bazelisk version: `1.27.0`
- License posture: BSD-3-Clause-Clear plus the Alliance for Open Media patent-license terms carried by the upstream repository; bundled web-demo files may carry additional notices.

The pin is reviewed and immutable for this validation lane. Aurora CI must not silently follow upstream `main` or `latest`.

## Why this lane exists

Aurora already has a pinned `libiamf` rendered-channel-PCM reference and a pinned AOMedia OAR renderer reference. `iamf-tools` is useful because upstream explicitly describes it as a different IAMF implementation with encoder, decoder, and probe interfaces. That gives Aurora another independently maintained implementation for cross-checking descriptors, temporal units, rendered PCM, and later encoder/decoder round trips.

## Baseline gate

The baseline proves only that the exact pin is usable as a reproducible external reference:

1. checkout and verify the exact upstream commit;
2. build `encoder_main`, `decoder_main`, and `probe_main` using the upstream Bazel graph;
3. probe a pinned upstream IAMF fixture in machine-readable JSON form;
4. scan temporal-unit counts/duration;
5. decode the fixture explicitly to stereo WAV;
6. require non-empty, finite decoded PCM and record deterministic hashes/provenance.

The baseline fixture is:

`output/iamf-tools-reference/baseline.iamf` (stereo PCM)

The pinned encoder generates this file from the upstream stereo PCM template and `sawtooth_10000_stereo_48khz_s24le.wav` (24,000 frames at 48 kHz). The template and WAV hashes are verified before encoding; populated metadata and encoded files are retained with their provenance. No bitstream bytes are patched.

Run `34880014504` failed because the older checked-in `tones_256samp_5p1_pcm.iamf` has a codec config that the pinned decoder reads as zero samples per frame. Its probe emitted JSON with missing descriptors, so JSON syntax alone was insufficient. The gate now requires nonempty codec/audio-element/mix descriptors and an error-free complete temporal scan. This replacement is explicitly stereo; it does not establish the originally proposed 5.1 downmix coverage.

## Cross-reference gate

The same CI lane encodes the same pinned source WAV using the upstream stereo Opus template, then performs a cross-reference on that generated file:

`output/iamf-tools-reference/differential.iamf` (stereo Opus)

That fixture is rendered independently through:

- pinned `iamf-tools` `decoder_main`, explicitly selecting stereo (`2.0`);
- pinned `libiamf` `iamfdec`, using Aurora's already reviewed complete-file rendered-PCM reference configuration.

Both outputs are converted to interleaved F32 only for evidence analysis. The analyzer requires valid finite non-silent stereo 48 kHz output from both implementations, checks bounded frame-accounting disagreement, and records per-channel RMS, energy fractions, normalized correlation, hashes, and duration delta. It does **not** require sample-identical PCM across the independent decoders/renderers.

`libiamf`'s reviewed build contains its own pinned OAR submodule revision. That embedded renderer revision is recorded separately from Aurora's standalone OAR semantic-oracle pin. They must not be conflated.

## OAR role boundary

Aurora's standalone AOMedia OAR lane remains an object/spatial-render semantic oracle for focused position, gain, channel-order, and LFE-exclusion differentials. In this IAMF cross-reference, standalone OAR is deliberately **not** presented as a third IAMF ingestion decoder. This keeps bitstream parsing/decoding evidence separate from renderer-only semantic evidence.

## What the differential establishes

A green cross-reference shows that the exact pinned `iamf-tools` and `libiamf` paths can independently consume the same reviewed standalone IAMF fixture and produce structurally valid stereo PCM with bounded frame accounting. The evidence artifact exposes the actual differences rather than converting independent-renderer variation into a false exact-equivalence claim.

Future corpus expansion can add 5.1/7.1.4 layouts, IAMF parameter automation, HOA, malformed/truncated cases, and encoder-generated round trips. Stronger tolerances should be introduced only after measured evidence supports them.

## Truth boundary

Passing this lane does not prove that Aurora natively decodes IAMF object scenes. The existing Aurora `libiamf` process adapter remains rendered-channel PCM only. It also does not prove universal IAMF conformance, arbitrary renderer equivalence, physical output behavior, product interoperability, patent clearance beyond upstream published terms, or certification.

A later decision to use `iamf-tools` in a product/runtime path requires a separate realtime, API-stability, performance, redistribution, and legal review.
