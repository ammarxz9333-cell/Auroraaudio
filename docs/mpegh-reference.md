# MPEG-H dual-oracle reference lane

Aurora does not currently claim native or production MPEG-H runtime support. This lane exists only to cross-check two independent, exact-pin external MPEG-H 3D Audio decoder implementations against the same public upstream fixture.

## Exact external references

| Reference | Exact commit | Aurora role |
| --- | --- | --- |
| Fraunhofer IIS `mpeghdec` | `4448b69738da2fa5f2f2f2b0ce29eea32509e046` | external decode/render oracle |
| Ittiam `libmpegh` | `f7ff0ac78d4d83f0b853bf2dff2ef075c92724f8` | external decode/render oracle and fixture source |

The Fraunhofer build at this pin declares `ilo` tag `r2.0.2` and `mmtisobmff` tag `r1.0.4`. CI additionally verifies those tags resolve to commits `b8cb3ebf73789fc67278ccb15dde20632f1c2e05` and `e2969132bef16ac68d0fd974ee2324d8e209a5bd`, respectively.

Neither decoder is vendored, linked into Aurora core, or made a runtime dependency by this lane.

## Shared fixture

The initial gate consumes the public Ittiam upstream fixture directly from the exact pinned checkout:

`smoke_test_suite/inp/sine_1khz_cicp6.mp4`

The fixture is not copied into Aurora. Both decoders are asked to render target CICP 6, which is expected to produce six output channels. Using the MP4/ISOBMFF form is deliberate: it is directly supported by both selected command-line reference paths.

## Comparison

Both rendered WAV outputs are probed and normalized to interleaved `f32le`. The evidence analyzer fails closed on:

- invalid, empty, non-finite, or silent PCM;
- unequal sample rates or channel counts;
- output other than the expected six-channel CICP-6 target;
- excessive post-priming frame-count divergence;
- an active channel appearing active in only one decoder;
- low same-index active-channel correlation after a small bounded residual-lag search;
- large active-channel RMS-ratio divergence.

The analyzer detects the first active frame independently for each speaker channel before correlation. This is necessary because the shared fixture does not energize every active speaker at the same instant. Per-channel trimming is not allowed to hide timing errors: each channel's A-minus-B onset offset must remain consistent with the stream-wide A-minus-B onset offset within the same bounded residual-lag window. Same-index channel matching remains mandatory; cross-channel best matches are recorded only as failure diagnostics and never authorize an implicit speaker permutation.

Cross-vendor output is **not** required to be bit-identical. Decoder implementations can differ in sample representation, bounded priming, and rendering details. The lane records the observed frame counts, stream-wide and per-channel leading activity offsets, per-channel residual lag, correlation, RMS ratio, fixture hash, PCM hashes, exact upstream pins, dependency pins, and CI-built binary hashes.

The initial thresholds are defined in `config/mpegh-reference-v1.json`; changing them is an evidence-policy change and should be reviewed rather than silently relaxed after a failure.

## License and patent boundary

The Fraunhofer FDK MPEG-H source license explicitly states that it grants no patent license. Ittiam's source license is permissive in form, but its additional license notice likewise states that patent rights are not granted and that additional third-party patent licenses may be required. A successful CI run is therefore technical evidence only; it is not patent, trademark, redistribution, codec-pool, or certification clearance.

Aurora uploads only JSON evidence from this CI lane. It does not publish the decoder binaries or copy the third-party fixture into Aurora artifacts.

## What a pass means

A pass means only that these two pinned external decoder builds can render this one pinned public CICP-6 fixture into structurally compatible, strongly correlated six-channel PCM under the tested Linux CI environment.

It does **not** prove:

- Aurora runtime MPEG-H decoding;
- object or HOA metadata ingestion into Aurora;
- object identity, trajectory, or interaction semantics;
- 7.1.4 or 11.1.4 MPEG-H rendering;
- exhaustive MPEG-H conformance;
- protected-service interoperability;
- physical HDMI/eARC, DAC, amplifier, or speaker behavior;
- patent or trademark clearance; or
- product certification.

Future expansion can add additional shared fixtures and layouts only when the same evidence discipline can be preserved across both independent reference implementations.
