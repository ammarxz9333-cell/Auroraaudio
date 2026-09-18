# Bounded head-pose delivery bridge

Aurora separates physical tracker/device I/O from clock mapping and HRTF scheduling.

## Thread boundary

The bridge created by `create_head_pose_delivery_bridge` has two owners:

- `HeadPoseIngressProducer`: moved to a tracker/device adapter thread.
- `HeadPoseDeliveryControl`: retained by Aurora control code.

Construction preallocates a fixed `ArrayQueue`. The supported capacity is 2 through 1024
events. The producer publishes only fixed-size events and never blocks or allocates after
construction.

The public producer is intentionally not cloneable, preserving one logical event ordering
source even though the underlying queue implementation is thread-safe.

## Events

Ingress accepts:

- raw `TrackerPoseSample`;
- explicit `Disconnected`;
- explicit `Reanchor(HeadPoseClockAnchor)`.

The control side processes at most one event per `try_poll` call. There is deliberately no
unbounded drain helper. A control loop that wants more throughput must choose an explicit
maximum poll count per tick.

For a sample, the caller supplies the Aurora logical media frame observed at dequeue time.
The existing `HeadPoseClockMapper` then enforces sequence/timestamp monotonicity, source-gap
limits and delivery lag/lead limits transactionally.

## Overflow and reconnect

Queue overflow rejects the new event without mutating mapper state or discarding events
already accepted by the queue.

A successful disconnect makes the control side reject subsequent samples without touching
mapper history. Recovery requires an explicit re-anchor. When the control side emits
`Reanchored`, a binaural scheduler must also call `reset_pose_epoch` before accepting a
tracker whose source sequence restarted.

If a disconnect marker was accepted but a following re-anchor could not be queued because
the bridge was full, Aurora remains disconnected. The adapter can retry the re-anchor later;
there is no inferred reconnect.

## Realtime boundary

This bridge is not called by the audio callback. Tracker I/O, Bluetooth/USB/network access,
device SDK calls, calibration and timestamp acquisition remain adapter-thread work.
Clock mapping and scheduler publication remain control-thread work. The audio callback only
consumes already prepared renderer state at exact Aurora block boundaries.

Unit tests verify that producer publication plus one-event control polling allocate zero
times after construction.

## Truth boundary

The bridge proves a bounded, nonblocking software transport between a future adapter thread
and Aurora control code. It does not prove any physical tracker, operating-system scheduling
latency, radio/USB behavior, timestamp accuracy, sensor calibration, personalized HRTF
quality, perception or measured motion-to-sound latency.
