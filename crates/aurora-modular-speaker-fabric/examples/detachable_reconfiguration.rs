use aurora_modular_speaker_fabric::{
    detached_rear_intent, stereo_pair_intent, FabricPlanner, ModuleAttachment, ModuleClockSource,
    ModuleSyncStatus, SpeakerModuleCapabilities, SpeakerModuleState,
};

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
    println!("docked={bar:?}");

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
            &detached_rear_intent("left-pod", "right-pod"),
        )
        .expect("detached rear plan");
    println!("detached={rears:?}");
}
