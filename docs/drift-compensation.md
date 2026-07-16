# Drift Compensation

## Strategy Boundary

`DriftCompensator` is owned by Aurora and called only by the output side of the
duplex bridge. It receives numeric fill and callback context, returns at most one
whole-frame insertion or removal, declares its transition and bounded window,
and accepts future input/output ratio updates. It retains no borrowed PCM.

This boundary can host sample slip/crossfade, linear interpolation, a future
polyphase asynchronous resampler, or an external adapter. The transport owns
bounded PCM storage; the compensator owns ratio/correction state; the consumer
owns fixed transition scratch. Every correction uses one frame boundary for all
channels, preserving multichannel phase coherence.

## Reference Transitions

- Raw: abrupt insertion/removal, tests and comparison only.
- Linear interpolation: distributes one correction across one interpolated frame.
- Crossfade: time-warps one removed frame over a configurable fixed window.
- Zero crossing: defers until every channel is near zero or changes sign, then
  applies interpolation. It may wait indefinitely for correlated material.

The default is a 16-frame crossfade. No mode is production ASRC. A polyphase
ASRC would provide better band limitation and smooth ratio updates at higher CPU,
state, and algorithmic-latency cost.

## Artifact Measurements

The benchmark applies one coherent removal to deterministic 750 Hz sine and
noise fixtures. Spectral artifact energy is mean squared first-difference error,
a deterministic high-frequency proxy rather than a perceptual codec metric.

| Signal | Transition | Max discontinuity | RMS error | Artifact energy |
| --- | --- | ---: | ---: | ---: |
| sine | raw | 0.098017 | 0.034428 | 0.00004837 |
| sine | linear | 0.073513 | 0.034023 | 0.00002954 |
| sine | crossfade 16 | 0.051891 | 0.030470 | 0.00001003 |
| noise | raw | 0.429244 | 0.180689 | 0.085421 |
| noise | linear | 0.429244 | 0.174707 | 0.081011 |
| noise | crossfade 16 | 0.429244 | 0.158216 | 0.066314 |

Crossfade materially improves the sine metrics and the noise RMS/energy, but it
does not reduce the noise fixture's worst discontinuity. Correction frequency
therefore remains an audible-risk signal even with transition smoothing.

## Realistic Clock Drift

The hardware-independent analytical simulation models 48 kHz, 128-frame
callbacks, target fill 512, deadband 128, and capacity 2,048. Positive and
negative ppm have symmetric correction counts and opposite fill direction.

| Absolute ppm | 10 min corrections | 1 h corrections | 8 h corrections | Average interval | 8 h bounded |
| ---: | ---: | ---: | ---: | ---: | --- |
| 10 | 160 | 1,600 | 13,696 | 2.083 s | yes |
| 25 | 592 | 4,192 | 34,432 | 0.833 s | yes |
| 50 | 1,312 | 8,512 | 68,992 | 0.417 s | yes |
| 100 | 2,752 | 17,152 | 138,112 | 0.208 s | yes |
| 250 | 7,072 | 43,072 | 345,472 | 0.083 s | yes |

No realistic case predicts underflow or overflow under this idealized scheduler,
but every listed mismatch exceeds the conservative one-correction-per-ten-second
audibility warning. A 20,000 ppm abrupt mismatch accumulates 2.56 frames per
block; one-frame-per-block correction cannot contain it and reports fatal.

The earlier one-frame-per-64-frame callback tests represent approximately
15,625 ppm and remain stress-only tests, not consumer-device drift estimates.

## Numeric Fault Policy

Callbacks publish health, minimum correction interval, maximum fill excursion,
consecutive underflows, consecutive overflows, and cumulative counters. They do
not log. Default thresholds are:

- warning below 480,000 output frames between corrections (10 seconds at 48 kHz)
- degraded below 48,000 frames (1 second at 48 kHz)
- fatal above 1,024 frames of target-fill excursion
- fatal after more than three consecutive underflows
- fatal after more than one consecutive overflow callback

The frame thresholds are numeric policy values; a control layer may convert them
using the active sample rate without touching callback code.

