use aurora_realtime_audio_sim::{
    simulate_head_tracker_delivery, HeadTrackerDeliveryEvent, HeadTrackerFaultProfile,
    HeadTrackerSimulationConfig,
};
use aurora_renderer_api::{
    HeadPoseClockAnchor, HeadPoseClockError, HeadPoseClockMapper, HeadPoseClockPolicy,
    HeadPoseError, HeadPosePolicy, HeadPoseState, TrackerPoseSample, UnitQuaternion,
};

#[derive(Debug, Default)]
struct Report {
    accepted: u64,
    dropped: u64,
    reconnects: u64,
    stale_failures: u64,
    clock_rejections: Vec<HeadPoseClockError>,
}

fn config() -> HeadTrackerSimulationConfig {
    HeadTrackerSimulationConfig::new(48_000, 100, 80, 0xA0_51_5A_17, 48).unwrap()
}

fn clock_policy() -> HeadPoseClockPolicy {
    // 100 Hz source => 10 ms nominal period. One lost/reordered sample creates a
    // 20 ms source gap, while a synthetic jump is intentionally larger than 25 ms.
    HeadPoseClockPolicy::new(48_000, 25_000_000, 96, 96).unwrap()
}

fn pose_policy() -> HeadPosePolicy {
    // 100 Hz at 48 kHz => 480 frames/sample. Two periods are allowed for
    // interpolation/hold; the stale-hold profile deliberately exceeds this.
    HeadPosePolicy::new(960, 960).unwrap()
}

fn evaluate(profile: HeadTrackerFaultProfile) -> Report {
    let mut mapper = HeadPoseClockMapper::new(
        clock_policy(),
        HeadPoseClockAnchor {
            source_timestamp_ns: 0,
            media_frame: 0,
        },
    );
    let mut poses = HeadPoseState::new(pose_policy());
    let mut report = Report::default();

    for event in simulate_head_tracker_delivery(config(), profile) {
        match event {
            HeadTrackerDeliveryEvent::Drop { .. } => report.dropped += 1,
            HeadTrackerDeliveryEvent::Reconnect { anchor } => {
                mapper.reset(HeadPoseClockAnchor {
                    source_timestamp_ns: anchor.source_timestamp_ns,
                    media_frame: anchor.media_frame,
                });
                // A reconnect is an explicit source epoch boundary. The downstream
                // two-sample freshness window is rebuilt rather than accepting a
                // restarted source sequence into the previous epoch.
                poses = HeadPoseState::new(pose_policy());
                report.reconnects += 1;
            }
            HeadTrackerDeliveryEvent::StaleProbe { media_frame } => {
                match poses.resolve(media_frame) {
                    Err(HeadPoseError::StalePose { .. }) => report.stale_failures += 1,
                    other => panic!("expected stale pose at frame {media_frame}, got {other:?}"),
                }
            }
            HeadTrackerDeliveryEvent::Sample(sample) => {
                let orientation = UnitQuaternion::try_new(
                    sample.orientation_wxyz[0],
                    sample.orientation_wxyz[1],
                    sample.orientation_wxyz[2],
                    sample.orientation_wxyz[3],
                )
                .unwrap();
                match mapper.map(
                    TrackerPoseSample {
                        sequence: sample.sequence,
                        source_timestamp_ns: sample.source_timestamp_ns,
                        orientation,
                    },
                    sample.observed_media_frame,
                ) {
                    Ok(mapped) => {
                        poses.commit(mapped).unwrap();
                        report.accepted += 1;
                    }
                    Err(error) => report.clock_rejections.push(error),
                }
            }
        }
    }

    report
}

#[test]
fn healthy_jitter_and_dropout_stay_within_declared_budgets() {
    for profile in [
        HeadTrackerFaultProfile::Healthy,
        HeadTrackerFaultProfile::Jitter,
        HeadTrackerFaultProfile::Dropout,
    ] {
        let report = evaluate(profile);
        assert!(report.clock_rejections.is_empty(), "{profile:?}: {report:?}");
        assert_eq!(report.reconnects, 0);
        assert_eq!(report.stale_failures, 0);
    }
    assert_eq!(evaluate(HeadTrackerFaultProfile::Dropout).dropped, 1);
}

#[test]
fn duplicate_and_reorder_fail_closed_transactionally() {
    let duplicate = evaluate(HeadTrackerFaultProfile::Duplicate);
    assert_eq!(duplicate.clock_rejections.len(), 1);
    assert!(matches!(
        duplicate.clock_rejections[0],
        HeadPoseClockError::NonMonotonicSequence { .. }
    ));

    let reorder = evaluate(HeadTrackerFaultProfile::Reorder);
    assert_eq!(reorder.clock_rejections.len(), 1);
    assert!(matches!(
        reorder.clock_rejections[0],
        HeadPoseClockError::NonMonotonicSequence { .. }
    ));
}

#[test]
fn timestamp_jump_is_rejected_without_poisoning_following_samples() {
    let report = evaluate(HeadTrackerFaultProfile::TimestampJump);
    assert_eq!(report.clock_rejections.len(), 1);
    assert!(matches!(
        report.clock_rejections[0],
        HeadPoseClockError::SourceGapTooLarge { .. }
    ));
    assert_eq!(report.accepted, u64::from(config().sample_count() - 1));
}

#[test]
fn stale_hold_expires_and_explicit_reanchor_recovers() {
    let report = evaluate(HeadTrackerFaultProfile::StaleHold);
    assert_eq!(report.dropped, 4);
    assert_eq!(report.stale_failures, 1);
    assert_eq!(report.reconnects, 1);
    assert!(report.clock_rejections.is_empty(), "{report:?}");
    assert!(report.accepted > 0);
}

#[test]
fn reconnect_starts_a_new_source_epoch_without_cross_epoch_freshness() {
    let report = evaluate(HeadTrackerFaultProfile::Reconnect);
    assert_eq!(report.reconnects, 1);
    assert!(report.clock_rejections.is_empty(), "{report:?}");
    assert_eq!(report.accepted, u64::from(config().sample_count()));
}
