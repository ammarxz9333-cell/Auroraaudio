use aurora_network_esp_avb::{
    ModularCinemaPlan, ModularFabricPolicy, ModuleAttachment, ModuleIntent, ModuleObservation,
    ModuleSync, ModuleTransport, RolePair,
};

fn cinema_module(
    id: &str,
    role: RolePair,
    attachment: ModuleAttachment,
    transport: ModuleTransport,
) -> ModuleObservation {
    let sync = match transport {
        ModuleTransport::DockBus => ModuleSync::DockClockLocked,
        ModuleTransport::WiredAvbP4 | ModuleTransport::WirelessAvbC6 => {
            ModuleSync::NetworkClockLocked {
                absolute_offset_us: 100,
            }
        }
    };
    ModuleObservation {
        module_id: id.to_owned(),
        attachment,
        transport,
        sync,
        battery_percent: if attachment == ModuleAttachment::Detached {
            Some(80)
        } else {
            None
        },
        intent: ModuleIntent::Cinema(role),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let policy = ModularFabricPolicy::default();
    let mut modules = vec![
        cinema_module(
            "front",
            RolePair::Front,
            ModuleAttachment::Docked,
            ModuleTransport::DockBus,
        ),
        cinema_module(
            "center-sub",
            RolePair::CenterLfe,
            ModuleAttachment::Docked,
            ModuleTransport::DockBus,
        ),
        cinema_module(
            "side",
            RolePair::SideSurround,
            ModuleAttachment::Detached,
            ModuleTransport::WirelessAvbC6,
        ),
        cinema_module(
            "back",
            RolePair::BackSurround,
            ModuleAttachment::Detached,
            ModuleTransport::WirelessAvbC6,
        ),
        cinema_module(
            "top-front",
            RolePair::TopFront,
            ModuleAttachment::Docked,
            ModuleTransport::DockBus,
        ),
        cinema_module(
            "top-rear",
            RolePair::TopRear,
            ModuleAttachment::Detached,
            ModuleTransport::WirelessAvbC6,
        ),
    ];

    let first = ModularCinemaPlan::build(&modules, policy)?;
    if first.assignments().len() != 6 {
        return Err("initial modular cinema plan is incomplete".into());
    }

    modules[0].attachment = ModuleAttachment::Detached;
    modules[0].transport = ModuleTransport::WirelessAvbC6;
    modules[0].sync = ModuleSync::NetworkClockLocked {
        absolute_offset_us: 80,
    };
    modules[0].battery_percent = Some(70);
    modules[0].intent = ModuleIntent::Cinema(RolePair::SideSurround);
    modules[2].intent = ModuleIntent::Cinema(RolePair::Front);

    let reassigned = ModularCinemaPlan::build(&modules, policy)?;
    if reassigned.assignment_for("front").map(|a| a.role_pair) != Some(RolePair::SideSurround) {
        return Err("detached module was not reassigned to side-surround".into());
    }
    if reassigned.assignment_for("side").map(|a| a.role_pair) != Some(RolePair::Front) {
        return Err("former side module was not reassigned to front".into());
    }

    modules[3].sync = ModuleSync::Lost;
    if ModularCinemaPlan::build(&modules, policy).is_ok() {
        return Err("cinema plan must fail closed when one active endpoint loses sync".into());
    }

    println!(
        "aurora-modular-speaker-fabric: PASS modules=6 channels=12 docked-and-wireless=true reassign=true fail_closed=true"
    );
    Ok(())
}
