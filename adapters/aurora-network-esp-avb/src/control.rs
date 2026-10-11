//! Host-side control bridge from Aurora modular handovers to ESP-AVB endpoints.
//!
//! This module does not implement an ESP-IDF RPC or network socket. It validates
//! the detachable-speaker handover against the current ESP-AVB endpoint contract
//! and produces deterministic endpoint envelopes for a board-specific control
//! transport to encode/send later.

use std::collections::BTreeMap;
use std::fmt;

use aurora_modular_speaker_fabric::{
    endpoint::EndpointCommand, EndpointHandoverCommand, HandoverPlan, ModuleClockSource,
    ModuleRoute,
};

use crate::{EspAvbEndpointMedium, ESP_AVB_CHANNELS_PER_STREAM};

/// Stable association between one Aurora detachable module and one ESP-AVB endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EspAvbModuleBinding {
    pub module_id: String,
    pub endpoint_id: String,
    pub stream_id: u64,
    pub medium: EspAvbEndpointMedium,
}

/// Validated host-side envelope ready for a future ESP control transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EspAvbControlEnvelope {
    pub endpoint_id: String,
    pub stream_id: u64,
    pub medium: EspAvbEndpointMedium,
    pub command: EndpointCommand,
}

/// Fail-closed control-bridge errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EspAvbControlError {
    InvalidBinding,
    DuplicateModule,
    DuplicateEndpoint,
    DuplicateStream,
    UnknownModule,
    InvalidEpoch,
    InvalidStartFrame,
    InvalidCommandOrder,
    UnsupportedRoute,
    UnsupportedClock,
    UnsupportedLaneCount,
    MediumRouteMismatch,
}

impl fmt::Display for EspAvbControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBinding => formatter.write_str("invalid ESP-AVB module binding"),
            Self::DuplicateModule => formatter.write_str("duplicate ESP-AVB module binding"),
            Self::DuplicateEndpoint => formatter.write_str("duplicate ESP-AVB endpoint binding"),
            Self::DuplicateStream => formatter.write_str("duplicate ESP-AVB stream binding"),
            Self::UnknownModule => formatter.write_str("handover targets an unbound ESP-AVB module"),
            Self::InvalidEpoch => formatter.write_str("ESP-AVB handover epoch is invalid"),
            Self::InvalidStartFrame => {
                formatter.write_str("ESP-AVB handover start frame is invalid")
            }
            Self::InvalidCommandOrder => {
                formatter.write_str("ESP-AVB handover command order is invalid")
            }
            Self::UnsupportedRoute => {
                formatter.write_str("route is unsupported by the ESP-AVB control bridge")
            }
            Self::UnsupportedClock => {
                formatter.write_str("clock source is unsupported by the ESP-AVB control bridge")
            }
            Self::UnsupportedLaneCount => {
                formatter.write_str("role count exceeds the ESP-AVB stereo endpoint contract")
            }
            Self::MediumRouteMismatch => {
                formatter.write_str("ESP-AVB endpoint medium does not match the requested route")
            }
        }
    }
}

impl std::error::Error for EspAvbControlError {}

/// Compile one atomic modular-speaker handover into endpoint-addressed ESP-AVB
/// control envelopes while preserving Aurora's global mute -> prepare -> arm ->
/// commit ordering.
///
/// This bridge intentionally accepts only PTP-disciplined network routes. Dock
/// PCM stays local to the soundbar dock and AdaptivePeer belongs to a different
/// transport family.
pub fn compile_handover_control(
    handover: &HandoverPlan,
    bindings: &[EspAvbModuleBinding],
) -> Result<Vec<EspAvbControlEnvelope>, EspAvbControlError> {
    if handover.epoch == 0 {
        return Err(EspAvbControlError::InvalidEpoch);
    }
    if handover.start_frame == 0 {
        return Err(EspAvbControlError::InvalidStartFrame);
    }

    validate_bindings(bindings)?;

    let mut phases = BTreeMap::<&str, u8>::new();
    let mut envelopes = Vec::with_capacity(handover.commands.len());

    for command in &handover.commands {
        let (module_id, epoch) = handover_identity(command);
        if epoch != handover.epoch {
            return Err(EspAvbControlError::InvalidEpoch);
        }

        let binding = bindings
            .iter()
            .find(|candidate| candidate.module_id == module_id)
            .ok_or(EspAvbControlError::UnknownModule)?;

        let endpoint_command = match command {
            EndpointHandoverCommand::Mute { module_id, epoch } => {
                transition(&mut phases, module_id, 0, 1)?;
                EndpointCommand::Mute {
                    module_id: module_id.clone(),
                    epoch: *epoch,
                }
            }
            EndpointHandoverCommand::Prepare { assignment, epoch } => {
                transition(&mut phases, &assignment.module_id, 1, 2)?;
                validate_assignment(binding, assignment.route, assignment.clock_source, assignment.roles.len())?;
                EndpointCommand::Prepare {
                    module_id: assignment.module_id.clone(),
                    epoch: *epoch,
                    route: assignment.route,
                    clock_source: assignment.clock_source,
                    roles: assignment.roles.clone(),
                }
            }
            EndpointHandoverCommand::Arm {
                module_id,
                epoch,
                start_frame,
            } => {
                transition(&mut phases, module_id, 2, 3)?;
                if *start_frame == 0 || *start_frame != handover.start_frame {
                    return Err(EspAvbControlError::InvalidStartFrame);
                }
                EndpointCommand::Arm {
                    module_id: module_id.clone(),
                    epoch: *epoch,
                    start_frame: *start_frame,
                }
            }
            EndpointHandoverCommand::Commit { module_id, epoch } => {
                transition(&mut phases, module_id, 3, 4)?;
                EndpointCommand::Commit {
                    module_id: module_id.clone(),
                    epoch: *epoch,
                }
            }
        };

        envelopes.push(EspAvbControlEnvelope {
            endpoint_id: binding.endpoint_id.clone(),
            stream_id: binding.stream_id,
            medium: binding.medium,
            command: endpoint_command,
        });
    }

    Ok(envelopes)
}

fn validate_bindings(bindings: &[EspAvbModuleBinding]) -> Result<(), EspAvbControlError> {
    for (index, binding) in bindings.iter().enumerate() {
        if binding.module_id.is_empty() || binding.endpoint_id.is_empty() || binding.stream_id == 0 {
            return Err(EspAvbControlError::InvalidBinding);
        }
        for other in bindings.iter().skip(index + 1) {
            if binding.module_id == other.module_id {
                return Err(EspAvbControlError::DuplicateModule);
            }
            if binding.endpoint_id == other.endpoint_id {
                return Err(EspAvbControlError::DuplicateEndpoint);
            }
            if binding.stream_id == other.stream_id {
                return Err(EspAvbControlError::DuplicateStream);
            }
        }
    }
    Ok(())
}

fn validate_assignment(
    binding: &EspAvbModuleBinding,
    route: ModuleRoute,
    clock_source: ModuleClockSource,
    lane_count: usize,
) -> Result<(), EspAvbControlError> {
    if lane_count == 0 || lane_count > ESP_AVB_CHANNELS_PER_STREAM {
        return Err(EspAvbControlError::UnsupportedLaneCount);
    }
    if clock_source != ModuleClockSource::Ptp {
        return Err(EspAvbControlError::UnsupportedClock);
    }

    match (route, binding.medium) {
        (ModuleRoute::WiredNetwork, EspAvbEndpointMedium::WiredEsp32P4)
        | (ModuleRoute::WirelessNetwork, EspAvbEndpointMedium::WirelessEsp32C6) => Ok(()),
        (ModuleRoute::WiredNetwork | ModuleRoute::WirelessNetwork, _) => {
            Err(EspAvbControlError::MediumRouteMismatch)
        }
        (ModuleRoute::DockBus | ModuleRoute::Muted, _) => Err(EspAvbControlError::UnsupportedRoute),
    }
}

fn transition<'a>(
    phases: &mut BTreeMap<&'a str, u8>,
    module_id: &'a str,
    expected: u8,
    next: u8,
) -> Result<(), EspAvbControlError> {
    let current = phases.get(module_id).copied().unwrap_or(0);
    if current != expected {
        return Err(EspAvbControlError::InvalidCommandOrder);
    }
    phases.insert(module_id, next);
    Ok(())
}

fn handover_identity(command: &EndpointHandoverCommand) -> (&str, u64) {
    match command {
        EndpointHandoverCommand::Mute { module_id, epoch }
        | EndpointHandoverCommand::Arm {
            module_id, epoch, ..
        }
        | EndpointHandoverCommand::Commit { module_id, epoch } => (module_id, *epoch),
        EndpointHandoverCommand::Prepare { assignment, epoch } => (&assignment.module_id, *epoch),
    }
}

#[cfg(test)]
mod tests {
    use aurora_core::ChannelRole;
    use aurora_modular_speaker_fabric::{ModuleAssignment, ModuleClockSource};

    use super::*;

    fn binding(module_id: &str, endpoint_id: &str, stream_id: u64) -> EspAvbModuleBinding {
        EspAvbModuleBinding {
            module_id: module_id.to_owned(),
            endpoint_id: endpoint_id.to_owned(),
            stream_id,
            medium: EspAvbEndpointMedium::WirelessEsp32C6,
        }
    }

    fn two_pod_handover() -> HandoverPlan {
        HandoverPlan {
            epoch: 42,
            start_frame: 96_000,
            commands: vec![
                EndpointHandoverCommand::Mute {
                    module_id: "left-pod".to_owned(),
                    epoch: 42,
                },
                EndpointHandoverCommand::Mute {
                    module_id: "right-pod".to_owned(),
                    epoch: 42,
                },
                EndpointHandoverCommand::Prepare {
                    assignment: ModuleAssignment {
                        module_id: "left-pod".to_owned(),
                        route: ModuleRoute::WirelessNetwork,
                        clock_source: ModuleClockSource::Ptp,
                        roles: vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft],
                    },
                    epoch: 42,
                },
                EndpointHandoverCommand::Prepare {
                    assignment: ModuleAssignment {
                        module_id: "right-pod".to_owned(),
                        route: ModuleRoute::WirelessNetwork,
                        clock_source: ModuleClockSource::Ptp,
                        roles: vec![ChannelRole::SurroundRight, ChannelRole::TopRearRight],
                    },
                    epoch: 42,
                },
                EndpointHandoverCommand::Arm {
                    module_id: "left-pod".to_owned(),
                    epoch: 42,
                    start_frame: 96_000,
                },
                EndpointHandoverCommand::Arm {
                    module_id: "right-pod".to_owned(),
                    epoch: 42,
                    start_frame: 96_000,
                },
                EndpointHandoverCommand::Commit {
                    module_id: "left-pod".to_owned(),
                    epoch: 42,
                },
                EndpointHandoverCommand::Commit {
                    module_id: "right-pod".to_owned(),
                    epoch: 42,
                },
            ],
        }
    }

    #[test]
    fn two_wireless_atmos_pods_compile_to_one_shared_epoch_and_start_frame() {
        let bindings = [
            binding("left-pod", "esp-c6-left", 0xA700_0010),
            binding("right-pod", "esp-c6-right", 0xA700_0011),
        ];
        let messages = compile_handover_control(&two_pod_handover(), &bindings).unwrap();
        assert_eq!(messages.len(), 8);
        assert!(matches!(
            messages[4].command,
            EndpointCommand::Arm {
                epoch: 42,
                start_frame: 96_000,
                ..
            }
        ));
        assert!(matches!(
            messages[5].command,
            EndpointCommand::Arm {
                epoch: 42,
                start_frame: 96_000,
                ..
            }
        ));
        assert_eq!(messages[0].endpoint_id, "esp-c6-left");
        assert_eq!(messages[1].endpoint_id, "esp-c6-right");
    }

    #[test]
    fn adaptive_peer_is_rejected_from_ptp_only_esp_avb_bridge() {
        let mut handover = two_pod_handover();
        if let EndpointHandoverCommand::Prepare { assignment, .. } = &mut handover.commands[2] {
            assignment.clock_source = ModuleClockSource::AdaptivePeer;
        }
        let bindings = [
            binding("left-pod", "esp-c6-left", 1),
            binding("right-pod", "esp-c6-right", 2),
        ];
        assert_eq!(
            compile_handover_control(&handover, &bindings),
            Err(EspAvbControlError::UnsupportedClock)
        );
    }

    #[test]
    fn dock_bus_is_not_smuggled_into_network_control() {
        let mut handover = two_pod_handover();
        if let EndpointHandoverCommand::Prepare { assignment, .. } = &mut handover.commands[2] {
            assignment.route = ModuleRoute::DockBus;
        }
        let bindings = [
            binding("left-pod", "esp-c6-left", 1),
            binding("right-pod", "esp-c6-right", 2),
        ];
        assert_eq!(
            compile_handover_control(&handover, &bindings),
            Err(EspAvbControlError::UnsupportedRoute)
        );
    }

    #[test]
    fn wired_route_requires_the_wired_p4_medium() {
        let mut handover = two_pod_handover();
        if let EndpointHandoverCommand::Prepare { assignment, .. } = &mut handover.commands[2] {
            assignment.route = ModuleRoute::WiredNetwork;
        }
        let bindings = [
            binding("left-pod", "esp-c6-left", 1),
            binding("right-pod", "esp-c6-right", 2),
        ];
        assert_eq!(
            compile_handover_control(&handover, &bindings),
            Err(EspAvbControlError::MediumRouteMismatch)
        );
    }

    #[test]
    fn malformed_order_and_mismatched_start_frame_fail_closed() {
        let mut handover = two_pod_handover();
        handover.commands.swap(0, 2);
        let bindings = [
            binding("left-pod", "esp-c6-left", 1),
            binding("right-pod", "esp-c6-right", 2),
        ];
        assert_eq!(
            compile_handover_control(&handover, &bindings),
            Err(EspAvbControlError::InvalidCommandOrder)
        );

        let mut handover = two_pod_handover();
        if let EndpointHandoverCommand::Arm { start_frame, .. } = &mut handover.commands[4] {
            *start_frame = 96_001;
        }
        assert_eq!(
            compile_handover_control(&handover, &bindings),
            Err(EspAvbControlError::InvalidStartFrame)
        );
    }

    #[test]
    fn duplicate_endpoint_identity_is_rejected_before_compilation() {
        let bindings = [
            binding("left-pod", "same-endpoint", 1),
            binding("right-pod", "same-endpoint", 2),
        ];
        assert_eq!(
            compile_handover_control(&two_pod_handover(), &bindings),
            Err(EspAvbControlError::DuplicateEndpoint)
        );
    }
}
