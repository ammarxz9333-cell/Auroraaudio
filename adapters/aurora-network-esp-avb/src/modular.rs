//! Modular Aurora speaker-fabric control plane.
//!
//! This module models detachable speaker endpoints independently from any one
//! enclosure, radio, amplifier or dock implementation. A physical module may
//! move between a docked soundbar role and a detached room role while Aurora
//! preserves one canonical 7.1.4 source bus.
//!
//! The planner is intentionally fail-closed for cinema mode: every canonical
//! role pair must be present exactly once, every active module must report a
//! valid synchronized clock state, and detached battery-powered modules must
//! satisfy the configured battery floor. This is a software contract only; it
//! does not claim physical wireless/dock hardware has been validated.

use std::fmt;

use crate::{AURORA_7_1_4_CHANNELS, ESP_AVB_7_1_4_ENDPOINTS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RolePair {
    Front,
    CenterLfe,
    SideSurround,
    BackSurround,
    TopFront,
    TopRear,
}

impl RolePair {
    pub const fn channels(self) -> [usize; 2] {
        match self {
            Self::Front => [0, 1],
            Self::CenterLfe => [2, 3],
            Self::SideSurround => [4, 5],
            Self::BackSurround => [6, 7],
            Self::TopFront => [8, 9],
            Self::TopRear => [10, 11],
        }
    }

    pub const fn names(self) -> [&'static str; 2] {
        match self {
            Self::Front => ["FL", "FR"],
            Self::CenterLfe => ["FC", "LFE"],
            Self::SideSurround => ["SL", "SR"],
            Self::BackSurround => ["SBL", "SBR"],
            Self::TopFront => ["TFL", "TFR"],
            Self::TopRear => ["TRL", "TRR"],
        }
    }

    pub const ALL: [Self; ESP_AVB_7_1_4_ENDPOINTS] = [
        Self::Front,
        Self::CenterLfe,
        Self::SideSurround,
        Self::BackSurround,
        Self::TopFront,
        Self::TopRear,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleAttachment {
    Docked,
    Detached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleTransport {
    DockBus,
    WiredAvbP4,
    WirelessAvbC6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleSync {
    DockClockLocked,
    NetworkClockLocked { absolute_offset_us: u32 },
    Acquiring,
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleIntent {
    Cinema(RolePair),
    StandaloneStereo,
    Multiroom,
    Muted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleObservation {
    pub module_id: String,
    pub attachment: ModuleAttachment,
    pub transport: ModuleTransport,
    pub sync: ModuleSync,
    pub battery_percent: Option<u8>,
    pub intent: ModuleIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModularFabricPolicy {
    pub max_network_offset_us: u32,
    pub minimum_detached_battery_percent: u8,
}

impl Default for ModularFabricPolicy {
    fn default() -> Self {
        Self {
            max_network_offset_us: 500,
            minimum_detached_battery_percent: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CinemaAssignment {
    pub module_id: String,
    pub attachment: ModuleAttachment,
    pub transport: ModuleTransport,
    pub role_pair: RolePair,
    pub channels: [usize; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModularCinemaPlan {
    assignments: [CinemaAssignment; ESP_AVB_7_1_4_ENDPOINTS],
}

impl ModularCinemaPlan {
    pub fn build(
        modules: &[ModuleObservation],
        policy: ModularFabricPolicy,
    ) -> Result<Self, ModularFabricError> {
        validate_policy(policy)?;

        let mut slots: [Option<CinemaAssignment>; ESP_AVB_7_1_4_ENDPOINTS] =
            std::array::from_fn(|_| None);
        let mut seen_ids: Vec<&str> = Vec::new();

        for module in modules {
            if module.module_id.is_empty() {
                return Err(ModularFabricError::InvalidModuleId);
            }
            if seen_ids.iter().any(|seen| *seen == module.module_id) {
                return Err(ModularFabricError::DuplicateModuleId);
            }
            seen_ids.push(module.module_id.as_str());

            let role_pair = match module.intent {
                ModuleIntent::Cinema(pair) => pair,
                ModuleIntent::StandaloneStereo | ModuleIntent::Multiroom | ModuleIntent::Muted => {
                    continue
                }
            };

            validate_transport_attachment(module)?;
            validate_sync(module, policy)?;
            validate_battery(module, policy)?;

            let slot = role_index(role_pair);
            if slots[slot].is_some() {
                return Err(ModularFabricError::DuplicateRolePair(role_pair));
            }

            slots[slot] = Some(CinemaAssignment {
                module_id: module.module_id.clone(),
                attachment: module.attachment,
                transport: module.transport,
                role_pair,
                channels: role_pair.channels(),
            });
        }

        for pair in RolePair::ALL {
            if slots[role_index(pair)].is_none() {
                return Err(ModularFabricError::MissingRolePair(pair));
            }
        }

        let assignments = slots.map(|slot| slot.expect("all canonical roles checked above"));
        let plan = Self { assignments };
        plan.validate_channel_coverage()?;
        Ok(plan)
    }

    pub fn assignments(&self) -> &[CinemaAssignment; ESP_AVB_7_1_4_ENDPOINTS] {
        &self.assignments
    }

    pub fn assignment_for(&self, module_id: &str) -> Option<&CinemaAssignment> {
        self.assignments
            .iter()
            .find(|assignment| assignment.module_id == module_id)
    }

    fn validate_channel_coverage(&self) -> Result<(), ModularFabricError> {
        let mut seen = [false; AURORA_7_1_4_CHANNELS];
        for assignment in &self.assignments {
            for channel in assignment.channels {
                if channel >= AURORA_7_1_4_CHANNELS || seen[channel] {
                    return Err(ModularFabricError::InvalidChannelCoverage);
                }
                seen[channel] = true;
            }
        }
        if seen.iter().any(|covered| !covered) {
            return Err(ModularFabricError::InvalidChannelCoverage);
        }
        Ok(())
    }
}

fn validate_policy(policy: ModularFabricPolicy) -> Result<(), ModularFabricError> {
    if policy.minimum_detached_battery_percent > 100 {
        return Err(ModularFabricError::InvalidPolicy);
    }
    Ok(())
}

fn validate_transport_attachment(module: &ModuleObservation) -> Result<(), ModularFabricError> {
    match (module.attachment, module.transport) {
        (ModuleAttachment::Docked, ModuleTransport::DockBus) => Ok(()),
        (ModuleAttachment::Detached, ModuleTransport::WiredAvbP4)
        | (ModuleAttachment::Detached, ModuleTransport::WirelessAvbC6) => Ok(()),
        _ => Err(ModularFabricError::TransportAttachmentMismatch),
    }
}

fn validate_sync(
    module: &ModuleObservation,
    policy: ModularFabricPolicy,
) -> Result<(), ModularFabricError> {
    match (module.transport, module.sync) {
        (ModuleTransport::DockBus, ModuleSync::DockClockLocked) => Ok(()),
        (
            ModuleTransport::WiredAvbP4 | ModuleTransport::WirelessAvbC6,
            ModuleSync::NetworkClockLocked { absolute_offset_us },
        ) if absolute_offset_us <= policy.max_network_offset_us => Ok(()),
        (_, ModuleSync::Acquiring) => Err(ModularFabricError::SyncAcquiring),
        (_, ModuleSync::Lost) => Err(ModularFabricError::SyncLost),
        _ => Err(ModularFabricError::SyncOutsidePolicy),
    }
}

fn validate_battery(
    module: &ModuleObservation,
    policy: ModularFabricPolicy,
) -> Result<(), ModularFabricError> {
    if module.attachment == ModuleAttachment::Docked {
        return Ok(());
    }

    let battery = module
        .battery_percent
        .ok_or(ModularFabricError::BatteryStateUnknown)?;
    if battery > 100 {
        return Err(ModularFabricError::InvalidBatteryState);
    }
    if battery < policy.minimum_detached_battery_percent {
        return Err(ModularFabricError::BatteryBelowPolicy);
    }
    Ok(())
}

const fn role_index(pair: RolePair) -> usize {
    match pair {
        RolePair::Front => 0,
        RolePair::CenterLfe => 1,
        RolePair::SideSurround => 2,
        RolePair::BackSurround => 3,
        RolePair::TopFront => 4,
        RolePair::TopRear => 5,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModularFabricError {
    InvalidPolicy,
    InvalidModuleId,
    DuplicateModuleId,
    DuplicateRolePair(RolePair),
    MissingRolePair(RolePair),
    TransportAttachmentMismatch,
    SyncAcquiring,
    SyncLost,
    SyncOutsidePolicy,
    BatteryStateUnknown,
    InvalidBatteryState,
    BatteryBelowPolicy,
    InvalidChannelCoverage,
}

impl fmt::Display for ModularFabricError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy => formatter.write_str("invalid modular speaker policy"),
            Self::InvalidModuleId => formatter.write_str("speaker module id must not be empty"),
            Self::DuplicateModuleId => formatter.write_str("duplicate physical speaker module id"),
            Self::DuplicateRolePair(pair) => {
                write!(formatter, "duplicate cinema role pair: {:?}", pair)
            }
            Self::MissingRolePair(pair) => write!(formatter, "missing cinema role pair: {:?}", pair),
            Self::TransportAttachmentMismatch => {
                formatter.write_str("speaker attachment and transport are incompatible")
            }
            Self::SyncAcquiring => formatter.write_str("speaker synchronization is still acquiring"),
            Self::SyncLost => formatter.write_str("speaker synchronization was lost"),
            Self::SyncOutsidePolicy => {
                formatter.write_str("speaker synchronization is outside policy")
            }
            Self::BatteryStateUnknown => {
                formatter.write_str("detached speaker battery state is unknown")
            }
            Self::InvalidBatteryState => formatter.write_str("invalid speaker battery state"),
            Self::BatteryBelowPolicy => {
                formatter.write_str("detached speaker battery is below cinema policy")
            }
            Self::InvalidChannelCoverage => {
                formatter.write_str("modular cinema plan does not cover canonical 7.1.4 exactly")
            }
        }
    }
}

impl std::error::Error for ModularFabricError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(
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

    fn mixed_soundbar_and_detached_rears() -> Vec<ModuleObservation> {
        vec![
            module(
                "front",
                RolePair::Front,
                ModuleAttachment::Docked,
                ModuleTransport::DockBus,
            ),
            module(
                "center-sub",
                RolePair::CenterLfe,
                ModuleAttachment::Docked,
                ModuleTransport::DockBus,
            ),
            module(
                "side",
                RolePair::SideSurround,
                ModuleAttachment::Detached,
                ModuleTransport::WirelessAvbC6,
            ),
            module(
                "back",
                RolePair::BackSurround,
                ModuleAttachment::Detached,
                ModuleTransport::WirelessAvbC6,
            ),
            module(
                "top-front",
                RolePair::TopFront,
                ModuleAttachment::Docked,
                ModuleTransport::DockBus,
            ),
            module(
                "top-rear",
                RolePair::TopRear,
                ModuleAttachment::Detached,
                ModuleTransport::WirelessAvbC6,
            ),
        ]
    }

    #[test]
    fn mixed_docked_and_detached_modules_form_exact_7_1_4_plan() {
        let modules = mixed_soundbar_and_detached_rears();
        let plan = ModularCinemaPlan::build(&modules, ModularFabricPolicy::default()).unwrap();

        assert_eq!(plan.assignments().len(), 6);
        assert_eq!(plan.assignments()[0].role_pair, RolePair::Front);
        assert_eq!(plan.assignments()[0].channels, [0, 1]);
        assert_eq!(plan.assignments()[5].role_pair, RolePair::TopRear);
        assert_eq!(plan.assignments()[5].channels, [10, 11]);
        assert_eq!(
            plan.assignment_for("back").unwrap().transport,
            ModuleTransport::WirelessAvbC6
        );
    }

    #[test]
    fn same_physical_module_can_be_reassigned_after_detach() {
        let mut modules = mixed_soundbar_and_detached_rears();
        modules[0].attachment = ModuleAttachment::Detached;
        modules[0].transport = ModuleTransport::WirelessAvbC6;
        modules[0].sync = ModuleSync::NetworkClockLocked {
            absolute_offset_us: 80,
        };
        modules[0].battery_percent = Some(70);

        modules[0].intent = ModuleIntent::Cinema(RolePair::SideSurround);
        modules[2].intent = ModuleIntent::Cinema(RolePair::Front);

        let plan = ModularCinemaPlan::build(&modules, ModularFabricPolicy::default()).unwrap();
        assert_eq!(
            plan.assignment_for("front").unwrap().role_pair,
            RolePair::SideSurround
        );
        assert_eq!(
            plan.assignment_for("side").unwrap().role_pair,
            RolePair::Front
        );
    }

    #[test]
    fn cinema_fails_closed_when_wireless_clock_is_not_locked() {
        let mut modules = mixed_soundbar_and_detached_rears();
        modules[3].sync = ModuleSync::Acquiring;
        assert_eq!(
            ModularCinemaPlan::build(&modules, ModularFabricPolicy::default()),
            Err(ModularFabricError::SyncAcquiring)
        );

        modules[3].sync = ModuleSync::NetworkClockLocked {
            absolute_offset_us: 501,
        };
        assert_eq!(
            ModularCinemaPlan::build(&modules, ModularFabricPolicy::default()),
            Err(ModularFabricError::SyncOutsidePolicy)
        );
    }

    #[test]
    fn detached_module_requires_battery_floor() {
        let mut modules = mixed_soundbar_and_detached_rears();
        modules[2].battery_percent = Some(9);
        assert_eq!(
            ModularCinemaPlan::build(&modules, ModularFabricPolicy::default()),
            Err(ModularFabricError::BatteryBelowPolicy)
        );
    }

    #[test]
    fn missing_or_duplicate_cinema_roles_are_rejected() {
        let mut modules = mixed_soundbar_and_detached_rears();
        modules[5].intent = ModuleIntent::Muted;
        assert_eq!(
            ModularCinemaPlan::build(&modules, ModularFabricPolicy::default()),
            Err(ModularFabricError::MissingRolePair(RolePair::TopRear))
        );

        let mut modules = mixed_soundbar_and_detached_rears();
        modules[5].intent = ModuleIntent::Cinema(RolePair::TopFront);
        assert_eq!(
            ModularCinemaPlan::build(&modules, ModularFabricPolicy::default()),
            Err(ModularFabricError::DuplicateRolePair(RolePair::TopFront))
        );
    }

    #[test]
    fn docked_and_detached_transport_rules_are_explicit() {
        let mut modules = mixed_soundbar_and_detached_rears();
        modules[0].transport = ModuleTransport::WirelessAvbC6;
        modules[0].sync = ModuleSync::NetworkClockLocked {
            absolute_offset_us: 50,
        };
        assert_eq!(
            ModularCinemaPlan::build(&modules, ModularFabricPolicy::default()),
            Err(ModularFabricError::TransportAttachmentMismatch)
        );
    }

    #[test]
    fn portable_and_multiroom_modules_can_coexist_without_stealing_cinema_roles() {
        let mut modules = mixed_soundbar_and_detached_rears();
        modules.push(ModuleObservation {
            module_id: "portable-kitchen".to_owned(),
            attachment: ModuleAttachment::Detached,
            transport: ModuleTransport::WirelessAvbC6,
            sync: ModuleSync::Lost,
            battery_percent: Some(40),
            intent: ModuleIntent::Multiroom,
        });
        modules.push(ModuleObservation {
            module_id: "portable-stereo".to_owned(),
            attachment: ModuleAttachment::Detached,
            transport: ModuleTransport::WirelessAvbC6,
            sync: ModuleSync::Lost,
            battery_percent: Some(50),
            intent: ModuleIntent::StandaloneStereo,
        });

        assert!(ModularCinemaPlan::build(&modules, ModularFabricPolicy::default()).is_ok());
    }
}
