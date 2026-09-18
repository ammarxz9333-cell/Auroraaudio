use aurora_core::Vector3;
use aurora_realtime_engine::{
    create_head_pose_delivery_bridge, HeadPoseControlEvent, HeadPoseIngressPushError,
};
use aurora_renderer_api::{
    HeadPoseClockAnchor, HeadPoseClockPolicy, HeadPosePolicy, TrackerPoseSample, UnitQuaternion,
};
use aurora_renderer_basic::binaural::hrtf::scheduler::{HeadPoseHrtfScheduler, ObjectDirection};
use aurora_renderer_basic::binaural::hrtf::{DirectionalHrtf, SofaMeasurement};
use aurora_renderer_basic::binaural::{Filters, Input, PreparedBinaural};

fn sample(sequence: u64, source_timestamp_ns: u64) -> TrackerPoseSample {
    TrackerPoseSample {
        sequence,
        source_timestamp_ns,
        orientation: UnitQuaternion::IDENTITY,
    }
}

fn scheduler() -> HeadPoseHrtfScheduler {
    let bank = DirectionalHrtf::prepare(
        48_000,
        1,
        0.1,
        vec![SofaMeasurement {
            direction: Vector3::new(1.0, 0.0, 0.0),
            coefficients: vec![1.0, 0.0],
        }],
    )
    .unwrap();
    HeadPoseHrtfScheduler::new(
        bank,
        HeadPosePolicy::new(600, 600).unwrap(),
        vec!["dialogue".to_owned()],
        1,
    )
    .unwrap()
}

fn renderer() -> PreparedBinaural {
    let initial = Filters::prepare(Input::Objects(1), 48_000, 1, 1, vec![0.5, 0.5]).unwrap();
    PreparedBinaural::new(initial, 8).unwrap()
}

#[test]
fn bounded_ingress_reanchor_drives_scheduler_without_resetting_media_time() {
    let initial_anchor = HeadPoseClockAnchor {
        source_timestamp_ns: 1_000_000_000,
        media_frame: 48_000,
    };
    let policy = HeadPoseClockPolicy::new(48_000, 20_000_000, 240, 48).unwrap();
    let (producer, mut control) =
        create_head_pose_delivery_bridge(policy, initial_anchor, 8).unwrap();
    let mut scheduler = scheduler();
    let mut renderer = renderer();
    let objects = [ObjectDirection {
        id: "dialogue",
        world_direction: Vector3::new(0.0, 1.0, 0.0),
    }];

    producer
        .try_push_sample(sample(100, 1_000_000_000))
        .unwrap();
    let first = match control.try_poll(48_000).unwrap() {
        HeadPoseControlEvent::Mapped(mapped) => mapped,
        other => panic!("unexpected first event: {other:?}"),
    };
    scheduler.commit_pose(first).unwrap();
    let first_generation = scheduler.prepare_at(first.media_frame, &objects).unwrap();
    scheduler
        .commit_at_boundary(&mut renderer, first.media_frame, 1)
        .unwrap();
    scheduler.release_committed(first_generation).unwrap();

    producer.try_push_disconnected().unwrap();
    let next_anchor = HeadPoseClockAnchor {
        source_timestamp_ns: 10_000,
        media_frame: 50_000,
    };
    producer.try_push_reanchor(next_anchor).unwrap();
    producer.try_push_sample(sample(1, 10_000)).unwrap();

    assert_eq!(
        control.try_poll(49_000),
        Some(HeadPoseControlEvent::Disconnected)
    );
    assert_eq!(
        control.try_poll(50_000),
        Some(HeadPoseControlEvent::Reanchored(next_anchor))
    );
    scheduler.reset_pose_epoch().unwrap();

    let restarted = match control.try_poll(50_000).unwrap() {
        HeadPoseControlEvent::Mapped(mapped) => mapped,
        other => panic!("unexpected restarted event: {other:?}"),
    };
    assert_eq!(restarted.sequence, 1);
    assert_eq!(restarted.media_frame, 50_000);

    scheduler.commit_pose(restarted).unwrap();
    let second_generation = scheduler
        .prepare_at(restarted.media_frame, &objects)
        .unwrap();
    assert!(second_generation > first_generation);
    scheduler
        .commit_at_boundary(&mut renderer, restarted.media_frame, 1)
        .unwrap();
    scheduler.release_committed(second_generation).unwrap();
}

#[test]
fn full_queue_is_explicit_and_recoverable_at_adapter_boundary() {
    let policy = HeadPoseClockPolicy::new(48_000, 20_000_000, 240, 48).unwrap();
    let anchor = HeadPoseClockAnchor {
        source_timestamp_ns: 0,
        media_frame: 0,
    };
    let (producer, mut control) = create_head_pose_delivery_bridge(policy, anchor, 2).unwrap();

    producer.try_push_sample(sample(1, 0)).unwrap();
    producer.try_push_disconnected().unwrap();
    assert_eq!(
        producer.try_push_reanchor(HeadPoseClockAnchor {
            source_timestamp_ns: 10,
            media_frame: 100,
        }),
        Err(HeadPoseIngressPushError::Overflow)
    );

    assert!(matches!(
        control.try_poll(0),
        Some(HeadPoseControlEvent::Mapped(_))
    ));
    assert_eq!(
        control.try_poll(0),
        Some(HeadPoseControlEvent::Disconnected)
    );

    let next_anchor = HeadPoseClockAnchor {
        source_timestamp_ns: 10,
        media_frame: 100,
    };
    producer.try_push_reanchor(next_anchor).unwrap();
    assert_eq!(
        control.try_poll(100),
        Some(HeadPoseControlEvent::Reanchored(next_anchor))
    );
}
