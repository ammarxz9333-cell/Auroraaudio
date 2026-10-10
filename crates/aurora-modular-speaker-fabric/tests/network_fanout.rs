use aurora_modular_speaker_fabric::{
    detachable_atmos_rear_intent, materialize_network_routes, FabricPlanner, ModuleAttachment,
    ModuleClockSource, ModuleSyncStatus, NetworkFanoutError, PreparedModuleNetworkFanout,
    SpeakerModuleCapabilities, SpeakerModuleState,
};
use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkTimingPolicy,
    AURORA_NETWORK_MEDIA_RATE,
};

fn pod(id: &str, skew: u32) -> SpeakerModuleState {
    SpeakerModuleState {
        module_id: id.to_owned(),
        capabilities: SpeakerModuleCapabilities {
            output_lanes: 2,
            dock_pcm: true,
            wired_network_audio: false,
            wireless_network_audio: true,
            battery_powered: true,
        },
        attachment: ModuleAttachment::Detached,
        sync: ModuleSyncStatus {
            locked: true,
            scheduled_playout: true,
            estimated_skew_micros: Some(skew),
            clock_source: ModuleClockSource::Ptp,
        },
        online: true,
    }
}

fn timing() -> NetworkTimingPolicy {
    NetworkTimingPolicy {
        target_latency_frames: 480,
        minimum_latency_frames: 240,
        maximum_latency_frames: 960,
        maximum_rate_correction_ppm: 250.0,
    }
}

fn samples(block_frames: usize) -> Vec<f32> {
    (0..block_frames)
        .flat_map(|frame| (0..12).map(move |channel| (frame * 100 + channel) as f32))
        .collect()
}

#[test]
fn detachable_atmos_pods_receive_horizontal_and_height_lanes_from_one_timeline() {
    let planner = FabricPlanner::with_defaults();
    let modules = [pod("left-pod", 120), pod("right-pod", 140)];
    let plan = planner
        .plan(
            &modules,
            &detachable_atmos_rear_intent("left-pod", "right-pod"),
        )
        .unwrap();

    let routes = materialize_network_routes(&plan, 2, timing()).unwrap();
    assert_eq!(routes[0].source_channel_indices, vec![4, 10]);
    assert_eq!(routes[1].source_channel_indices, vec![5, 11]);

    let mut fanout = PreparedModuleNetworkFanout::prepare(routes).unwrap();
    let pcm = samples(2);
    let input = NetworkAudioBlock {
        sequence: 7,
        timestamp: MediaTimestamp::new(96_000, AURORA_NETWORK_MEDIA_RATE).unwrap(),
        format: NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: 12,
            block_frames: 2,
        },
        samples: &pcm,
    };

    fanout.split(&input).unwrap();

    let left = fanout.module_block(0).unwrap();
    let right = fanout.module_block(1).unwrap();
    assert_eq!(left.sequence, 7);
    assert_eq!(right.sequence, 7);
    assert_eq!(left.timestamp, right.timestamp);
    assert_eq!(left.samples, &[4.0, 10.0, 104.0, 110.0]);
    assert_eq!(right.samples, &[5.0, 11.0, 105.0, 111.0]);
}

#[test]
fn fanout_rejects_timeline_skip_until_explicit_reset() {
    let planner = FabricPlanner::with_defaults();
    let modules = [pod("left-pod", 120), pod("right-pod", 140)];
    let plan = planner
        .plan(
            &modules,
            &detachable_atmos_rear_intent("left-pod", "right-pod"),
        )
        .unwrap();
    let routes = materialize_network_routes(&plan, 2, timing()).unwrap();
    let mut fanout = PreparedModuleNetworkFanout::prepare(routes).unwrap();
    let pcm = samples(2);
    let format = NetworkAudioFormat {
        sample_rate: AURORA_NETWORK_MEDIA_RATE,
        channels: 12,
        block_frames: 2,
    };

    fanout
        .split(&NetworkAudioBlock {
            sequence: 1,
            timestamp: MediaTimestamp::new(0, AURORA_NETWORK_MEDIA_RATE).unwrap(),
            format,
            samples: &pcm,
        })
        .unwrap();

    let skipped = NetworkAudioBlock {
        sequence: 3,
        timestamp: MediaTimestamp::new(2, AURORA_NETWORK_MEDIA_RATE).unwrap(),
        format,
        samples: &pcm,
    };
    assert_eq!(
        fanout.split(&skipped),
        Err(NetworkFanoutError::TimelineDiscontinuity)
    );

    fanout.reset();
    fanout.split(&skipped).unwrap();
}
