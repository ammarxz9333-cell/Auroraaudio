# Aurora DSP Primitives Extraction Plan

## Status

- Decision: **adapt selected ideas, do not adopt `cycfi/q` as Aurora's DSP core**.
- Tracking issue: `#50`.
- Dependency: issue `#44` must establish common artifact and performance evidence first.
- Integration target: issue `#43`, Checkpoint B, for moving-source delay continuity.

## Objective

Build a small Aurora-owned Rust DSP foundation for realtime-safe spatial-audio operations. The first target is a bounded fractional-delay primitive that removes integer-delay jumps and supports measurable moving-source continuity.

## 1. Fractional-delay primitive

Implement an Aurora-owned delay line with:

- preallocated bounded ring storage;
- fractional read positions;
- explicit interpolation policy;
- linear interpolation as the initial implementation;
- finite, non-negative, in-range delay validation;
- deterministic construction and reset semantics;
- no allocation, locks, blocking calls, filesystem access, or panics in the processing path;
- sample-wise and preallocated-block processing surfaces;
- no hidden global sample rate.

Do not replace the current Aurora delay path until a direct comparison exists.

## 2. Dynamic delay continuity

Evaluate two transition policies:

### Smooth read-position ramp

Move the read position continuously from the old delay to the new delay over a bounded transition interval.

Measure:

- transient peak;
- click/discontinuity energy;
- induced pitch or Doppler modulation;
- spectral error;
- CPU cost.

### Dual-tap crossfade

Run old-delay and new-delay taps concurrently and crossfade between them when the requested change exceeds a configured threshold.

Measure the same metrics and additionally:

- temporary comb-filtering risk;
- transition-length sensitivity;
- double-read memory bandwidth.

The accepted strategy may be hybrid: ramp for small continuous changes and dual-tap crossfade for large discontinuous jumps.

## 3. Required fixtures

Use deterministic fixtures through issue `#44`:

- stationary impulse;
- stationary sine at several frequencies;
- rotating source at constant angular velocity;
- abrupt azimuth jump;
- near-zero distance;
- maximum allowed delay;
- repeated render and reset;
- final partial block;
- invalid NaN, infinity, negative, and out-of-range inputs.

## 4. Acceptance metrics

The implementation is not accepted until reports include:

- impulse-response correctness;
- analytical or reference-vector interpolation tolerance;
- continuity/discontinuity score;
- peak overshoot;
- frequency-response or spectral-error summary;
- pitch-modulation estimate for moving fixtures;
- Criterion timing results;
- p50, p95, and p99 processing cost where meaningful;
- allocation audit or no-allocation assertion;
- bounded memory evidence;
- exact commit SHA and configuration hash.

Thresholds must be explicit in the implementing PR rather than described qualitatively.

## 5. Composable primitive contract

Every accepted Aurora DSP primitive must:

- own its state explicitly;
- construct and validate configuration outside realtime;
- provide explicit `reset` behavior;
- avoid hidden shared state;
- avoid trait-object dispatch in the innermost loop unless benchmarks justify it;
- expose stable Aurora-owned public interfaces;
- support deterministic tests independent from CPAL, networking, and external processes.

Static generic composition may be used internally where it improves optimization without leaking complexity into public APIs.

## 6. Candidate primitive backlog

### Priority A

- fractional delay and interpolation;
- moving sum and moving average;
- peak envelope follower;
- RMS envelope follower;
- attack-release envelope follower;
- bounded smoothing utilities;
- simple differentiator.

### Priority B, only with a concrete product need

- state-variable filter for diagnostics, calibration, or bounded core DSP;
- signal conditioning helpers;
- best-lag or correlation helpers for measurement.

### Evaluate separately

- FFT abstraction, only after comparison with established Rust FFT crates such as `rustfft` and after identifying a concrete convolution or measurement requirement.

### Deferred

- synthesizers;
- oscillators;
- granular effects;
- pitch tracking;
- noise gates;
- music-production effects;
- PortAudio and PortMidi I/O.

## 7. Licensing and derivation

- Prefer clean-room Rust implementations from published DSP principles and Aurora requirements.
- Do not copy C++ code mechanically.
- Record source references and mathematical derivations in the implementing PR.
- If code is closely derived, perform explicit Boost Software License attribution and legal review.
- `q_io`, PortAudio, PortMidi, C++ objects, raw pointers, and native ABI types must not enter Aurora public APIs.

## 8. Implementation sequence

1. Issue `#43A` documents the current abrupt-delay and dynamic-continuity limitation.
2. Issue `#44` lands the common artifact, benchmark, and memory-evidence runner.
3. Issue `#50A` compares the existing Aurora delay path with a bounded fractional-delay prototype.
4. Issue `#50B` implements linear interpolation and complete correctness tests.
5. Issue `#50C` compares read-position ramping with dual-tap crossfade.
6. Issue `#43B` integrates the accepted continuity strategy into `GeometricBinaural`.
7. Additional primitives are implemented only as separate reviewable PRs tied to a concrete product requirement.

## 9. Completion rule

Issue `#50` is complete only when:

- one Aurora-owned fractional-delay implementation exists;
- linear interpolation is tested and benchmarked;
- ramp and crossfade strategies have comparable artifacts;
- an explicit strategy is accepted for moving-source continuity;
- invalid inputs and boundaries are covered;
- realtime paths are allocation-free or have a documented exception;
- no `cycfi/q`, `q_io`, PortAudio, or PortMidi runtime dependency is added;
- capability claims and limitations are reflected in the capability registry.