# Focused KEMAR Lookup and Motion Validation Plan

## Decision

Use only the few ideas from `syhw/binaural_from_surround` that materially improve Aurora. Do not copy its Python implementation and do not create a separate subsystem.

Tracking issue: #54

## Useful idea 1: irregular-grid direction lookup

Aurora must not assume uniform HRTF spacing. For each admitted dataset:

- enumerate the real measurement directions;
- convert them to Aurora's canonical coordinate frame;
- build a deterministic nearest-neighbor and neighbor index outside realtime;
- return both requested and resolved directions;
- report angular lookup error;
- reject or explicitly clamp unsupported elevations according to policy;
- never present nearest-neighbor snapping as interpolation.

For MIT KEMAR, the irregular elevation-dependent azimuth spacing must be generated from dataset metadata or a manifest, not hard-coded rounding loops.

## Useful idea 2: manifest-gated symmetry optimization

Where the dataset explicitly guarantees left-right symmetry, Aurora may store one side and mirror directions by swapping ears.

Requirements:

- symmetry must be declared in the dataset manifest;
- validate 0-degree and 180-degree special cases;
- compare mirrored output with explicitly stored opposite-side measurements when available;
- disable this optimization for datasets without documented symmetry;
- report memory reduction and any measurable mismatch.

## Useful idea 3: moving-HRTF transition baseline

The reference project's blockwise overlap-add motion is only a baseline. Aurora must compare:

- abrupt nearest-direction switching;
- dual-filter crossfade;
- partitioned-convolution filter crossfade;
- HRIR interpolation before convolution.

Motion trajectories must be defined in sample time and remain independent of block size. Convolution tails must be preserved.

## Required correctness foundation

Before accepting optimized HRTF processing:

- provide an exact offline linear-convolution reference;
- compare optimized block or partitioned convolution against it;
- avoid circular convolution;
- preserve output tails and deterministic latency;
- perform FFT plan creation and allocation outside realtime.

## Metrics

Through issue #44, record only metrics that decide implementation quality:

- angular lookup error;
- transition discontinuity energy;
- ITD and ILD continuity;
- spectral jump during motion;
- moving-tone pitch modulation;
- CPU and memory;
- deterministic output checksum.

## Explicitly rejected ideas

Do not adopt:

- Python/Scipy runtime code;
- hand-coded filename logic as a public API;
- silent clamping or upward-only rounding;
- FFT multiplication without linear-convolution padding;
- fixed int16 output scaling;
- normalization by one positive peak;
- motion step count derived from arbitrary window size;
- buffers that discard convolution tails.

## Integration

This work refines issues #46 and #52 and the offline Aurora HRTF renderer. It does not introduce a new product feature or duplicate issue #53.
