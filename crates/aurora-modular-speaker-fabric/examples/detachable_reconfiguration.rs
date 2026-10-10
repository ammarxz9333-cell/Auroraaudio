use aurora_modular_speaker_fabric::{
    build_handover_plan, detachable_atmos_rear_intent, docked_atmos_wing_intent,
    EndpointHandoverCommand, EndpointHandoverState, FabricPlanner, ModuleAttachment,
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
        .plan(&docked, &docked_atmos_wing_intent("left-pod", "right-pod"))
        .expect("docked Atmos wing plan");
    assert_eq!(bar.assignments.len(), 2);
    assert_eq!(
        bar.assignments[0].roles,
        vec![
            aurora_core::ChannelRole::FrontLeft,
            aurora_core::ChannelRole::TopFrontLeft,
        ]
    );
    assert_eq!(
        bar.assignments[1].roles,
        vec![
            aurora_core::ChannelRole::FrontRight,
            aurora_core::ChannelRole::TopFrontRight,
        ]
    );

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

    let handover = build_handover_plan(Some(&bar), &rears, 1, 192_000)
        .expect("atomic dock-to-wireless handover");
    assert_eq!(handover.commands.len(), 8);

    let mut left_endpoint = EndpointHandoverState::new("left-pod");
    let mut right_endpoint = EndpointHandoverState::new("right-pod");
    for command in &handover.commands {
        let module_id = match command {
            EndpointHandoverCommand::Mute { module_id, .. }
            | EndpointHandoverCommand::Arm { module_id, .. }
            | EndpointHandoverCommand::Commit { module_id, .. } => module_id.as_str(),
            EndpointHandoverCommand::Prepare { assignment, .. } => assignment.module_id.as_str(),
        };
        match module_id {
            "left-pod" => left_endpoint.apply(command).expect("left endpoint handover"),
            "right-pod" => right_endpoint.apply(command).expect("right endpoint handover"),
            _ => panic!("unexpected module in handover"),
        }
    }
    assert!(!left_endpoint.is_muted());
    assert!(!right_endpoint.is_muted());
    assert_eq!(left_endpoint.armed_start_frame(), Some(192_000));
    assert_eq!(right_endpoint.armed_start_frame(), Some(192_000));

    println!(
        "AURORA-MODULAR-ATMOS-PASS docked=FL+TFL,FR+TFR detached=SL+TRL,SR+TRR lanes_per_pod=2 handover=atomic"
    );
}
