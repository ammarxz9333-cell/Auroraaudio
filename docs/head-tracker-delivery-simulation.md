# Head-tracker delivery simulation

AuroraSim now has a hardware-neutral deterministic head-tracker delivery model. It produces source sequence/timestamp samples plus control-side delivery timing and explicit fault events; it does not own renderer state or a physical tracker API.

The standard profiles are `healthy`, `jitter`, `dropout`, `duplicate`, `reorder`, `stale-hold`, `timestamp-jump`, and `reconnect`. A fixed seed produces the same event timeline. Jitter is bounded in Aurora logical frames. Drop/duplicate/reorder are explicit events, timestamp jumps preserve the prior accepted mapper state, and reconnect is an explicit new source-clock epoch with a new source-clock/Aurora-frame anchor.

`crates/aurora-simulation-assurance/tests/head_tracker_delivery.rs` feeds the generated timeline through `HeadPoseClockMapper` and `HeadPoseState`. The assurance lane proves bounded healthy/jitter/dropout behavior, fail-closed duplicate/reorder/timestamp discontinuities, stale-hold expiry, and explicit reconnect/re-anchor recovery. Existing scheduler/HRTF tests separately prove exact boundary commit, prepared-filter lifetime and zero-allocation realtime processing.

This is software/virtual evidence only. It does not measure sensor latency, transport jitter, timestamp quality, physical tracking accuracy, HRTF personalization, perceptual quality, device reconnect behavior, or end-to-end acoustic latency.
