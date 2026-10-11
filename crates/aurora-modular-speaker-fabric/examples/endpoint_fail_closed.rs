use aurora_core::ChannelRole;
use aurora_modular_speaker_fabric::{
    endpoint::{
        DetachableEndpoint, EndpointCommand, EndpointPhase, EndpointPolicy, EndpointPowerStatus,
    },
    ModuleClockSource, ModuleRoute, ModuleSyncStatus, SpeakerModuleCapabilities,
};

fn main() {
    let capabilities = SpeakerModuleCapabilities {
        output_lanes: 2,
        dock_pcm: true,
        wired_network_audio: false,
        wireless_network_audio: true,
        battery_powered: true,
    };
    let mut pod =
        DetachableEndpoint::new("left-pod", capabilities, EndpointPolicy::default()).unwrap();

    pod.update_power(EndpointPowerStatus {
        external_power: false,
        battery_percent: Some(82),
        charging: false,
    })
    .unwrap();
    pod.update_sync(ModuleSyncStatus {
        locked: true,
        scheduled_playout: true,
        estimated_skew_micros: Some(160),
        clock_source: ModuleClockSource::Ptp,
    });

    for command in [
        EndpointCommand::Mute {
            module_id: "left-pod".into(),
            epoch: 1,
        },
        EndpointCommand::Prepare {
            module_id: "left-pod".into(),
            epoch: 1,
            route: ModuleRoute::WirelessNetwork,
            clock_source: ModuleClockSource::Ptp,
            roles: vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft],
        },
        EndpointCommand::Arm {
            module_id: "left-pod".into(),
            epoch: 1,
            start_frame: 96_000,
        },
        EndpointCommand::Commit {
            module_id: "left-pod".into(),
            epoch: 1,
        },
    ] {
        let actions = pod.apply(&command).unwrap();
        println!("{command:?} -> {actions:?}");
    }

    assert_eq!(pod.phase(), EndpointPhase::Committed);
    assert!(pod.amplifier_muted());
    assert!(pod.on_media_frame(95_999).unwrap().is_empty());

    let start_actions = pod.on_media_frame(96_000).unwrap();
    assert_eq!(pod.phase(), EndpointPhase::Playing);
    assert!(!pod.amplifier_muted());
    println!("start -> {start_actions:?}");

    pod.update_sync(ModuleSyncStatus {
        locked: false,
        scheduled_playout: true,
        estimated_skew_micros: None,
        clock_source: ModuleClockSource::Ptp,
    });
    assert_eq!(pod.phase(), EndpointPhase::Fault);
    assert!(pod.amplifier_muted());

    println!("AURORA-DETACHABLE-ENDPOINT-FAIL-CLOSED-PASS");
}
