# Shared output DSP fast paths

Flat calibration previously allocated a 4801 x 12 float ring, wrote every sample and read the same frame even with zero delay and no correction. Preparation now records an identity path and allocates this ring only when a validated channel delay is nonzero. EQ/trim/polarity with zero delay still execute, directly on each channel. Config replacement remains setup-only and resets history as before.

Lip-sync continues writing all frames, even at zero delay, so future delay requests can use real retained history. It skips the redundant zero-delay read/copy. Nonzero calibration/lip-sync read positions use bounded subtraction/wrap rather than modulo. Delay limits and 240-frame crossfades are unchanged. The master gain target is invariant within a block and is computed once; per-sample smoothing, mute placement, bass routing and limiter behavior remain intact.

## Local release benchmark — 2026-10-04

Measured with the existing Criterion benchmark on the same Windows GNU host, Rust 1.99, locked dependencies, 100 samples and default warmup. Baseline production code is `f6ce6422086e2c669acb3045d3edf21fd4f50fc2`; only the flat benchmark case was added before measuring it. Both versions process 40 frames x 12 channels at 48 kHz (833.33 microseconds of audio).

| Configuration | Before time interval, microseconds | After time interval, microseconds | Criterion estimated change |
| --- | --- | --- | --- |
| Flat, no calibration delay | 5.5840–5.6761 | 4.8750–5.0684 | -12.521% (interval -14.650% to -10.277%) |
| 48-frame calibration delay, no PEQ | 5.9057–6.2185 | 5.2157–5.3813 | -8.3924% (interval -11.159% to -5.3715%) |
| 48-frame delay, eight PEQ bands/channel | 13.282–13.569 | 12.046–12.396 | -7.5946% (interval -9.6707% to -5.5128%) |

The two no-delay calibration paths avoid 230448 bytes of delay storage. The lip-sync ring stays allocated to permit bounded realtime delay changes without allocation. Local wall-clock samples are sensitive to host scheduling and frequency/load; these results do not establish target CPU worst-case execution time, end-to-end latency or superiority over a commercial system.

Reproduce sequentially with `cargo bench --locked -p aurora-dsp-basic --bench output -- --save-baseline before-fast-path`, then the optimized revision with `--baseline before-fast-path`. Retain the same benchmark definitions and target directory. Raw logs and Criterion data remain under ignored local output/target directories.

## Behavior gates

- Bitwise differential against the previous buffered calibration operation, with mixed EQ/trim/polarity, channel delays 0/1/48/4800, multiple ring wraps, signed zero and reset.
- Bitwise differential against the previous lip-sync operation over 50000 frames, including zero-to-nonzero history, maximum delay, queued requests during a crossfade, return to zero, wrap and reset.
- Zero allocations while processing/controlling flat and zero-delay trimmed paths; retained worst-case eight-band processing/control/reset allocation proof.
- Existing malformed configuration, peak/limiter, finite samples, polarity, mute timing and per-role shared-output gates stay required.

The release per-role self-test passed all twelve roles. Replaying the existing moving-object PCM fixture through the optimized shared processor produced 3624960 frames (75.52 seconds) with byte-identical output to the previous processor: SHA-256 `4e097140ea4afbaa632f324f8a9c2f02d4f80add821f2921e909bfe4d34a18e3`. This is file replay of software-produced speaker PCM, not a fresh Netflix/eARC/physical chain test.
