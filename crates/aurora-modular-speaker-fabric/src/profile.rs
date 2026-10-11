//! Deterministic topology profiles for detachable Aurora speaker pairs.
//!
//! A profile is the explicit user/calibration-approved mapping that lets Aurora
//! react automatically to dock state without guessing room position. Mixed,
//! unknown, offline, or unsynchronized states produce a mute-only handover.

use thiserror::Error;

use crate::{
    build_handover_plan, detachable_atmos_rear_intent, docked_atmos_wing_intent, DeploymentIntent,
    DeploymentMode, FabricError, FabricPlan, FabricPlanner, HandoverError, HandoverPlan,
    ModuleAttachment, SpeakerModuleState,
};

/// Stable topology selected by the approved two-pod Atmos profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairTopology {
    /// Both pods are in their expected soundbar dock slots.
    DockedBar,
    /// Both pods are physically detached and may be admitted as rear Atmos pods.
    DetachedRears,
}

/// Explicit identity/slot binding for one detachable Atmos pair.
///
/// The dock slot IDs are part of the profile so a swapped or unexpected pod
/// does not silently acquire another speaker role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachableAtmosPairProfile {
    pub left_module_id: String,
    pub right_module_id: String,
    pub left_dock_slot: String,
    pub right_dock_slot: String,
}

impl DetachableAtmosPairProfile {
    pub fn new(
        left_module_id: impl Into<String>,
        right_module_id: impl Into<String>,
        left_dock_slot: impl Into<String>,
        right_dock_slot: impl Into<String>,
    ) -> Result<Self, ProfileError> {
        let profile = Self {
            left_module_id: left_module_id.into(),
            right_module_id: right_module_id.into(),
            left_dock_slot: left_dock_slot.into(),
            right_dock_slot: right_dock_slot.into(),
        };
        if profile.left_module_id.is_empty()
            || profile.right_module_id.is_empty()
            || profile.left_dock_slot.is_empty()
            || profile.right_dock_slot.is_empty()
            || profile.left_module_id == profile.right_module_id
            || profile.left_dock_slot == profile.right_dock_slot
        {
            return Err(ProfileError::InvalidProfile);
        }
        Ok(profile)
    }

    /// Resolves only physical topology. Synchronization/power/transport
    /// admission remains the responsibility of FabricPlanner.
    pub fn resolve(
        &self,
        modules: &[SpeakerModuleState],
    ) -> Result<ResolvedPairProfile, ProfileError> {
        let left = unique_module(modules, &self.left_module_id)?;
        let right = unique_module(modules, &self.right_module_id)?;

        match (&left.attachment, &right.attachment) {
            (
                ModuleAttachment::Docked { slot_id: left_slot },
                ModuleAttachment::Docked {
                    slot_id: right_slot,
                },
            ) => {
                if left_slot != &self.left_dock_slot || right_slot != &self.right_dock_slot {
                    return Err(ProfileError::UnexpectedDockSlot);
                }
                Ok(ResolvedPairProfile {
                    topology: PairTopology::DockedBar,
                    intent: docked_atmos_wing_intent(
                        &self.left_module_id,
                        &self.right_module_id,
                    ),
                })
            }
            (ModuleAttachment::Detached, ModuleAttachment::Detached) => Ok(ResolvedPairProfile {
                topology: PairTopology::DetachedRears,
                intent: detachable_atmos_rear_intent(
                    &self.left_module_id,
                    &self.right_module_id,
                ),
            }),
            _ => Err(ProfileError::TransitionInProgress),
        }
    }
}

/// Resolved, explicit deployment request selected from physical attachment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPairProfile {
    pub topology: PairTopology,
    pub intent: DeploymentIntent,
}

/// Profile-resolution failures are unsafe for automatic playback.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProfileError {
    #[error("invalid detachable-pair profile")]
    InvalidProfile,
    #[error("profile module is missing from inventory: {0}")]
    MissingModule(String),
    #[error("profile module appears more than once in inventory: {0}")]
    DuplicateModule(String),
    #[error("docked module is not in its approved dock slot")]
    UnexpectedDockSlot,
    #[error("detachable pair is between approved topologies")]
    TransitionInProgress,
}

fn unique_module<'a>(
    modules: &'a [SpeakerModuleState],
    module_id: &str,
) -> Result<&'a SpeakerModuleState, ProfileError> {
    let mut matches = modules.iter().filter(|module| module.module_id == module_id);
    let first = matches
        .next()
        .ok_or_else(|| ProfileError::MissingModule(module_id.to_owned()))?;
    if matches.next().is_some() {
        return Err(ProfileError::DuplicateModule(module_id.to_owned()));
    }
    Ok(first)
}

/// Reason an automatic profile could not be admitted for playback.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PairAdmissionFailure {
    #[error(transparent)]
    Profile(#[from] ProfileError),
    #[error(transparent)]
    Fabric(#[from] FabricError),
}

/// Result of one automatic topology reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairReconfiguration {
    /// A complete topology passed Aurora fabric admission.
    Active {
        topology: PairTopology,
        plan: FabricPlan,
        handover: HandoverPlan,
    },
    /// Playback must remain silent until a complete approved topology returns.
    ///
    /// If a previous plan existed, handover contains Mute commands for every
    /// previously active module and no Prepare/Arm/Commit commands.
    FailClosed {
        reason: PairAdmissionFailure,
        handover: HandoverPlan,
    },
}

/// Resolve the approved pair profile, run normal Aurora admission, and build one
/// atomic handover. Any topology/admission ambiguity is converted to a mute-only
/// plan instead of leaving the previous soundbar/rear assignment active.
pub fn build_pair_reconfiguration(
    profile: &DetachableAtmosPairProfile,
    planner: &FabricPlanner,
    previous: Option<&FabricPlan>,
    modules: &[SpeakerModuleState],
    epoch: u64,
    start_frame: u64,
) -> Result<PairReconfiguration, HandoverError> {
    let admitted = profile
        .resolve(modules)
        .map_err(PairAdmissionFailure::from)
        .and_then(|resolved| {
            planner
                .plan(modules, &resolved.intent)
                .map(|plan| (resolved.topology, plan))
                .map_err(PairAdmissionFailure::from)
        });

    match admitted {
        Ok((topology, plan)) => {
            let handover = build_handover_plan(previous, &plan, epoch, start_frame)?;
            Ok(PairReconfiguration::Active {
                topology,
                plan,
                handover,
            })
        }
        Err(reason) => {
            let muted = FabricPlan {
                mode: DeploymentMode::Cinema,
                assignments: Vec::new(),
            };
            let handover = build_handover_plan(previous, &muted, epoch, start_frame)?;
            Ok(PairReconfiguration::FailClosed { reason, handover })
        }
    }
}

#[cfg(test)]
mod tests {
    use aurora_core::ChannelRole;

    use crate::{
        FabricPlanner, ModuleClockSource, ModuleSyncStatus, SpeakerModuleCapabilities,
    };

    use super::*;

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

    fn profile() -> DetachableAtmosPairProfile {
        DetachableAtmosPairProfile::new(
            "left-pod",
            "right-pod",
            "left-edge",
            "right-edge",
        )
        .unwrap()
    }

    fn docked() -> Vec<SpeakerModuleState> {
        vec![
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
        ]
    }

    #[test]
    fn approved_dock_slots_select_front_and_top_front_roles() {
        let resolved = profile().resolve(&docked()).unwrap();
        assert_eq!(resolved.topology, PairTopology::DockedBar);
        assert_eq!(
            resolved.intent.targets[0].roles,
            vec![ChannelRole::FrontLeft, ChannelRole::TopFrontLeft]
        );
        assert_eq!(
            resolved.intent.targets[1].roles,
            vec![ChannelRole::FrontRight, ChannelRole::TopFrontRight]
        );
    }

    #[test]
    fn both_detached_select_rear_atmos_roles() {
        let modules = vec![
            module("left-pod", ModuleAttachment::Detached, ptp(120)),
            module("right-pod", ModuleAttachment::Detached, ptp(160)),
        ];
        let resolved = profile().resolve(&modules).unwrap();
        assert_eq!(resolved.topology, PairTopology::DetachedRears);
        assert_eq!(
            resolved.intent.targets[0].roles,
            vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft]
        );
        assert_eq!(
            resolved.intent.targets[1].roles,
            vec![ChannelRole::SurroundRight, ChannelRole::TopRearRight]
        );
    }

    #[test]
    fn mixed_attachment_mutes_previous_topology_instead_of_guessing() {
        let planner = FabricPlanner::with_defaults();
        let docked = docked();
        let previous = planner
            .plan(&docked, &profile().resolve(&docked).unwrap().intent)
            .unwrap();

        let mixed = vec![
            module("left-pod", ModuleAttachment::Detached, ptp(100)),
            module(
                "right-pod",
                ModuleAttachment::Docked {
                    slot_id: "right-edge".into(),
                },
                ModuleSyncStatus::docked_locked(),
            ),
        ];
        let result =
            build_pair_reconfiguration(&profile(), &planner, Some(&previous), &mixed, 2, 144_000)
                .unwrap();

        match result {
            PairReconfiguration::FailClosed { reason, handover } => {
                assert_eq!(
                    reason,
                    PairAdmissionFailure::Profile(ProfileError::TransitionInProgress)
                );
                assert_eq!(handover.commands.len(), 2);
                assert!(handover.commands.iter().all(|command| matches!(
                    command,
                    crate::EndpointHandoverCommand::Mute { .. }
                )));
            }
            PairReconfiguration::Active { .. } => panic!("mixed topology must fail closed"),
        }
    }

    #[test]
    fn detached_pair_with_bad_sync_mutes_previous_topology() {
        let planner = FabricPlanner::with_defaults();
        let docked = docked();
        let previous = planner
            .plan(&docked, &profile().resolve(&docked).unwrap().intent)
            .unwrap();

        let bad_sync = ModuleSyncStatus {
            locked: false,
            scheduled_playout: true,
            estimated_skew_micros: Some(100),
            clock_source: ModuleClockSource::Ptp,
        };
        let detached = vec![
            module("left-pod", ModuleAttachment::Detached, bad_sync),
            module("right-pod", ModuleAttachment::Detached, ptp(100)),
        ];
        let result = build_pair_reconfiguration(
            &profile(),
            &planner,
            Some(&previous),
            &detached,
            3,
            192_000,
        )
        .unwrap();

        match result {
            PairReconfiguration::FailClosed { reason, handover } => {
                assert!(matches!(
                    reason,
                    PairAdmissionFailure::Fabric(FabricError::SynchronizationInsufficient(_))
                ));
                assert_eq!(handover.commands.len(), 2);
            }
            PairReconfiguration::Active { .. } => panic!("bad sync must fail closed"),
        }
    }

    #[test]
    fn fully_detached_admitted_pair_builds_atomic_rear_handover() {
        let planner = FabricPlanner::with_defaults();
        let docked = docked();
        let previous = planner
            .plan(&docked, &profile().resolve(&docked).unwrap().intent)
            .unwrap();
        let detached = vec![
            module("left-pod", ModuleAttachment::Detached, ptp(100)),
            module("right-pod", ModuleAttachment::Detached, ptp(120)),
        ];

        let result = build_pair_reconfiguration(
            &profile(),
            &planner,
            Some(&previous),
            &detached,
            4,
            240_000,
        )
        .unwrap();

        match result {
            PairReconfiguration::Active {
                topology,
                plan,
                handover,
            } => {
                assert_eq!(topology, PairTopology::DetachedRears);
                assert_eq!(plan.assignments.len(), 2);
                assert_eq!(handover.commands.len(), 8);
                assert!(plan.assignments.iter().all(|assignment| {
                    assignment.route == crate::ModuleRoute::WirelessNetwork
                }));
            }
            PairReconfiguration::FailClosed { .. } => panic!("valid detached pair was rejected"),
        }
    }
}
