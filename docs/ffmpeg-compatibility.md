# FFmpeg compatibility matrix

Aurora uses FFmpeg only for bounded external-process channel-audio roles: AC-3/E-AC-3 channel-bed decode from IEC61937/SPDIF, resampling, and sample-format conversion. FFmpeg is not an Aurora JOC/OAMD object renderer and a green result here must not be reported as Atmos object recovery.

## Reviewed versions and result

The compatibility lane compares three exact upstream release commits:

| Lane | Release | Exact commit | Result | Decision |
| --- | --- | --- | --- | --- |
| baseline | 6.1.1 | `e38092ef9395d7049f871ef4d5411eb410e283e0` | PASS | historical/pre-matrix baseline |
| maintenance | 6.1.6 | `f1e3a2bf7a2f2cde936d1ed97f09a26853d20125` | PASS | promoted pinned maintenance baseline |
| stable | 9.0.1 | `bf1b838f2ab88b4f8fd83443325c782ea0e0f7fa` | PASS | green major-version evaluation candidate |

The release commits were resolved from FFmpeg's signed upstream release tags. CI checks out the exact commit rather than following a branch or `latest` alias. Because the CI checkout is shallow and by commit SHA, FFmpeg's generated version string can report the short commit SHA instead of the release label; CI therefore verifies the full Git commit exactly and accepts either that short SHA or the reviewed release label in `ffmpeg -version`.

## Gate

`.github/workflows/ffmpeg-compatibility-ci.yml` builds each exact commit independently and runs the existing Aurora test:

`validation/surround-upmix/test_surround_upmix.py`

That test exercises real AC-3/E-AC-3 encoding and IEC61937/SPDIF carriage, then checks Aurora's channel-based path for:

- decoded-bed preservation and channel order;
- synthetic height activity only where expected;
- center, LFE, and mono isolation from the synthetic heights;
- PCM output before input EOF;
- invalid-input rejection without PCM output;
- host-side throughput reporting.

Each lane also records the FFmpeg build configuration, demuxers, muxers, decoders, filters, binary SHA-256, release commit, and a JSON evidence summary.

The reviewed run on 2026-09-14 passed all three lanes on the same bounded gate. This justifies moving Aurora's recorded FFmpeg maintenance baseline from 6.1.1 to exact-pinned 6.1.6. The successful 9.0.1 lane demonstrates that this specific external-process path also works against the reviewed stable major release, but Aurora does not adopt that major-version jump solely from this matrix.

## Raw TrueHD channel-bed extension (2026-10-04)

The existing worker accepts `--input-format truehd` for raw TrueHD, while no arguments retain IEC61937/SPDIF ingestion. The format allowlist rejects unknown values. This is an external channel-bed/upmix worker, not a `truehdd` Rust adapter or native Atmos/DAMF renderer.

`validation/surround-upmix/test_truehd_bed.py` generates independent per-channel 24-bit signals, encodes them with FFmpeg's experimental TrueHD encoder and requires an exact lossless six-channel round trip. It then executes Aurora's worker and verifies unchanged eight-channel reference bed samples inside the twelve-channel output, active synthetic heights and rejection of invalid data/formats. No proprietary or protected capture is used. This self-generated fixture does not establish general TrueHD compatibility.

The first local host run used FFmpeg 8.1.1 and passed 4800 frames with zero bed difference. Its binary version/hash are emitted in the JSON report. The existing exact-commit compatibility CI now runs this additional gate; earlier matrix PASS results above cover the original AC-3/E-AC-3 scope and do not retrospectively verify TrueHD. The accepted pin stays unchanged pending fresh evidence.

## Baseline decision

`config/external-components-v1.json` records FFmpeg 6.1.6 at commit `f1e3a2bf7a2f2cde936d1ed97f09a26853d20125` as the tested maintenance baseline.

FFmpeg 9.0.1 remains an explicitly green evaluation candidate. A future major-baseline update should additionally review broader container/API behavior and any other Aurora paths introduced after this matrix rather than silently inheriting the result here.

## Truth boundary

This matrix does **not** establish JOC/OAMD object recovery, IAMF rendering equivalence, protected-service compatibility, HDMI/eARC electrical behavior, hardware latency, physical channel output, or certification. It also does not prove every FFmpeg feature/API remains compatible; it proves the explicit CLI behaviors Aurora currently relies on in the tested channel-audio path.
