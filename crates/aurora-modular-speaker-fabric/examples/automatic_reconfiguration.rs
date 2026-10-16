use aurora_modular_speaker_fabric::{
    profile::{
        build_pair_reconfiguration, DetachableAtmosPairProfile, PairReconfiguration, PairTopology,
    },
    FabricPlanner, ModuleAttachment, ModuleClockSource, ModuleSyncStatus, SpeakerModuleCapabilities,
    SpeakerModuleState,
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

fn ptp(skew: u32) -> ModuleSyncStatus {
    ModuleSyncStatus {
        locked: true,
        scheduled_playout: true,
        estimated_skew_micros: Some(skew),
        clock_source: ModuleClockSource::Ptp,
    }
}

fn main() {
    let profile = DetachableAtmosPairProfile::new(
        "left-pod",
        "right-pod",
        "left-edge",
        "right-edge",
    )
    .expect("valid profile");
    let planner = FabricPlanner::with_defaults();

    let docked = vec![
        module(
            "left-pod",
            ModuleAttachment::Docked {
                slot_id: "left-edge".into(),
            },
            ModuleSyncStatus::docked_locked(),
        ),
        module(
            "right-pod",
            ModuleAttachment::Docked {
                slot_id: "right-edge".into(),
            },
            ModuleSyncStatus::docked_locked(),
        ),
    ];

    let first = build_pair_reconfiguration(&profile, &planner, None, &docked, 1, 96_000)
        .expect("initial docked topology");
    let docked_plan = match first {
        PairReconfiguration::Active {
            topology,
            plan,
            handover,
        } => {
            assert_eq!(topology, PairTopology::DockedBar);
            assert_eq!(handover.commands.len(), 8);
            plan
        }
        PairReconfiguration::FailClosed { .. } => panic!("docked pair rejected"),
    };

    // One pod is removed first. Aurora does not guess a new role and does not
    // leave the other pod playing the previous topology: both are muted.
    let mixed = vec![
        module("left-pod", ModuleAttachment::Detached, ptp(120)),
        module(
            "right-pod",
            ModuleAttachment::Docked {
                slot_id: "right-edge".into(),
            },
            ModuleSyncStatus::docked_locked(),
        ),
    ];
    let transition =
        build_pair_reconfiguration(&profile, &planner, Some(&docked_plan), &mixed, 2, 144_000)
            .expect("mixed state produces a safe mute plan");
    match transition {
        PairReconfiguration::FailClosed { handover, .. } => {
            assert_eq!(handover.commands.len(), 2);
        }
        PairReconfiguration::Active { .. } => panic!("mixed topology became audible"),
    }

    // Once both pods are detached and synchronized, the approved profile
    // automatically changes them from FL+TFL/FR+TFR to SL+TRL/SR+TRR.
    let detached = vec![
        module("left-pod", ModuleAttachment::Detached, ptp(140)),
        module("right-pod", ModuleAttachment::Detached, ptp(170)),
    ];
    let rears = build_pair_reconfiguration(&profile, &planner, None, &detached, 3, 192_000)
        .expect("detached topology");
    match rears {
        PairReconfiguration::Active {
            topology,
            plan,
            handover,
        } => {
            assert_eq!(topology, PairTopology::DetachedRears);
            assert_eq!(plan.assignments.len(), 2);
            assert_eq!(handover.commands.len(), 8);
        }
        PairReconfiguration::FailClosed { .. } => panic!("detached pair rejected"),
    }

    println!(
        "AURORA-AUTO-MODULAR-PASS docked=FL+TFL,FR+TFR mixed=muted detached=SL+TRL,SR+TRR"
    );
}
