# Head-pose HRTF scheduler

`binaural::hrtf::scheduler::HeadPoseHrtfScheduler` closes the control-plane gap between mapped tracker samples and the prepared pose-selected FIR path.

## Contract

The scheduler owns:

- the bounded two-sample `HeadPoseState` timeline;
- the prepared `DirectionalHrtf` bank;
- one immutable configured list of 1–16 stable object identifiers in PCM channel order;
- monotonic filter generations;
- at most one prepared candidate and its exact target media frame;
- candidate lifetime until explicit control-thread release or cancellation.

Object directions may move between snapshots. Object count, identity and ordering may not change silently; such changes require explicit scheduler/renderer reconfiguration. Duplicate or empty configured IDs are rejected.

## Prepare -> boundary commit -> release

1. Tracker/device-specific code maps a sample onto Aurora's logical media-frame timeline and calls `commit_pose` on the control thread.
2. Control code calls `prepare_at(target_frame, objects)`. The exact object IDs/order are validated, the bounded pose is resolved for that frame, world directions are transformed into head-local directions, and a complete next-generation FIR candidate is allocated/prepared transactionally.
3. At the exclusive block boundary whose logical frame is exactly `target_frame`, call `commit_at_boundary`. The scheduler only borrows the candidate into `PreparedBinaural::commit`; it does not allocate, release or replace candidate storage there.
4. After the boundary operation returns, call `release_committed(generation)` on the control thread. This is deliberately separate because dropping the owned `Filters` may deallocate.

A wrong boundary frame, stale/missing pose, object reorder, duplicate/missing identity, uncovered HRTF direction, stale generation, active renderer transition, or malformed candidate fails closed. Failure before renderer acceptance does not mutate the active renderer.

Only one candidate may be outstanding. This keeps ownership explicit and bounded. An uncommitted candidate may be removed with `cancel_pending`; a committed candidate must use `release_committed`.

## Realtime evidence

`crates/aurora-realtime-engine/tests/head_pose_hrtf_scheduler_allocation.rs` prepares the pose/HRTF candidate before measurement, then measures the exact boundary commit followed by 10,000 render blocks. The candidate is released only after the measured realtime region. This validates the Aurora-owned zero-allocation boundary/process path; it does not prove a tracker driver, OS scheduling, physical latency or perceptual HRTF quality.

## Remaining boundary

This scheduler does not ingest a physical tracker or define a tracker clock. A platform adapter still has to map device timestamps/poses onto Aurora media frames and deliver samples with bounded latency/jitter. AuroraSim coverage for continuous tracker delivery and physical/perceptual validation remain separate gates.
