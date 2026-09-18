use aurora_core::Vector3;
use aurora_renderer_api::{HeadPosePolicy, HeadPoseSample, HeadPoseState, UnitQuaternion};
use aurora_renderer_basic::binaural::{
    hrtf::{sofa_to_aurora_direction, DirectionalHrtf, PreparationError, SofaMeasurement},
    Error, PreparedBinaural,
};

fn measurement(direction: Vector3, left: f32, right: f32) -> SofaMeasurement {
    SofaMeasurement {
        direction,
        coefficients: vec![left, right],
    }
}

fn bank() -> DirectionalHrtf {
    DirectionalHrtf::prepare(
        48_000,
        1,
        0.1,
        vec![
            measurement(Vector3::new(1.0, 0.0, 0.0), 0.5, 0.5),
            measurement(Vector3::new(0.0, -1.0, 0.0), 0.25, 0.75),
            measurement(Vector3::new(0.0, 1.0, 0.0), 0.75, 0.25),
        ],
    )
    .unwrap()
}

fn poses() -> HeadPoseState {
    let mut poses = HeadPoseState::new(HeadPosePolicy::new(100, 10).unwrap());
    poses
        .commit(HeadPoseSample {
            sequence: 1,
            media_frame: 100,
            orientation: UnitQuaternion::IDENTITY,
        })
        .unwrap();
    let half = std::f32::consts::FRAC_PI_4;
    poses
        .commit(HeadPoseSample {
            sequence: 2,
            media_frame: 200,
            orientation: UnitQuaternion::try_new(half.cos(), 0.0, 0.0, half.sin()).unwrap(),
        })
        .unwrap();
    poses
}

#[test]
fn sofa_axes_map_front_left_right_up_without_mirroring() {
    for (sofa, aurora) in [
        (Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        (Vector3::new(0.0, 1.0, 0.0), Vector3::new(-1.0, 0.0, 0.0)),
        (Vector3::new(0.0, -1.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        (Vector3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 0.0, 1.0)),
    ] {
        assert_eq!(sofa_to_aurora_direction(sofa).unwrap(), aurora);
    }
}

#[test]
fn world_front_selects_right_ear_filter_after_left_turn_and_crossfades() {
    let bank = bank();
    let poses = poses();
    let front = [Vector3::new(0.0, 4.0, 0.0)];
    let initial = bank.prepare_objects(&poses, 100, &front, 1).unwrap();
    let candidate = bank.prepare_objects(&poses, 200, &front, 2).unwrap();
    let mut renderer = PreparedBinaural::new(initial, 2).unwrap();
    let mut output = [0.0; 4];
    renderer.commit(&candidate, 2).unwrap();
    renderer.process(&[1.0, 1.0], &mut output).unwrap();
    assert_eq!(output, [0.375, 0.625, 0.25, 0.75]);
    assert_eq!(renderer.commit(&candidate, 2), Err(Error::Generation));
}

#[test]
fn stable_object_order_and_interpolated_pose_select_expected_filters() {
    let bank = bank();
    let poses = poses();
    let diagonal = [Vector3::new(-1.0, 1.0, 0.0)];
    let halfway = bank.prepare_objects(&poses, 150, &diagonal, 1).unwrap();
    let mut renderer = PreparedBinaural::new(halfway, 1).unwrap();
    let mut output = [0.0; 2];
    renderer.process(&[1.0], &mut output).unwrap();
    assert_eq!(output, [0.5, 0.5]);

    let filters = bank
        .prepare_objects(
            &poses,
            100,
            &[Vector3::new(1.0, 0.0, 0.0), Vector3::new(-1.0, 0.0, 0.0)],
            1,
        )
        .unwrap();
    let mut renderer = PreparedBinaural::new(filters, 1).unwrap();
    renderer.process(&[1.0, 0.0], &mut output).unwrap();
    assert_eq!(output, [0.25, 0.75]);
    renderer.process(&[0.0, 1.0], &mut output).unwrap();
    assert_eq!(output, [0.75, 0.25]);
}

#[test]
fn missing_stale_uncovered_and_invalid_candidates_preserve_active_pcm() {
    let bank = bank();
    let poses = poses();
    let front = [Vector3::new(0.0, 1.0, 0.0)];
    let initial = bank.prepare_objects(&poses, 100, &front, 1).unwrap();
    let mut renderer = PreparedBinaural::new(initial, 1).unwrap();
    assert!(matches!(
        bank.prepare_objects(&poses, 211, &front, 2),
        Err(PreparationError::Pose(_))
    ));
    assert!(bank.prepare_objects(&poses, 210, &front, 2).is_ok());
    assert!(matches!(
        bank.prepare_objects(&poses, 100, &[Vector3::ZERO], 2),
        Err(PreparationError::Direction(_))
    ));
    assert!(matches!(
        bank.prepare_objects(&poses, 100, &[Vector3::new(0.0, -1.0, 0.0)], 2),
        Err(PreparationError::UncoveredDirection)
    ));
    assert!(bank.prepare_objects(&poses, 100, &front, 0).is_err());
    assert!(bank.prepare_objects(&poses, 100, &[], 2).is_err());
    assert!(bank
        .prepare_objects(&poses, 100, &[front[0]; 17], 2)
        .is_err());
    let empty = HeadPoseState::new(HeadPosePolicy::new(100, 10).unwrap());
    assert!(matches!(
        bank.prepare_objects(&empty, 100, &front, 2),
        Err(PreparationError::Pose(_))
    ));
    let mut output = [0.0; 2];
    renderer.process(&[1.0], &mut output).unwrap();
    assert_eq!(output, [0.5, 0.5]);
    assert_eq!(renderer.generation(), 1);
}

#[test]
fn malformed_banks_fail_closed() {
    for direction in [Vector3::ZERO, Vector3::new(f32::NAN, 0.0, 0.0)] {
        assert!(
            DirectionalHrtf::prepare(48_000, 1, 0.1, vec![measurement(direction, 0.5, 0.5)])
                .is_err()
        );
    }
    let front = Vector3::new(1.0, 0.0, 0.0);
    assert!(DirectionalHrtf::prepare(
        48_000,
        1,
        0.1,
        vec![measurement(front, 0.5, 0.5), measurement(front, 0.2, 0.2)]
    )
    .is_err());
    for angle in [0.0, -1.0, f32::NAN, 4.0] {
        assert!(
            DirectionalHrtf::prepare(48_000, 1, angle, vec![measurement(front, 0.5, 0.5)]).is_err()
        );
    }
    for (rate, taps, left) in [
        (0, 1, 0.5),
        (48_000, 2, 0.5),
        (48_000, 1, f32::NAN),
        (48_000, 1, 17.0),
    ] {
        assert!(
            DirectionalHrtf::prepare(rate, taps, 0.1, vec![measurement(front, left, 0.5)]).is_err()
        );
    }
}
