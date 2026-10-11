//! Standard AECP/AEM SET_CONTROL program for Aurora detachable endpoints.
//!
//! The exact pinned esp_avb component already implements IEEE 1722.1
//! SET_CONTROL and GET_CONTROL. This module maps Aurora endpoint commands
//! onto scalar AEM control writes and converts Aurora media-frame time to
//! absolute PTP nanoseconds before a physical endpoint is armed.

use std::fmt;

use aurora_core::ChannelRole;
use aurora_modular_speaker_fabric::{
    endpoint::EndpointCommand, ModuleClockSource, ModuleRoute,
};
use aurora_realtime_audio_api::AURORA_NETWORK_MEDIA_RATE;

use crate::EspAvbControlEnvelope;

/// First Aurora-reserved AEM CONTROL descriptor index.
///
/// Exact pinned esp_avb owns 0=IDENTIFY, 1=Speaker Volume, 2=Mic Gain.
pub const AURORA_AEM_CONTROL_EPOCH: u16 = 3;
pub const AURORA_AEM_CONTROL_ROLE0: u16 = 4;
pub const AURORA_AEM_CONTROL_ROLE1: u16 = 5;
pub const AURORA_AEM_CONTROL_START_PTP_NS: u16 = 6;
pub const AURORA_AEM_CONTROL_COMMAND: u16 = 7;

pub const AURORA_AEM_COMMAND_MUTE: u8 = 1;
pub const AURORA_AEM_COMMAND_PREPARE: u8 = 2;
pub const AURORA_AEM_COMMAND_ARM: u8 = 3;
pub const AURORA_AEM_COMMAND_COMMIT: u8 = 4;
pub const AURORA_AEM_COMMAND_STOP: u8 = 5;
pub const AURORA_AEM_ROLE_UNUSED: u8 = 0xff;

/// Runtime AVDECC identity resolved for a logical ESP endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EspAvbAemTarget {
    pub endpoint_id: String,
    pub entity_id: [u8; 8],
}

/// One known correspondence between the Aurora media timeline and PTP time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtpMediaAnchor {
    pub frame_index: u64,
    pub ptp_time_ns: u64,
    pub sample_rate: u32,
}

impl PtpMediaAnchor {
    /// Maps a future Aurora media frame to nearest-integer PTP nanoseconds.
    pub fn ptp_time_for_frame(
        self,
        frame_index: u64,
    ) -> Result<u64, EspAvbAemControlError> {
        if self.sample_rate != AURORA_NETWORK_MEDIA_RATE {
            return Err(EspAvbAemControlError::InvalidMediaAnchor);
        }
        let delta_frames = frame_index
            .checked_sub(self.frame_index)
            .ok_or(EspAvbAemControlError::FrameBeforeAnchor)?;
        let numerator = u128::from(delta_frames)
            .checked_mul(1_000_000_000)
            .ok_or(EspAvbAemControlError::TimestampOverflow)?;
        let rounded_ns = numerator
            .checked_add(u128::from(self.sample_rate / 2))
            .ok_or(EspAvbAemControlError::TimestampOverflow)?
            / u128::from(self.sample_rate);
        let rounded_ns =
            u64::try_from(rounded_ns).map_err(|_| EspAvbAemControlError::TimestampOverflow)?;
        self.ptp_time_ns
            .checked_add(rounded_ns)
            .ok_or(EspAvbAemControlError::TimestampOverflow)
    }
}

/// Scalar type expected by the matching AEM CONTROL descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AemControlScalarType {
    LinearUint8,
    LinearUint64,
}

/// One future AVDECC SET_CONTROL operation.
///
/// Integer values are encoded in network byte order in the first value_len bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AemSetControlWrite {
    pub endpoint_id: String,
    pub target_entity_id: [u8; 8],
    pub stream_id: u64,
    pub control_index: u16,
    pub scalar_type: AemControlScalarType,
    pub value: [u8; 8],
    pub value_len: u8,
}

impl AemSetControlWrite {
    fn uint8(
        envelope: &EspAvbControlEnvelope,
        target_entity_id: [u8; 8],
        control_index: u16,
        value: u8,
    ) -> Self {
        let mut bytes = [0_u8; 8];
        bytes[0] = value;
        Self {
            endpoint_id: envelope.endpoint_id.clone(),
            target_entity_id,
            stream_id: envelope.stream_id,
            control_index,
            scalar_type: AemControlScalarType::LinearUint8,
            value: bytes,
            value_len: 1,
        }
    }

    fn uint64(
        envelope: &EspAvbControlEnvelope,
        target_entity_id: [u8; 8],
        control_index: u16,
        value: u64,
    ) -> Self {
        Self {
            endpoint_id: envelope.endpoint_id.clone(),
            target_entity_id,
            stream_id: envelope.stream_id,
            control_index,
            scalar_type: AemControlScalarType::LinearUint64,
            value: value.to_be_bytes(),
            value_len: 8,
        }
    }

    pub fn value_u64(&self) -> Option<u64> {
        (self.value_len == 8).then(|| u64::from_be_bytes(self.value))
    }

    pub fn value_u8(&self) -> Option<u8> {
        (self.value_len == 1).then_some(self.value[0])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EspAvbAemControlError {
    InvalidMediaAnchor,
    FrameBeforeAnchor,
    TimestampOverflow,
    EmptyTarget,
    ZeroEntityId,
    DuplicateTarget,
    UnknownTarget,
    InvalidEpoch,
    UnsupportedRoute,
    UnsupportedClock,
    UnsupportedRole,
    UnsupportedLaneCount,
}

impl fmt::Display for EspAvbAemControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMediaAnchor => formatter.write_str("invalid Aurora/PTP media anchor"),
            Self::FrameBeforeAnchor => formatter.write_str("handover frame predates PTP anchor"),
            Self::TimestampOverflow => formatter.write_str("PTP activation timestamp overflow"),
            Self::EmptyTarget => formatter.write_str("empty AVDECC endpoint target"),
            Self::ZeroEntityId => formatter.write_str("zero AVDECC entity id is not routable"),
            Self::DuplicateTarget => formatter.write_str("duplicate AVDECC endpoint/entity target"),
            Self::UnknownTarget => formatter.write_str("ESP endpoint has no AVDECC target"),
            Self::InvalidEpoch => formatter.write_str("Aurora endpoint epoch must be non-zero"),
            Self::UnsupportedRoute => {
                formatter.write_str("AEM control program requires an ESP network route")
            }
            Self::UnsupportedClock => {
                formatter.write_str("AEM control program requires PTP clock discipline")
            }
            Self::UnsupportedRole => formatter.write_str("role has no stable Aurora AEM code"),
            Self::UnsupportedLaneCount => {
                formatter.write_str("AEM endpoint supports one or two rendered roles")
            }
        }
    }
}

impl std::error::Error for EspAvbAemControlError {}

/// Translates validated endpoint envelopes into standard AEM SET_CONTROL writes.
///
/// The caller later assigns AECP sequence IDs, sends the packets, requires
/// success responses, and aborts the whole handover if any write is not ACKed.
pub fn compile_aem_control_program(
    envelopes: &[EspAvbControlEnvelope],
    targets: &[EspAvbAemTarget],
    anchor: PtpMediaAnchor,
) -> Result<Vec<AemSetControlWrite>, EspAvbAemControlError> {
    if anchor.sample_rate != AURORA_NETWORK_MEDIA_RATE {
        return Err(EspAvbAemControlError::InvalidMediaAnchor);
    }
    validate_targets(targets)?;

    let mut writes = Vec::with_capacity(envelopes.len() * 4);
    for envelope in envelopes {
        let target = targets
            .iter()
            .find(|target| target.endpoint_id == envelope.endpoint_id)
            .ok_or(EspAvbAemControlError::UnknownTarget)?;
        append_command_writes(&mut writes, envelope, target.entity_id, anchor)?;
    }
    Ok(writes)
}

fn append_command_writes(
    writes: &mut Vec<AemSetControlWrite>,
    envelope: &EspAvbControlEnvelope,
    entity_id: [u8; 8],
    anchor: PtpMediaAnchor,
) -> Result<(), EspAvbAemControlError> {
    let (epoch, command_code) = match &envelope.command {
        EndpointCommand::Mute { epoch, .. } => (*epoch, AURORA_AEM_COMMAND_MUTE),
        EndpointCommand::Prepare {
            epoch,
            route,
            clock_source,
            roles,
            ..
        } => {
            validate_prepare(*route, *clock_source, roles)?;
            (*epoch, AURORA_AEM_COMMAND_PREPARE)
        }
        EndpointCommand::Arm { epoch, .. } => (*epoch, AURORA_AEM_COMMAND_ARM),
        EndpointCommand::Commit { epoch, .. } => (*epoch, AURORA_AEM_COMMAND_COMMIT),
        EndpointCommand::Stop { epoch, .. } => (*epoch, AURORA_AEM_COMMAND_STOP),
    };
    if epoch == 0 {
        return Err(EspAvbAemControlError::InvalidEpoch);
    }

    // Stage the epoch before every command. The endpoint applies no transition
    // until COMMAND is written, so an interrupted AECP sequence does not execute
    // a partial handover.
    writes.push(AemSetControlWrite::uint64(
        envelope,
        entity_id,
        AURORA_AEM_CONTROL_EPOCH,
        epoch,
    ));

    match &envelope.command {
        EndpointCommand::Prepare { roles, .. } => {
            let role0 = encode_role(&roles[0])?;
            let role1 = if roles.len() == 2 {
                encode_role(&roles[1])?
            } else {
                AURORA_AEM_ROLE_UNUSED
            };
            writes.push(AemSetControlWrite::uint8(
                envelope,
                entity_id,
                AURORA_AEM_CONTROL_ROLE0,
                role0,
            ));
            writes.push(AemSetControlWrite::uint8(
                envelope,
                entity_id,
                AURORA_AEM_CONTROL_ROLE1,
                role1,
            ));
        }
        EndpointCommand::Arm { start_frame, .. } => {
            let start_ptp_ns = anchor.ptp_time_for_frame(*start_frame)?;
            writes.push(AemSetControlWrite::uint64(
                envelope,
                entity_id,
                AURORA_AEM_CONTROL_START_PTP_NS,
                start_ptp_ns,
            ));
        }
        EndpointCommand::Mute { .. }
        | EndpointCommand::Commit { .. }
        | EndpointCommand::Stop { .. } => {}
    }

    writes.push(AemSetControlWrite::uint8(
        envelope,
        entity_id,
        AURORA_AEM_CONTROL_COMMAND,
        command_code,
    ));
    Ok(())
}

fn validate_prepare(
    route: ModuleRoute,
    clock_source: ModuleClockSource,
    roles: &[ChannelRole],
) -> Result<(), EspAvbAemControlError> {
    if !matches!(route, ModuleRoute::WiredNetwork | ModuleRoute::WirelessNetwork) {
        return Err(EspAvbAemControlError::UnsupportedRoute);
    }
    if clock_source != ModuleClockSource::Ptp {
        return Err(EspAvbAemControlError::UnsupportedClock);
    }
    if roles.is_empty() || roles.len() > 2 {
        return Err(EspAvbAemControlError::UnsupportedLaneCount);
    }
    for role in roles {
        encode_role(role)?;
    }
    Ok(())
}

fn validate_targets(targets: &[EspAvbAemTarget]) -> Result<(), EspAvbAemControlError> {
    for (index, target) in targets.iter().enumerate() {
        if target.endpoint_id.is_empty() {
            return Err(EspAvbAemControlError::EmptyTarget);
        }
        if target.entity_id == [0; 8] {
            return Err(EspAvbAemControlError::ZeroEntityId);
        }
        for other in targets.iter().skip(index + 1) {
            if target.endpoint_id == other.endpoint_id || target.entity_id == other.entity_id {
                return Err(EspAvbAemControlError::DuplicateTarget);
            }
        }
    }
    Ok(())
}

fn encode_role(role: &ChannelRole) -> Result<u8, EspAvbAemControlError> {
    match role {
        ChannelRole::FrontLeft => Ok(0),
        ChannelRole::FrontRight => Ok(1),
        ChannelRole::FrontCenter => Ok(2),
        ChannelRole::LowFrequencyEffects => Ok(3),
        ChannelRole::SurroundLeft => Ok(4),
        ChannelRole::SurroundRight => Ok(5),
        ChannelRole::SurroundBackLeft => Ok(6),
        ChannelRole::SurroundBackRight => Ok(7),
        ChannelRole::TopFrontLeft => Ok(8),
        ChannelRole::TopFrontRight => Ok(9),
        ChannelRole::TopRearLeft => Ok(10),
        ChannelRole::TopRearRight => Ok(11),
        ChannelRole::Custom(_) => Err(EspAvbAemControlError::UnsupportedRole),
    }
}

#[cfg(test)]
mod tests {
    use aurora_modular_speaker_fabric::endpoint::EndpointCommand;

    use crate::{EspAvbControlEnvelope, EspAvbEndpointMedium};

    use super::*;

    fn envelope(command: EndpointCommand) -> EspAvbControlEnvelope {
        EspAvbControlEnvelope {
            endpoint_id: "esp-c6-left".to_owned(),
            stream_id: 0xA700_0010,
            medium: EspAvbEndpointMedium::WirelessEsp32C6,
            command,
        }
    }

    fn target() -> EspAvbAemTarget {
        EspAvbAemTarget {
            endpoint_id: "esp-c6-left".to_owned(),
            entity_id: [0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x00, 0x01],
        }
    }

    fn anchor() -> PtpMediaAnchor {
        PtpMediaAnchor {
            frame_index: 48_000,
            ptp_time_ns: 1_000_000_000,
            sample_rate: 48_000,
        }
    }

    #[test]
    fn frame_to_ptp_mapping_is_deterministic_and_rounded_to_nearest_ns() {
        assert_eq!(anchor().ptp_time_for_frame(96_000).unwrap(), 2_000_000_000);
        assert_eq!(anchor().ptp_time_for_frame(48_001).unwrap(), 1_000_020_833);
        assert_eq!(
            anchor().ptp_time_for_frame(47_999),
            Err(EspAvbAemControlError::FrameBeforeAnchor)
        );
    }

    #[test]
    fn arm_writes_epoch_then_absolute_ptp_time_then_command() {
        let envelopes = [envelope(EndpointCommand::Arm {
            module_id: "left-pod".to_owned(),
            epoch: 42,
            start_frame: 96_000,
        })];
        let writes = compile_aem_control_program(&envelopes, &[target()], anchor()).unwrap();
        assert_eq!(writes.len(), 3);
        assert_eq!(writes[0].control_index, AURORA_AEM_CONTROL_EPOCH);
        assert_eq!(writes[0].value_u64(), Some(42));
        assert_eq!(writes[1].control_index, AURORA_AEM_CONTROL_START_PTP_NS);
        assert_eq!(writes[1].value_u64(), Some(2_000_000_000));
        assert_eq!(writes[2].control_index, AURORA_AEM_CONTROL_COMMAND);
        assert_eq!(writes[2].value_u8(), Some(AURORA_AEM_COMMAND_ARM));
    }

    #[test]
    fn prepare_encodes_two_atmos_roles_in_canonical_role_codes() {
        let envelopes = [envelope(EndpointCommand::Prepare {
            module_id: "left-pod".to_owned(),
            epoch: 42,
            route: ModuleRoute::WirelessNetwork,
            clock_source: ModuleClockSource::Ptp,
            roles: vec![ChannelRole::SurroundLeft, ChannelRole::TopRearLeft],
        })];
        let writes = compile_aem_control_program(&envelopes, &[target()], anchor()).unwrap();
        assert_eq!(writes.len(), 4);
        assert_eq!(writes[1].control_index, AURORA_AEM_CONTROL_ROLE0);
        assert_eq!(writes[1].value_u8(), Some(4));
        assert_eq!(writes[2].control_index, AURORA_AEM_CONTROL_ROLE1);
        assert_eq!(writes[2].value_u8(), Some(10));
        assert_eq!(writes[3].value_u8(), Some(AURORA_AEM_COMMAND_PREPARE));
    }

    #[test]
    fn single_role_uses_explicit_unused_second_lane() {
        let envelopes = [envelope(EndpointCommand::Prepare {
            module_id: "left-pod".to_owned(),
            epoch: 2,
            route: ModuleRoute::WirelessNetwork,
            clock_source: ModuleClockSource::Ptp,
            roles: vec![ChannelRole::SurroundLeft],
        })];
        let writes = compile_aem_control_program(&envelopes, &[target()], anchor()).unwrap();
        assert_eq!(writes[2].value_u8(), Some(AURORA_AEM_ROLE_UNUSED));
    }

    #[test]
    fn custom_role_and_non_ptp_prepare_fail_closed() {
        let custom = [envelope(EndpointCommand::Prepare {
            module_id: "left-pod".to_owned(),
            epoch: 2,
            route: ModuleRoute::WirelessNetwork,
            clock_source: ModuleClockSource::Ptp,
            roles: vec![ChannelRole::Custom("mystery".to_owned())],
        })];
        assert_eq!(
            compile_aem_control_program(&custom, &[target()], anchor()),
            Err(EspAvbAemControlError::UnsupportedRole)
        );

        let adaptive = [envelope(EndpointCommand::Prepare {
            module_id: "left-pod".to_owned(),
            epoch: 2,
            route: ModuleRoute::WirelessNetwork,
            clock_source: ModuleClockSource::AdaptivePeer,
            roles: vec![ChannelRole::SurroundLeft],
        })];
        assert_eq!(
            compile_aem_control_program(&adaptive, &[target()], anchor()),
            Err(EspAvbAemControlError::UnsupportedClock)
        );
    }

    #[test]
    fn missing_or_duplicate_avdecc_targets_fail_before_programming() {
        let envelopes = [envelope(EndpointCommand::Mute {
            module_id: "left-pod".to_owned(),
            epoch: 1,
        })];
        assert_eq!(
            compile_aem_control_program(&envelopes, &[], anchor()),
            Err(EspAvbAemControlError::UnknownTarget)
        );

        let target = target();
        assert_eq!(
            compile_aem_control_program(&envelopes, &[target.clone(), target], anchor()),
            Err(EspAvbAemControlError::DuplicateTarget)
        );
    }
}