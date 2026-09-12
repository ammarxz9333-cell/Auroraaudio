# OpenJOC integration plan

Aurora tracks OpenJOC as an independent E-AC-3 JOC decode/render reference backend. The integration is deliberately outside the Aurora Rust workspace so Aurora can keep its Rust 1.78 MSRV while OpenJOC 0.17.0 requires Rust 1.85 / edition 2024.

## Pinned upstream

- project: `chyinan/OpenJOC`
- version: `0.17.0`
- commit: `9b2158bd787f9ef4d62971c1773a5d1bb7408ecd`
- license: Apache-2.0 for the OpenJOC core; package-specific notices still apply

## Boundary

OpenJOC is not part of `aurora-core`. Aurora may use it through either:

1. an external `openjoc` process for validation and offline differential testing; or
2. a future versioned C-ABI adapter loaded outside the realtime callback.

No OpenJOC Rust crate is added to the Aurora workspace while the two projects have incompatible MSRV policies.

## First acceptance lane

`validation/immersive/test-openjoc-reference.sh` provides the first fail-closed lane. It requires an explicit JOC fixture and:

1. verifies that the `openjoc` executable is available;
2. records `openjoc --version`;
3. runs `openjoc inspect INPUT --json` and requires valid JSON output;
4. renders `INPUT` to a 7.1.4 WAV with `openjoc render-joc`;
5. verifies the resulting WAV with `ffprobe` and requires 12 channels;
6. emits `OPENJOC-REFERENCE-PASS` only after all checks succeed.

The lane is software evidence only. It does not establish hardware eARC capture, streaming-service compatibility, Dolby certification, or production readiness.

## Differential validation

The next gate compares one pinned input through both independent JOC paths:

- Harletty + Omniphony: Aurora's existing immersive validation lane;
- OpenJOC: the reference lane above.

The comparison should record, at minimum:

- detected JOC admission/result;
- decoded duration and frame continuity;
- output sample rate and channel count;
- channel labels/order where available;
- object-count / scene metadata telemetry where exposed;
- per-channel RMS/peak and inter-path correlation after layout normalization;
- decoder/render time and discontinuity/error counters.

A numerical PCM mismatch is not automatically failure because the renderers are independent. Failures are semantic: wrong layout, missing/duplicated programme time, discontinuity, invalid output, or unexplained scene/object loss.

## Promotion gate

OpenJOC may move from `evaluate-active` to an Aurora runtime adapter only after:

- the pinned external-process lane passes on the same fixtures used by the existing JOC lane;
- channel/layout semantics are normalized at one Aurora-owned boundary;
- failure is fail-closed (no silent downgrade presented as object playback);
- realtime-process/ABI buffering and latency are measured separately;
- licensing/notices for the exact distributed package are recorded.
