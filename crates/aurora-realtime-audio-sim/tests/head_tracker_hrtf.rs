use aurora_core::Vector3;
use aurora_realtime_audio_sim::{
    builtin_head_tracker_profile, generate_head_tracker_trace, HeadTrackerDeliveryEvent,
};
use aurora_renderer_api::{HeadPoseClockMapper, HeadPoseClockPolicy, HeadPosePolicy};
use aurora_renderer_basic::binaural::hrtf::scheduler::{HeadPoseHrtfScheduler, ObjectDirection};
use aurora_renderer_basic::binaural::hrtf::{DirectionalHrtf, SofaMeasurement};
use aurora_renderer_basic::binaural::{Filters, Input, PreparedBinaural};

#[test]
fn reconnect_trace_drives_continuous_mapper_scheduler_and_hrtf_commits() {
    let config = builtin_head_tracker_profile("head-tracker-reconnect").unwrap();
    let trace = generate_head_tracker_trace(&config).unwrap();
    let clock_policy = HeadPoseClockPolicy::new(48_000, 25_000_000, 240, 48).unwrap();
    let mut mapper = HeadPoseClockMapper::new(clock_policy, trace.initial_anchor);

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
        HeadPosePolicy::new(600, 600).unwrap(),
        vec!["dialogue".to_owned()],
        1,
    )
    .unwrap();

    let initial = Filters::prepare(Input::Objects(1), 48_000, 1, 1, vec![0.5, 0.5]).unwrap();
    let mut renderer = PreparedBinaural::new(initial, 8).unwrap();
    let objects = [ObjectDirection {
        id: "dialogue",
        world_direction: Vector3::new(0.0, 1.0, 0.0),
    }];

    let mut committed = 0_u64;
    let mut reconnects = 0_u64;

    for event in trace.events {
        match event {
            HeadTrackerDeliveryEvent::Sample {
                sample,
                observed_media_frame,
            } => {
                let mapped = mapper.map(sample, observed_media_frame).unwrap();
                scheduler.commit_pose(mapped).unwrap();
                let generation = scheduler.prepare_at(mapped.media_frame, &objects).unwrap();
                scheduler
                    .commit_at_boundary(&mut renderer, mapped.media_frame, 1)
                    .unwrap();

                let mut output = [0.0; 16];
                renderer.process(&[1.0; 8], &mut output).unwrap();
                assert!(output.iter().all(|sample| sample.is_finite()));

                scheduler.release_committed(generation).unwrap();
                committed += 1;
            }
            HeadTrackerDeliveryEvent::Reanchor { anchor } => {
                mapper.reset(anchor);
                scheduler.reset_pose_epoch().unwrap();
                reconnects += 1;
            }
            HeadTrackerDeliveryEvent::Disconnected { .. } => {}
            HeadTrackerDeliveryEvent::Dropped { .. } => {
                panic!("reconnect profile must not inject sample loss");
            }
        }
    }

    assert_eq!(reconnects, 1);
    assert_eq!(committed, config.sample_count as u64);
    assert_eq!(renderer.generation(), committed + 1);
}
