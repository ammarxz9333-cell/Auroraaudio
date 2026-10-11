use aurora_core::ChannelRole;
use aurora_modular_speaker_fabric::{
    EndpointHandoverCommand, HandoverPlan, ModuleAssignment, ModuleClockSource, ModuleRoute,
};
use aurora_network_esp_avb::{
    compile_handover_control, EspAvbEndpointMedium, EspAvbModuleBinding,
};

fn main() {
    let handover = HandoverPlan {
        epoch: 42,
        start_frame: 96_000,
        commands: vec![
            EndpointHandoverCommand::Mute {
                module_id: "left-pod".into(),
                epoch: 42,
            },
            EndpointHandoverCommand::Mute {
                module_id: "right-pod".into(),
                epoch: 42,
            },
            EndpointHandoverCommand::Prepare {
                assignment: ModuleAssignment {
                    module_id: "left-pod".into(),
                    route: ModuleRoute::WirelessNetwork,
                    clock_source: ModuleClockSource::Ptp,
                    roles: vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft],
                },
                epoch: 42,
            },
            EndpointHandoverCommand::Prepare {
                assignment: ModuleAssignment {
                    module_id: "right-pod".into(),
                    route: ModuleRoute::WirelessNetwork,
                    clock_source: ModuleClockSource::Ptp,
                    roles: vec![ChannelRole::SurroundRight, ChannelRole::TopRearRight],
                },
                epoch: 42,
            },
            EndpointHandoverCommand::Arm {
                module_id: "left-pod".into(),
                epoch: 42,
                start_frame: 96_000,
            },
            EndpointHandoverCommand::Arm {
                module_id: "right-pod".into(),
                epoch: 42,
                start_frame: 96_000,
            },
            EndpointHandoverCommand::Commit {
                module_id: "left-pod".into(),
                epoch: 42,
            },
            EndpointHandoverCommand::Commit {
                module_id: "right-pod".into(),
                epoch: 42,
            },
        ],
    };

    let bindings = [
        EspAvbModuleBinding {
            module_id: "left-pod".into(),
            endpoint_id: "esp-c6-left".into(),
            stream_id: 0xA700_0010,
            medium: EspAvbEndpointMedium::WirelessEsp32C6,
        },
        EspAvbModuleBinding {
            module_id: "right-pod".into(),
            endpoint_id: "esp-c6-right".into(),
            stream_id: 0xA700_0011,
            medium: EspAvbEndpointMedium::WirelessEsp32C6,
        },
    ];

    let envelopes = compile_handover_control(&handover, &bindings).unwrap();
    assert_eq!(envelopes.len(), 8);

    println!(
        "AURORA-ESP-AVB-DETACHABLE-CONTROL-PASS messages={} modules=2 epoch={} start_frame={} lanes_per_pod=2 clock=ptp",
        envelopes.len(),
        handover.epoch,
        handover.start_frame
    );
}
