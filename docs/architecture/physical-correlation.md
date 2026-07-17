# Physical Correlation Framework

## Status

- `authorization_state`: `PROPOSED`
- `execution_state`: `NOT_STARTED`
- physical evidence available: none

## Purpose

This framework defines how Aurora shall compare simulated behavior with later accepted hardware and acoustic measurements. Its purpose is to quantify model error, calibrate bounded parameters, and identify where simulation is or is not predictive.

Physical correlation does not retroactively convert simulated results into physical evidence.

## Measurement prerequisites

A correlation dataset shall be accepted only when it records:

- hardware identities and revisions;
- firmware, software, configuration, and repository revisions;
- room geometry and relevant environmental conditions;
- microphone, interface, clock, and calibration-chain provenance;
- speaker and amplifier configuration;
- test signal, level, sample rate, and acquisition settings;
- repeated measurements sufficient to estimate variability;
- raw-data hashes and an immutable processing recipe.

Uncalibrated or incomplete measurements may be retained for exploration but must not satisfy acceptance gates.

## Correlation domains

The project should compare, where meaningful:

- end-to-end and per-stage latency;
- clock drift, synchronization error, and recovery time;
- buffer behavior, underflow, and discontinuity rates;
- transfer functions and impulse responses;
- propagation and reflection arrival times;
- frequency response and seat-to-seat variation;
- directivity and placement sensitivity;
- modeled distortion or noise only when the measurement chain supports valid comparison;
- compute, thermal, memory, and network behavior.

## Analysis requirements

Each comparison shall include:

- simulation and measurement versions;
- aligned units, bandwidth, time window, and preprocessing;
- absolute and normalized error metrics;
- confidence intervals or repeatability bounds;
- residual plots or equivalent diagnostics;
- model-validity domain and known confounders;
- classification as `CORRELATED`, `PARTIALLY_CORRELATED`, `NOT_CORRELATED`, or `INCONCLUSIVE`.

A good aggregate score must not conceal a critical local mismatch.

## Calibration policy

Model parameters may be updated only through reviewed, versioned calibration records. The calibration set and validation set must be separated. Improvements must be demonstrated on held-out physical data and must not degrade established deterministic fixtures.

Historical simulation reports remain immutable and retain the model version originally used.

## Exit criteria for hardware decisions

A simulator-informed hardware or architecture decision may be promoted only when:

- relevant deterministic software gates pass;
- model uncertainty is documented;
- required physical correlation domains meet approved thresholds;
- unresolved mismatches are explicitly accepted or corrected;
- the resulting decision records both simulated and physical evidence separately.

## Prohibited claims

- claiming measured SPL, distortion, localization, or room response without accepted measurements;
- using headphone binaural preview as physical loudspeaker validation;
- calibrating and validating against the same dataset without disclosure;
- discarding adverse measurements without a traceable technical reason;
- changing model parameters to force agreement without documenting the change.
