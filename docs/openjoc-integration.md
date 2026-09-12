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

## Reference acceptance lane

`validation/immersive/test-openjoc-reference.sh` is fail-closed and requires an explicit JOC fixture. Invoke it with `bash` so the validation contract does not depend on the checkout preserving executable bits.

The lane:

1. verifies that the `openjoc` executable is available and matches the pinned version;
2. runs `openjoc inspect INPUT --json --objects --emdf`;
3. requires the inspector contract to positively report `joc.present == true`;
4. requires at least one complete E-AC-3 access unit, `stream_parse == pass`, and at least one JOC profile;
5. renders the input to a 7.1.4 WAV with `openjoc render-joc`;
6. verifies the rendered file with `ffprobe`, requiring 12 channels, a positive sample rate, and positive duration;
7. emits `OPENJOC-REFERENCE-PASS` only after every admission and output check succeeds.

This prevents ordinary E-AC-3 or malformed/ambiguous inspection output from being silently counted as JOC playback evidence.

The lane is software evidence only. It does not establish hardware eARC capture, streaming-service compatibility, Dolby certification, or production readiness.

## Differential validation

`validation/immersive/test-joc-differential.sh` executes both independent JOC paths against the same deterministic fixture from the pinned Harletty commit:

- Harletty + Omniphony through Aurora's existing immersive validation lane;
- OpenJOC through the fail-closed reference lane above.

The script records the fixture SHA-256, keeps each renderer's artifacts in a common work directory, normalizes OpenJOC output to 12-channel 48 kHz `f32`, and produces `joc-differential-report.json` with:

- frame count and programme duration for each path;
- per-channel RMS and peak values;
- duration divergence;
- per-channel correlation over the common frame interval.

PCM correlation is deliberately non-gating because the renderers are independent and are not expected to produce numerically identical samples. The initial semantic gate rejects invalid/silent outputs and programme-duration divergence above 250 ms. That tolerance is intentionally conservative until a larger fixture corpus establishes a tighter evidence-based bound.

A successful run emits `AURORA-JOC-DIFFERENTIAL-PASS` only after both underlying JOC lanes have passed.

Future extensions should add channel-label normalization, object-count/scene telemetry comparison, decoder/render timing, and discontinuity/error counters before correlations are interpreted more strongly.

## Promotion gate

OpenJOC may move from `evaluate-active` to an Aurora runtime adapter only after:

- the pinned external-process lane passes on the same fixtures used by the existing JOC lane;
- differential evidence is collected across more than one fixture and includes discontinuity/error telemetry;
- channel/layout semantics are normalized at one Aurora-owned boundary;
- failure is fail-closed (no silent downgrade presented as object playback);
- realtime-process/ABI buffering and latency are measured separately;
- licensing/notices for the exact distributed package are recorded.
