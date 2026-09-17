# Head-pose tracker clock mapping

`aurora-renderer-api::HeadPoseClockMapper` is the hardware-neutral boundary between a tracker/source clock and Aurora's logical media-frame timeline.

## Explicit epoch anchor

Aurora does not infer that a tracker timestamp is wall clock, PTP, monotonic host time, or audio time. Control code must provide one explicit `HeadPoseClockAnchor { source_timestamp_ns, media_frame }` for the current mapping epoch. Every accepted tracker timestamp is converted relative to that anchor at the configured Aurora sample rate.

The conversion is integer-only: source nanoseconds are multiplied by sample rate in `u128`, rounded to the nearest logical frame with half-way values rounded forward, then added to the anchor frame. This avoids cumulative floating-point drift.

## Fail-closed delivery contract

Within one epoch:

- source sequence must strictly increase;
- source timestamps must strictly increase;
- a sample may not predate the anchor;
- consecutive source timestamp gaps are bounded;
- mapped Aurora frames must strictly increase;
- delivery lag against the caller-supplied observed Aurora frame is bounded;
- future lead against that observed frame is bounded;
- overflow fails instead of wrapping;
- rejected samples do not advance mapper state.

A reconnect, source-clock reset, timestamp jump, or any invalidated clock relationship requires an explicit `reset(new_anchor)`. Rollback is never silently interpreted as a new epoch.

## Ownership and realtime boundary

The mapper performs no device I/O, locking, allocation, logging, filesystem, process, or network work. It is intended for control/worker code. A platform adapter is responsible for reading a physical tracker's native timestamp, sequence and orientation, establishing the epoch anchor, and supplying the current Aurora logical media frame at delivery.

A successful `TrackerPoseSample` mapping produces the existing `HeadPoseSample`, which can be committed into `HeadPoseState` or `HeadPoseHrtfScheduler`. The mapper does not itself claim a physical tracker, measured sensor latency, OS scheduling bound, or perceptual HRTF quality.

## Remaining validation

The next gate is deterministic continuous-delivery simulation covering nominal cadence plus jitter, duplicate/reorder, burst loss/source gap, delayed delivery, excessive future lead, clock reset and reconnect. Physical latency/jitter must later be measured with a real selected tracker; software policy bounds are not physical measurements.
