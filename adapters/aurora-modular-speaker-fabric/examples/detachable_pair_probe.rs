use aurora_core::ChannelRole;
use aurora_modular_speaker_fabric::{
    AttachmentState, FabricPolicy, FabricProfile, ModuleSpec, ModuleTelemetry, ModuleTransport,
    RouteReason, SpeakerFabric, SyncState,
};

fn module(
    id: &str,
    dock_role: ChannelRole,
    detached_role: ChannelRole,
    stereo_role: ChannelRole,
) -> ModuleSpec {
    ModuleSpec {
        id: id.to_owned(),
        dock_role,
        detached_cinema_role: Some(detached_role),
        stereo_role: Some(stereo_role),
        supports_dock_bus: true,
        supports_network_audio: true,
        has_battery: true,
    }
}

fn locked() -> SyncState {
    SyncState::Locked {
        offset_micros: 140,
        jitter_micros: 80,
        drift_ppm: 12,
    }
}

fn main() {
    let fabric = SpeakerFabric::new(FabricPolicy::default()).expect("valid fabric policy");
    let left = module(
        "wing-left",
        ChannelRole::FrontLeft,
        ChannelRole::SurroundLeft,
        ChannelRole::FrontLeft,
    );
    let right = module(
        "wing-right",
        ChannelRole::FrontRight,
        ChannelRole::SurroundRight,
        ChannelRole::FrontRight,
    );

    let docked_left = fabric
        .resolve(
            &left,
            ModuleTelemetry {
                attachment: AttachmentState::Docked,
                battery_percent: None,
                sync: SyncState::Unknown,
            },
            FabricProfile::Soundbar,
        )
        .expect("docked left");
    let docked_right = fabric
        .resolve(
            &right,
            ModuleTelemetry {
                attachment: AttachmentState::Docked,
                battery_percent: None,
                sync: SyncState::Unknown,
            },
            FabricProfile::Soundbar,
        )
        .expect("docked right");

    assert_eq!(docked_left.transport, ModuleTransport::DockBus);
    assert_eq!(docked_right.transport, ModuleTransport::DockBus);

    let rear_left = fabric
        .resolve(
            &left,
            ModuleTelemetry {
                attachment: AttachmentState::Detached,
                battery_percent: Some(91),
                sync: locked(),
            },
            FabricProfile::Cinema,
        )
        .expect("detached rear left");
    let rear_right = fabric
        .resolve(
            &right,
            ModuleTelemetry {
                attachment: AttachmentState::Detached,
                battery_percent: Some(88),
                sync: locked(),
            },
            FabricProfile::Cinema,
        )
        .expect("detached rear right");

    assert_eq!(rear_left.role, Some(ChannelRole::SurroundLeft));
    assert_eq!(rear_right.role, Some(ChannelRole::SurroundRight));
    assert_eq!(rear_left.transport, ModuleTransport::Network);
    assert_eq!(rear_right.transport, ModuleTransport::Network);

    let unsafe_right = fabric
        .resolve(
            &right,
            ModuleTelemetry {
                attachment: AttachmentState::Detached,
                battery_percent: Some(87),
                sync: SyncState::Locked {
                    offset_micros: 150,
                    jitter_micros: 2_000,
                    drift_ppm: 15,
                },
            },
            FabricProfile::Cinema,
        )
        .expect("unsafe route resolves fail-closed");

    assert_eq!(unsafe_right.transport, ModuleTransport::Muted);
    assert_eq!(
        unsafe_right.reason,
        RouteReason::SyncOutsideCinemaPolicy
    );

    let redocked_left = fabric
        .resolve(
            &left,
            ModuleTelemetry {
                attachment: AttachmentState::Docked,
                battery_percent: Some(92),
                sync: SyncState::Fault,
            },
            FabricProfile::Cinema,
        )
        .expect("redocked left");
    let redocked_right = fabric
        .resolve(
            &right,
            ModuleTelemetry {
                attachment: AttachmentState::Docked,
                battery_percent: Some(89),
                sync: SyncState::Fault,
            },
            FabricProfile::Cinema,
        )
        .expect("redocked right");

    assert_eq!(redocked_left.role, Some(ChannelRole::FrontLeft));
    assert_eq!(redocked_right.role, Some(ChannelRole::FrontRight));
    assert_eq!(redocked_left.transport, ModuleTransport::DockBus);
    assert_eq!(redocked_right.transport, ModuleTransport::DockBus);

    println!(
        "AURORA-MODULAR-FABRIC-PASS docked=2 detached_rears=2 fail_closed=1 redocked=2"
    );
}
