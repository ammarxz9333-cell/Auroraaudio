use aurora_core::Vector3;
use aurora_renderer_api::{HeadPosePolicy, HeadPoseSample, UnitQuaternion};
use aurora_renderer_basic::binaural::hrtf::scheduler::{HeadPoseHrtfScheduler, ObjectDirection};
use aurora_renderer_basic::binaural::hrtf::{DirectionalHrtf, SofaMeasurement};
use aurora_renderer_basic::binaural::{Filters, Input, PreparedBinaural};
use aurora_test_alloc::{count_allocations, CountingAllocator};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn scheduled_boundary_commit_and_realtime_processing_allocate_zero_times() {
    let bank = DirectionalHrtf::prepare(
        48_000,
        1,
        0.1,
        vec![
            SofaMeasurement {
                direction: Vector3::new(1.0, 0.0, 0.0),
                coefficients: vec![1.0, 0.0],
            },
            SofaMeasurement {
                direction: Vector3::new(0.0, -1.0, 0.0),
                coefficients: vec![0.0, 1.0],
            },
        ],
    )
    .unwrap();
    let mut scheduler = HeadPoseHrtfScheduler::new(
        bank,
        HeadPosePolicy::new(100, 10).unwrap(),
        vec!["dialogue".to_owned(), "effect".to_owned()],
        1,
    )
    .unwrap();
    scheduler
        .commit_pose(HeadPoseSample {
            sequence: 1,
            media_frame: 100,
            orientation: UnitQuaternion::IDENTITY,
        })
        .unwrap();
    let objects = [
        ObjectDirection {
            id: "dialogue",
            world_direction: Vector3::new(0.0, 1.0, 0.0),
        },
        ObjectDirection {
            id: "effect",
            world_direction: Vector3::new(1.0, 0.0, 0.0),
        },
    ];
    let generation = scheduler.prepare_at(100, &objects).unwrap();

    let initial =
        Filters::prepare(Input::Objects(2), 48_000, 1, 1, vec![0.5, 0.5, 0.5, 0.5]).unwrap();
    let mut renderer = PreparedBinaural::new(initial, 8).unwrap();
    let input = [0.5; 16];
    let mut output = [0.0; 16];
    let mut okay = true;

    let allocations = count_allocations(|| {
        okay &= scheduler.commit_at_boundary(&mut renderer, 100, 80).is_ok();
        for _ in 0..10_000 {
            okay &= renderer.process(&input, &mut output).is_ok();
        }
    });

    assert!(okay);
    assert_eq!(allocations, 0);
    assert_eq!(renderer.generation(), generation);
    assert!(output.iter().all(|sample| sample.is_finite()));

    // Candidate destruction is deliberately outside the measured realtime boundary.
    scheduler.release_committed(generation).unwrap();
}
