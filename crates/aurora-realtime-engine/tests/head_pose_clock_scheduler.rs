use aurora_core::Vector3;
use aurora_renderer_api::{
    HeadPoseClockAnchor, HeadPoseClockMapper, HeadPoseClockPolicy, TrackerPoseSample,
    UnitQuaternion,
};
use aurora_renderer_basic::binaural::hrtf::scheduler::{HeadPoseHrtfScheduler, ObjectDirection};
use aurora_renderer_basic::binaural::hrtf::{DirectionalHrtf, SofaMeasurement};
use aurora_renderer_basic::binaural::{Filters, Input, PreparedBinaural};

#[test]
fn mapped_tracker_pose_drives_exact_scheduler_boundary() {
    let clock_policy = HeadPoseClockPolicy::new(48_000, 20_000_000, 240, 48).unwrap();
    let mut mapper = HeadPoseClockMapper::new(
        clock_policy,
        HeadPoseClockAnchor {
            source_timestamp_ns: 1_000_000_000,
            media_frame: 100,
        },
    );
    let mapped = mapper
        .map(
            TrackerPoseSample {
                sequence: 1,
                source_timestamp_ns: 1_000_000_000,
                orientation: UnitQuaternion::IDENTITY,
            },
            100,
        )
        .unwrap();
    assert_eq!(mapped.media_frame, 100);

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
    let mut scheduler = HeadPoseHrtfScheduler::new(
        bank,
        aurora_renderer_api::HeadPosePolicy::new(100, 10).unwrap(),
        vec!["dialogue".to_owned()],
        1,
    )
    .unwrap();
    scheduler.commit_pose(mapped).unwrap();
    let generation = scheduler
        .prepare_at(
            100,
            &[ObjectDirection {
                id: "dialogue",
                world_direction: Vector3::new(0.0, 1.0, 0.0),
            }],
        )
        .unwrap();

    let initial = Filters::prepare(Input::Objects(1), 48_000, 1, 1, vec![0.5, 0.5]).unwrap();
    let mut renderer = PreparedBinaural::new(initial, 8).unwrap();
    assert_eq!(
        scheduler.commit_at_boundary(&mut renderer, 99, 1),
        Err(aurora_renderer_basic::binaural::hrtf::scheduler::SchedulerError::BoundaryFrame {
            expected: 100,
            actual: 99,
        })
    );
    scheduler
        .commit_at_boundary(&mut renderer, 100, 1)
        .unwrap();
    assert_eq!(renderer.generation(), generation);

    let mut output = [0.0; 16];
    renderer.process(&[1.0; 8], &mut output).unwrap();
    for pair in output.chunks_exact(2) {
        assert_eq!(pair, &[1.0, 0.0]);
    }
    scheduler.release_committed(generation).unwrap();
}

#[test]
fn rejected_clock_epoch_does_not_poison_reanchor_and_delivery() {
    let policy = HeadPoseClockPolicy::new(48_000, 20_000_000, 240, 48).unwrap();
    let mut mapper = HeadPoseClockMapper::new(
        policy,
        HeadPoseClockAnchor {
            source_timestamp_ns: 1_000_000,
            media_frame: 100,
        },
    );
    mapper
        .map(
            TrackerPoseSample {
                sequence: 50,
                source_timestamp_ns: 2_000_000,
                orientation: UnitQuaternion::IDENTITY,
            },
            148,
        )
        .unwrap();
    assert!(mapper
        .map(
            TrackerPoseSample {
                sequence: 1,
                source_timestamp_ns: 10_000,
                orientation: UnitQuaternion::IDENTITY,
            },
            200,
        )
        .is_err());

    mapper.reset(HeadPoseClockAnchor {
        source_timestamp_ns: 10_000,
        media_frame: 200,
    });
    let mapped = mapper
        .map(
            TrackerPoseSample {
                sequence: 1,
                source_timestamp_ns: 10_000,
                orientation: UnitQuaternion::IDENTITY,
            },
            200,
        )
        .unwrap();
    assert_eq!(mapped.sequence, 1);
    assert_eq!(mapped.media_frame, 200);
}
