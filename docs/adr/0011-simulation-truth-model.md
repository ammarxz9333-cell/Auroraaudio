# ADR 0011: Separate Simulation Truth From Measurement

## Decision

Virtual-cable delay, routing, gain, polarity, and noise are explicit truth
inputs. Reports use `simulated_*_truth` source labels. Only accepted physical
capture may use measured-latency terminology.

## Consequences

Estimator accuracy can be tested objectively while physical validation remains
an explicit later step.
