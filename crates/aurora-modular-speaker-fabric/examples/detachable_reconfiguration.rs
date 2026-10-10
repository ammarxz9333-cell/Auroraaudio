use aurora_modular_speaker_fabric::{
    detachable_atmos_rear_intent, stereo_pair_intent, FabricPlanner, ModuleAttachment,
    ModuleClockSource, ModuleSyncStatus, SpeakerModuleCapabilities, SpeakerModuleState,
};
use aurora_realtime_audio_api::NetworkTimingPolicy;

fn module(id: &str, attachment: ModuleAttachment, sync: ModuleSyncStatus) -> SpeakerModuleState {
    SpeakerModuleState {
        module_id: id.to_owned(),
        capabilities: SpeakerModuleCapabilities {
            output_lanes: 2,
            dock_pcm: true,
            wired_network_audio: false,
            wireless_network_audio: true,
            battery_powered: true,
        },
        attachment,
        sync,
        online: true,
    }
}

fn main() {
    let planner = FabricPlanner::with_defaults();
    let docked = [
        module(
            "left-pod",
            ModuleAttachment::Docked {
                slot_id: "left-edge".to_owned(),
            },
            ModuleSyncStatus::docked_locked(),
        ),
        module(
            "right-pod",
            ModuleAttachment::Docked {
                slot_id: "right-edge".to_owned(),
            },
            ModuleSyncStatus::docked_locked(),
        ),
    ];
    let bar = planner
        .plan(&docked, &stereo_pair_intent("left-pod", "right-pod"))
        .expect("docked stereo plan");
    assert_eq!(bar.assignments.len(), 2);

    let wireless_sync = |skew| ModuleSyncStatus {
        locked: true,
        scheduled_playout: true,
        estimated_skew_micros: Some(skew),
        clock_source: ModuleClockSource::Ptp,
    };
    let detached = [
        module("left-pod", ModuleAttachment::Detached, wireless_sync(180)),
        module("right-pod", ModuleAttachment::Detached, wireless_sync(220)),
    ];
    let rears = planner
        .plan(
            &detached,
            &detachable_atmos_rear_intent("left-pod", "right-pod"),
        )
        .expect("detached Atmos rear plan");

    let routes = aurora_modular_speaker_fabric::materialize_network_routes(
        &rears,
        48,
        NetworkTimingPolicy {
            target_latency_frames: 480,
            minimum_latency_frames: 240,
            maximum_latency_frames: 960,
            maximum_rate_correction_ppm: 250.0,
        },
    )
    .expect("network routes");

    assert_eq!(routes[0].source_channel_indices, vec![4, 10]);
    assert_eq!(routes[1].source_channel_indices, vec![5, 11]);

    println!(
        "AURORA-MODULAR-ATMOS-PASS docked=2 detached=2 lanes_per_pod=2 left=SL+TRL right=SR+TRR"
    );
}
