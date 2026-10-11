# Head-tracker delivery simulation

AuroraSim now has a deterministic software-only head-tracker delivery model for the
hardware-neutral head-pose clock mapper and HRTF scheduling path.

## Scope

The simulator generates source sequence/timestamp samples from an explicit
`HeadPoseClockAnchor`, applies bounded delivery timing, and emits ordered control-plane
events. The built-in profiles are:

- `head-tracker-healthy`
- `head-tracker-jitter`
- `head-tracker-burst-loss`
- `head-tracker-reorder`
- `head-tracker-duplicate`
- `head-tracker-timestamp-jump`
- `head-tracker-stale-hold`
- `head-tracker-reconnect`

Jitter is seeded and deterministic. Drop, reorder, duplicate, timestamp-step and stale
delivery faults are explicit rather than inferred from wall-clock time.

## Mapping and recovery contract

`HeadPoseClockMapper` remains transactional. Invalid samples do not advance its accepted
sequence, source timestamp or mapped media frame. A timestamp jump or restarted source
sequence is not treated as a new epoch automatically.

Reconnect is explicit:

1. AuroraSim emits `Disconnected`.
2. Control code receives a new `Reanchor`.
3. The clock mapper is reset to that anchor.
4. `HeadPoseHrtfScheduler::reset_pose_epoch` clears only the two-sample tracker pose
   window.
5. Aurora logical media time, scheduler boundary history and filter generation continue.

A pending HRTF candidate blocks pose-epoch reset until it is committed/released or
cancelled on the control thread. Candidate destruction is still kept away from the
realtime boundary.

## End-to-end executable evidence

`crates/aurora-realtime-audio-sim/tests/head_tracker_hrtf.rs` drives the reconnect trace
through:

`tracker trace -> HeadPoseClockMapper -> HeadPoseHrtfScheduler -> DirectionalHrtf ->
PreparedBinaural commit/crossfade -> PCM processing`

The test uses exact Aurora media-frame boundaries and explicit mapper/scheduler epoch
reset. The simulator module separately exercises healthy jitter, burst loss, reorder,
duplicate, timestamp jump and stale delivery behavior.

## Truth boundary

This is deterministic software/reference evidence. It does not prove:

- a physical tracker or its timestamp quality;
- sensor fusion, calibration or absolute orientation accuracy;
- USB/Bluetooth/radio transport latency or loss behavior;
- operating-system scheduling latency;
- personalized HRTF quality or perception;
- acoustic output, headphones or measured motion-to-sound latency.

Physical tracker and perceptual acceptance remain separate gates.
