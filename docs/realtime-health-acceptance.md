# Realtime health acceptance

Aurora collects realtime metrics in `aurora-realtime-engine` without allocating diagnostic reports in the audio callback. The separate `aurora-realtime-acceptance` crate evaluates an already-collected metrics snapshot on the control/validation side.

The default policy is deliberately strict:

- at least one callback must have run;
- the persistent realtime fault must be `None`;
- input underruns: 0;
- output underruns: 0;
- dropped blocks: 0;
- p95 callback duration must not exceed 100% of the configured block-duration budget.

A caller may additionally set a maximum estimated end-to-end latency in frames. That latency remains a software estimate assembled from renderer, DSP, and configured device-buffer estimates; it is not a physical measurement.

The acceptance report returns the observed p95 budget use plus every violated condition instead of collapsing failures into one boolean. Policy evaluation is intentionally outside the audio callback and may allocate diagnostics.

Tests cover both deterministic synthetic metric snapshots and metrics emitted by a real `RealTimeEngine` run. A clean engine run must pass the strict policy, while a malformed callback must be rejected with its persistent fault, output underrun, and dropped-block violations.

This gate is hardware-agnostic. Passing it does not prove a particular host, audio interface, transport, DAC, amplifier, speaker system, or end-to-end physical latency.
