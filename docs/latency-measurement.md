# Latency Measurement Contract

Aurora's hardware-independent estimator correlates a deterministic bipolar
sequence against captured samples at repeated expected emission positions. It
reports median/minimum/maximum offsets, jitter, confidence, and valid count.
Silent captures and correlations below the configured confidence threshold are
rejected.

Synthetic delayed-signal tests validate estimator mathematics only. Their
results must be labeled synthetic offsets, never measured latency.

A future physical command must open real input and output streams, emit the
reference, capture input, and use a physical cable from an output to an input.
Only a successful non-silent capture with accepted correlation may be labeled
measured round-trip latency. Device estimates, callback periods, timestamps, and
synthetic tests are not substitutes.

The physical `measure-latency` command and simulated `simulate-latency` command
are separate. Simulated results carry `simulated_virtual_loopback_truth`; only
the physical command can emit `measurement_source=physical_captured_loopback`
after accepted non-silent input capture.
