use aurora_renderer_basic::binaural::{Error, Filters, Input, PreparedBinaural};
use aurora_test_alloc::{count_allocations, CountingAllocator};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn filters(generation: u64, left: f32, right: f32) -> Filters {
    Filters::prepare(Input::Objects(1), 48_000, generation, 1, vec![left, right]).unwrap()
}

#[test]
fn prepared_binaural_callback_transition_fault_and_recovery_allocate_zero_times() {
    let mut renderer = PreparedBinaural::new(filters(1, 0.5, 0.25), 8).unwrap();
    let candidate = filters(2, 0.25, 0.5);
    let input = [0.5; 8];
    let invalid = [f32::NAN; 8];
    let mut output = [0.0; 16];
    let mut okay = true;

    let allocations = count_allocations(|| {
        okay &= renderer.commit(&candidate, 80).is_ok();
        for _ in 0..10_000 {
            okay &= renderer.process(&input, &mut output).is_ok();
        }
        okay &= renderer.process(&invalid, &mut output) == Err(Error::Numeric);
        renderer.discontinuity();
        okay &= renderer.process(&input, &mut output).is_ok();
    });

    assert!(okay);
    assert_eq!(allocations, 0);
    assert!(output.iter().all(|sample| sample.is_finite()));
}

#[test]
fn rejected_filter_commit_does_not_allocate_or_mutate_generation() {
    let mut renderer = PreparedBinaural::new(filters(4, 0.5, 0.25), 8).unwrap();
    let stale = filters(4, 0.25, 0.5);
    let mut result = Ok(());

    let allocations = count_allocations(|| {
        result = renderer.commit(&stale, 80);
    });

    assert_eq!(result, Err(Error::Generation));
    assert_eq!(renderer.generation(), 4);
    assert_eq!(allocations, 0);
}

#[test]
fn head_pose_selected_filters_commit_and_render_without_allocation() {
    use aurora_core::Vector3;
    use aurora_renderer_api::{HeadPosePolicy, HeadPoseSample, HeadPoseState, UnitQuaternion};
    use aurora_renderer_basic::binaural::hrtf::{DirectionalHrtf, SofaMeasurement};

    let bank = DirectionalHrtf::prepare(
        48_000,
        1,
        0.1,
        vec![
            SofaMeasurement {
                direction: Vector3::new(1.0, 0.0, 0.0),
                coefficients: vec![0.5, 0.5],
            },
            SofaMeasurement {
                direction: Vector3::new(0.0, -1.0, 0.0),
                coefficients: vec![0.25, 0.75],
            },
        ],
    )
    .unwrap();
    let mut poses = HeadPoseState::new(HeadPosePolicy::new(100, 10).unwrap());
    poses
        .commit(HeadPoseSample {
            sequence: 1,
            media_frame: 100,
            orientation: UnitQuaternion::IDENTITY,
        })
        .unwrap();
    let front = [Vector3::new(0.0, 1.0, 0.0)];
    let initial = bank.prepare_objects(&poses, 100, &front, 1).unwrap();
    let half = std::f32::consts::FRAC_PI_4;
    poses
        .commit(HeadPoseSample {
            sequence: 2,
            media_frame: 200,
            orientation: UnitQuaternion::try_new(half.cos(), 0.0, 0.0, half.sin()).unwrap(),
        })
        .unwrap();
    let candidate = bank.prepare_objects(&poses, 200, &front, 2).unwrap();
    let mut renderer = PreparedBinaural::new(initial, 8).unwrap();
    let input = [1.0; 8];
    let mut output = [0.0; 16];
    let mut okay = true;
    let allocations = count_allocations(|| {
        okay &= renderer.commit(&candidate, 80).is_ok();
        for _ in 0..10_000 {
            okay &= renderer.process(&input, &mut output).is_ok();
        }
        okay &= renderer.commit(&candidate, 80) == Err(Error::Generation);
    });
    assert!(okay);
    assert_eq!(allocations, 0);
    for pair in output.chunks_exact(2) {
        assert_eq!(pair, &[0.25, 0.75]);
    }
}
