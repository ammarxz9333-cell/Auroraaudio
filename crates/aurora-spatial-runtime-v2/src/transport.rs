use std::collections::HashSet;

use aurora_core::{AudioBlock, ChannelRole};
use aurora_spatial_ir_v2::{
    BedSignalBinding, SpatialDecodedFrame as SpatialDecodedFrameV2,
};
use aurora_spatial_transport_v2::{
    cicp_layout_member_speaker_index, BedSignalTarget, SpatialTransportFrame,
};
use thiserror::Error;

use crate::native::{NativeV2SpatialRuntime, NativeV2SpatialRuntimeError};

impl NativeV2SpatialRuntime {
    /// Render a losslessly projectable Transport V2 scene through Aurora's
    /// native V2 speaker renderer.
    ///
    /// HOA transport signals are intentionally rejected because libmpegh's
    /// external PCM carries HOA transport channels, not ready Ambisonics
    /// coefficients. Flexible/custom bed geometry is likewise rejected until a
    /// geometry-to-room matching policy is proven. Callers can use the paired
    /// libmpegh reference render as the safe fallback for those frames.
    pub fn render_transport_frame(
        &mut self,
        frame: &SpatialTransportFrame,
    ) -> Result<AudioBlock, TransportV2RuntimeError> {
        let projected = project_transport_to_spatial_v2(frame)?;
        self.render_frame(&projected)
            .map_err(TransportV2RuntimeError::NativeV2)
    }
}

pub fn project_transport_to_spatial_v2(
    frame: &SpatialTransportFrame,
) -> Result<SpatialDecodedFrameV2, TransportV2RuntimeError> {
    frame
        .validate()
        .map_err(|error| TransportV2RuntimeError::InvalidTransport(error.to_string()))?;

    if !frame.hoa_signals.is_empty() || !frame.hoa_groups.is_empty() {
        return Err(TransportV2RuntimeError::HoaTransportRequiresNativeDecoder {
            signals: frame.hoa_signals.len(),
            groups: frame.hoa_groups.len(),
        });
    }

    let mut projected = frame.frame.clone();
    let mut roles = HashSet::with_capacity(frame.bed_signals.len());
    let mut beds = Vec::with_capacity(frame.bed_signals.len());
    for bed in &frame.bed_signals {
        let role = semantic_role_for_target(&bed.target)?;
        if !roles.insert(role.clone()) {
            return Err(TransportV2RuntimeError::DuplicateSemanticBedRole {
                role: role.to_string(),
            });
        }
        beds.push(BedSignalBinding {
            pcm_channel_index: bed.pcm_channel_index,
            role,
        });
    }
    projected.spatial.bed_signals = beds;
    projected
        .validate()
        .map_err(|error| TransportV2RuntimeError::InvalidProjectedV2(error.to_string()))?;
    Ok(projected)
}

fn semantic_role_for_target(
    target: &BedSignalTarget,
) -> Result<ChannelRole, TransportV2RuntimeError> {
    match target {
        BedSignalTarget::SemanticRole(role) => Ok(role.clone()),
        BedSignalTarget::CicpSpeakerIndex(index) => cicp_index_to_role(*index)
            .ok_or(TransportV2RuntimeError::UnsupportedCicpSpeaker { index: *index }),
        BedSignalTarget::CicpLayoutMember {
            layout_index,
            member_index,
        } => {
            let speaker_index = cicp_layout_member_speaker_index(*layout_index, *member_index)
                .ok_or(TransportV2RuntimeError::UnknownCicpLayoutMember {
                    layout_index: *layout_index,
                    member_index: *member_index,
                })?;
            cicp_index_to_role(speaker_index).ok_or(
                TransportV2RuntimeError::UnsupportedCicpLayoutMember {
                    layout_index: *layout_index,
                    member_index: *member_index,
                    speaker_index,
                },
            )
        }
        BedSignalTarget::ExplicitGeometry(_) => {
            Err(TransportV2RuntimeError::ExplicitGeometryRequiresRoomMatcher)
        }
    }
}

/// CICP speakers with exact equivalents in Aurora's current canonical role
/// vocabulary. Deliberately excludes front-wide, top-center, lower-layer and
/// screen-relative speakers instead of snapping them to a nearby role.
fn cicp_index_to_role(index: u8) -> Option<ChannelRole> {
    Some(match index {
        0 => ChannelRole::FrontLeft,
        1 => ChannelRole::FrontRight,
        2 => ChannelRole::FrontCenter,
        3 | 26 | 36 => ChannelRole::LowFrequencyEffects,
        4 | 13 => ChannelRole::SurroundLeft,
        5 | 14 => ChannelRole::SurroundRight,
        8 | 41 => ChannelRole::SurroundBackLeft,
        9 | 42 => ChannelRole::SurroundBackRight,
        17 | 32 => ChannelRole::TopFrontLeft,
        18 | 33 => ChannelRole::TopFrontRight,
        20 | 30 => ChannelRole::TopRearLeft,
        21 | 31 => ChannelRole::TopRearRight,
        _ => return None,
    })
}

#[derive(Debug, Error)]
pub enum TransportV2RuntimeError {
    #[error("Spatial Transport V2 validation failed: {0}")]
    InvalidTransport(String),
    #[error("projected Spatial IR V2 validation failed: {0}")]
    InvalidProjectedV2(String),
    #[error("HOA transport requires a native HOA transport decoder before Aurora speaker rendering ({signals} signals across {groups} groups)")]
    HoaTransportRequiresNativeDecoder { signals: usize, groups: usize },
    #[error("CICP speaker index {index} has no lossless Aurora semantic-role projection")]
    UnsupportedCicpSpeaker { index: u8 },
    #[error("CICP layout {layout_index} member {member_index} is unknown")]
    UnknownCicpLayoutMember {
        layout_index: u8,
        member_index: u16,
    },
    #[error("CICP layout {layout_index} member {member_index} resolves to speaker index {speaker_index}, which has no lossless Aurora semantic-role projection")]
    UnsupportedCicpLayoutMember {
        layout_index: u8,
        member_index: u16,
        speaker_index: u8,
    },
    #[error("transport bed role '{role}' is targeted more than once; implicit bed downmix is not admitted")]
    DuplicateSemanticBedRole { role: String },
    #[error("explicit/flexible transport bed geometry requires the room-geometry matcher")]
    ExplicitGeometryRequiresRoomMatcher,
    #[error(transparent)]
    NativeV2(#[from] NativeV2SpatialRuntimeError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::AudioBlock;
    use aurora_decoder_api::DecodedFrame;
    use aurora_spatial_ir_v2::{SpatialDomain, SpatialFrameMetadata};
    use aurora_spatial_transport_v2::{
        HoaGroupBinding, HoaSignalBinding, SpatialTransportFrame, TransportBedSignalBinding,
        TransportSceneDomain,
    };

    fn transport_bed(layout_index: u8, channel_count: usize) -> SpatialTransportFrame {
        SpatialTransportFrame {
            frame: SpatialDecodedFrameV2 {
                decoded: DecodedFrame {
                    audio: AudioBlock {
                        channels: vec![vec![0.0; 40]; channel_count],
                        frame_count: 40,
                        presentation_time_seconds: 0.0,
                        discontinuity: true,
                    },
                    objects: Vec::new(),
                },
                spatial: SpatialFrameMetadata {
                    domain: SpatialDomain::DiscreteBed,
                    bed_signals: Vec::new(),
                    object_signals: Vec::new(),
                    object_updates: Vec::new(),
                },
            },
            domain: TransportSceneDomain::DiscreteBed,
            bed_signals: (0..channel_count)
                .map(|index| TransportBedSignalBinding {
                    pcm_channel_index: index,
                    target: BedSignalTarget::CicpLayoutMember {
                        layout_index,
                        member_index: index as u16,
                    },
                })
                .collect(),
            hoa_signals: Vec::new(),
            hoa_groups: Vec::new(),
            codec_metadata: Vec::new(),
        }
    }

    #[test]
    fn cicp_layout_19_projects_to_canonical_7_1_4_bed() {
        let frame = transport_bed(19, 12);
        let projected = project_transport_to_spatial_v2(&frame).unwrap();
        let roles = projected
            .spatial
            .bed_signals
            .iter()
            .map(|bed| bed.role.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            roles,
            vec![
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundBackLeft,
                ChannelRole::SurroundBackRight,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::TopFrontLeft,
                ChannelRole::TopFrontRight,
                ChannelRole::TopRearLeft,
                ChannelRole::TopRearRight,
            ]
        );
    }

    #[test]
    fn front_wide_layout_fails_instead_of_snapping_to_front_left_right() {
        let frame = transport_bed(7, 8);
        assert!(matches!(
            project_transport_to_spatial_v2(&frame),
            Err(TransportV2RuntimeError::UnsupportedCicpLayoutMember { .. })
        ));
    }

    #[test]
    fn hoa_transport_fails_before_speaker_rendering() {
        let mut frame = transport_bed(2, 2);
        frame.frame.decoded.audio.channels.push(vec![0.0; 40]);
        frame.hoa_signals.push(HoaSignalBinding {
            pcm_channel_index: 2,
            transport_index: 0,
        });
        frame.hoa_groups.push(HoaGroupBinding {
            group_index: 0,
            transport_indices: vec![0],
            order: 1,
            fixed_position: false,
            priority: 0,
            uses_nfc: false,
            nfc_reference_distance_raw: None,
            matrix: None,
            screen_relative: false,
        });
        frame.domain = TransportSceneDomain::BedAndHoa;
        assert!(matches!(
            project_transport_to_spatial_v2(&frame),
            Err(TransportV2RuntimeError::HoaTransportRequiresNativeDecoder { .. })
        ));
    }
}
