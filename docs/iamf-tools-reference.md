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

The first gate intentionally proves only that the exact pin is usable as a reproducible external reference:

1. checkout and verify the exact upstream commit;
2. build `encoder_main`, `decoder_main`, and `probe_main` using the upstream Bazel graph;
3. probe a pinned upstream IAMF fixture in machine-readable JSON form;
4. scan temporal-unit counts/duration;
5. decode the fixture to stereo WAV;
6. require non-empty, finite decoded PCM and record deterministic hashes/provenance.

The baseline fixture is:

`iamf/cli/testdata/iamf/tones_256samp_5p1_pcm.iamf`

The baseline deliberately uses an upstream fixture before Aurora adds an encoder-generated corpus. This separates basic tool qualification from later differential semantics.

## Next differential gate

After the baseline is green, a follow-up on this branch should compare the same reviewed IAMF vectors through the applicable lanes:

- AOMedia `iamf-tools` decoder/probe;
- `libiamf` rendered-channel-PCM reference;
- AOMedia OAR where rendering semantics are applicable;
- Aurora-owned import/validation tooling.

Comparison should focus on descriptor semantics, selected mix/layout, sample rate, channel count/order where exposed, frame/sample duration, finite PCM, relative channel energy, gain automation/parameter behavior, and explicit malformed-input outcomes. Sample-identical PCM is not required across independent renderers unless a particular test defines that stronger contract.

## Truth boundary

Passing this lane does not prove that Aurora natively decodes IAMF object scenes. The existing Aurora `libiamf` process adapter remains rendered-channel PCM only. It also does not prove universal IAMF conformance, product interoperability, patent clearance beyond upstream published terms, or certification.

A later decision to use `iamf-tools` in a product/runtime path requires a separate realtime, API-stability, performance, redistribution, and legal review.
