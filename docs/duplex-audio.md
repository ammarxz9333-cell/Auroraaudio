# Duplex Audio Bridge

The deterministic simulator schedules input and output callbacks on independent
integer-tick clocks and applies the same Aurora drift-controller contract to a
fixed-capacity ring model. Accelerated runs require no sleeps. Callback-size and
sample-path behavior are guarded separately by realtime-engine tests so long-run
validation does not pretend to be physical callback timing.

## Ownership

`create_duplex_bridge` allocates one bounded contiguous SPSC frame ring and fixed
status storage before streams start. `DuplexProducer` belongs to the input callback.
`DuplexConsumer` belongs to the output callback and owns one fixed previous-frame
buffer. `DuplexStatus` is shared with the control thread through atomics.

The input callback copies one borrowed interleaved slice into contiguous
preallocated storage and publishes one index. The output callback copies directly
into its caller-owned buffer and publishes one index in the normal path. No
callback creates, clones, resizes, or transfers an owned audio block.
The queue is bounded; overflow rejects incoming frames and underflow writes
silence.

## Clock Drift Strategy

Input and output devices have independent clocks. The proof-of-concept elastic
buffer tracks projected fill after the next output block:

- Above `target_fill + correction_threshold`, remove one complete input frame
  through the configured bounded transition.
- Below `target_fill - correction_threshold`, interpolate one additional complete
  frame when enough queued data remains.
- Apply at most one correction frame per output callback.
- Never grow the queue or block either callback.

Counters expose input/output frames, current/minimum/maximum fill, signed trend,
inserted/removed slips, overflow, underflow, consecutive fault counts, minimum
correction interval, maximum excursion, numeric health, and shape faults.

`DriftCompensator` selects the strategy without coupling it to the transport.
See `docs/duplex-transport.md`, `docs/drift-compensation.md`, and ADR 0005.

## Audible-Risk Thresholds

Sample slip is not asynchronous sample-rate conversion. A removed frame can make
a full-scale discontinuity; an inserted frame can flatten one sample interval.
At 48 kHz each correction is 20.8 microseconds, but it can still click on
high-frequency or high-amplitude content.

For hardware evaluation, use a target of at least four processing blocks and a
correction threshold of at least one block. Treat more than one slip per ten
seconds, any correction adjacent to a sample step above 0.001 full scale
(-60 dBFS), repeated underflow, or any overflow as audible risk requiring review.
These are warning thresholds, not a production guarantee.

Production duplex requires a measured, band-limited asynchronous sample-rate
converter with clock estimation. Aurora's legacy sample-slip strategy is only a
bounded proof of concept for validating ownership, scheduling, and observability.

Sprint 2B normal live operation now uses the band-limited adaptive ASRC described
in `docs/asynchronous-resampling.md`. Sample slip remains only a fallback and test
reference. The selected ring and zero-allocation ownership model are unchanged.
