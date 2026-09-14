# FFmpeg compatibility matrix

Aurora uses FFmpeg only for bounded external-process channel-audio roles: AC-3/E-AC-3 channel-bed decode from IEC61937/SPDIF, resampling, and sample-format conversion. FFmpeg is not an Aurora JOC/OAMD object renderer and a green result here must not be reported as Atmos object recovery.

## Reviewed versions

The compatibility lane compares three exact upstream release commits:

| Lane | Release | Exact commit | Purpose |
| --- | --- | --- | --- |
| baseline | 6.1.1 | `e38092ef9395d7049f871ef4d5411eb410e283e0` | current Aurora recorded baseline |
| maintenance | 6.1.6 | `f1e3a2bf7a2f2cde936d1ed97f09a26853d20125` | latest 6.1 maintenance candidate |
| stable | 9.0.1 | `bf1b838f2ab88b4f8fd83443325c782ea0e0f7fa` | current stable candidate reviewed on 2026-09-14 |

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

## Decision policy

The registry remains at FFmpeg `6.1.1` until this matrix is green and reviewed. A green `6.1.6` lane makes it eligible as a maintenance-baseline update. A green `9.0.1` lane establishes compatibility with this bounded software path and makes it eligible for broader evaluation; it does not by itself require Aurora to jump major versions.

Any baseline update is a separate evidence-based change so a version bump cannot be hidden inside the test introduction.

## Truth boundary

This matrix does **not** establish JOC/OAMD object recovery, IAMF rendering equivalence, protected-service compatibility, HDMI/eARC electrical behavior, hardware latency, physical channel output, or certification. It also does not prove every FFmpeg feature/API remains compatible; it proves the explicit CLI behaviors Aurora currently relies on in the tested channel-audio path.
