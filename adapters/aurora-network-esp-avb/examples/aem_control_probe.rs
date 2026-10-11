use aurora_core::ChannelRole;
use aurora_modular_speaker_fabric::{
    EndpointHandoverCommand, HandoverPlan, ModuleAssignment, ModuleClockSource, ModuleRoute,
};
use aurora_network_esp_avb::{
    compile_aem_control_program, compile_handover_control, EspAvbAemTarget, EspAvbEndpointMedium,
    EspAvbModuleBinding, PtpMediaAnchor, AURORA_AEM_CONTROL_COMMAND,
    AURORA_AEM_CONTROL_START_PTP_NS,
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
    let targets = [
        EspAvbAemTarget {
            endpoint_id: "esp-c6-left".into(),
            entity_id: [0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x00, 0x01],
        },
        EspAvbAemTarget {
            endpoint_id: "esp-c6-right".into(),
            entity_id: [0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x00, 0x02],
        },
    ];

    let envelopes = compile_handover_control(&handover, &bindings).unwrap();
    let anchor = PtpMediaAnchor {
        frame_index: 48_000,
        ptp_time_ns: 1_000_000_000,
        sample_rate: 48_000,
    };
    let writes = compile_aem_control_program(&envelopes, &targets, anchor).unwrap();

    let start_times = writes
        .iter()
        .filter(|write| write.control_index == AURORA_AEM_CONTROL_START_PTP_NS)
        .map(|write| write.value_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(start_times, vec![2_000_000_000, 2_000_000_000]);
    assert_eq!(
        writes
            .iter()
            .filter(|write| write.control_index == AURORA_AEM_CONTROL_COMMAND)
            .count(),
        8
    );

    println!(
        "AURORA-ESP-AVB-AEM-CONTROL-PASS modules=2 envelopes={} writes={} start_ptp_ns={} controls=3-7 transport=aecp-aem",
        envelopes.len(),
        writes.len(),
        start_times[0]
    );
}
