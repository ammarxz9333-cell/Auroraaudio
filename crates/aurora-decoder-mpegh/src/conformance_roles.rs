use std::collections::HashMap;

use aurora_core::ChannelRole;
use aurora_spatial_transport_v2::{cicp_layout_member_speaker_index, cicp_layout_members};
use thiserror::Error;

use crate::{
    compare_mpegh_render_to_reference, MpeghConformanceError, MpeghConformancePolicy,
    MpeghConformanceReport, MpeghRenderedPcm, MpeghSpeaker, MpeghSpeakerLayout,
};

/// Compare Aurora speaker output with libmpegh after aligning channels by
/// semantic speaker role instead of raw channel index.
///
/// This is intentionally fail-closed. If the reference layout cannot be
/// projected losslessly to Aurora's semantic roles, or either side contains a
/// duplicate semantic destination, no conformance verdict is produced.
pub fn compare_mpegh_render_to_reference_by_role(
    candidate: &[Vec<f32>],
    candidate_roles: &[ChannelRole],
    reference: &MpeghRenderedPcm,
    reference_layout: &MpeghSpeakerLayout,
    policy: MpeghConformancePolicy,
) -> Result<MpeghConformanceReport, MpeghRoleConformanceError> {
    if candidate.len() != candidate_roles.len() {
        return Err(MpeghRoleConformanceError::CandidateRoleCountMismatch {
            channels: candidate.len(),
            roles: candidate_roles.len(),
        });
    }
    if reference.channel_count != candidate.len() {
        return Err(MpeghRoleConformanceError::ChannelCountMismatch {
            candidate: candidate.len(),
            reference: reference.channel_count,
        });
    }

    let reference_roles = reference_roles(reference_layout, reference.channel_count)?;
    let mut candidate_by_role = HashMap::with_capacity(candidate_roles.len());
    for (index, role) in candidate_roles.iter().cloned().enumerate() {
        if candidate_by_role.insert(role.clone(), index).is_some() {
            return Err(MpeghRoleConformanceError::DuplicateCandidateRole { role });
        }
    }

    let mut reordered = Vec::with_capacity(reference_roles.len());
    for role in &reference_roles {
        let index = candidate_by_role
            .get(role)
            .copied()
            .ok_or_else(|| MpeghRoleConformanceError::MissingCandidateRole {
                role: role.clone(),
            })?;
        reordered.push(candidate[index].clone());
    }

    Ok(compare_mpegh_render_to_reference(
        &reordered,
        reference,
        policy,
    )?)
}

/// Resolve the libmpegh speaker render into Aurora semantic roles. Prefer the
/// standardized CICP layout index; fall back to exact known geometry only when
/// libmpegh reports a non-standard/custom layout with per-speaker geometry.
pub fn reference_roles(
    layout: &MpeghSpeakerLayout,
    channel_count: usize,
) -> Result<Vec<ChannelRole>, MpeghRoleConformanceError> {
    if let Ok(cicp_index) = u8::try_from(layout.cicp_index) {
        if let Some(members) = cicp_layout_members(cicp_index) {
            if members.len() == channel_count {
                let mut roles = Vec::with_capacity(channel_count);
                for member_index in 0..members.len() {
                    let speaker_index = cicp_layout_member_speaker_index(
                        cicp_index,
                        member_index as u16,
                    )
                    .ok_or(MpeghRoleConformanceError::UnresolvedCicpRole {
                        cicp_index,
                        member_index,
                    })?;
                    let role = semantic_role_from_cicp_speaker(speaker_index).ok_or(
                        MpeghRoleConformanceError::UnresolvedCicpRole {
                            cicp_index,
                            member_index,
                        },
                    )?;
                    roles.push(role);
                }
                ensure_unique_reference_roles(&roles)?;
                return Ok(roles);
            }
        }
    }

    if layout.speakers.len() != channel_count {
        return Err(MpeghRoleConformanceError::ReferenceLayoutChannelMismatch {
            speakers: layout.speakers.len(),
            channels: channel_count,
        });
    }

    let mut roles = Vec::with_capacity(channel_count);
    for (channel, speaker) in layout.speakers.iter().enumerate() {
        let role = semantic_role_from_geometry(*speaker).ok_or(
            MpeghRoleConformanceError::UnresolvedReferenceGeometry {
                channel,
                is_lfe: speaker.is_lfe,
                azimuth_degrees: speaker.azimuth_degrees,
                elevation_degrees: speaker.elevation_degrees,
            },
        )?;
        roles.push(role);
    }
    ensure_unique_reference_roles(&roles)?;
    Ok(roles)
}

fn ensure_unique_reference_roles(
    roles: &[ChannelRole],
) -> Result<(), MpeghRoleConformanceError> {
    let mut seen = HashMap::with_capacity(roles.len());
    for (index, role) in roles.iter().cloned().enumerate() {
        if let Some(first_channel) = seen.insert(role.clone(), index) {
            return Err(MpeghRoleConformanceError::DuplicateReferenceRole {
                role,
                first_channel,
                second_channel: index,
            });
        }
    }
    Ok(())
}

fn semantic_role_from_cicp_speaker(index: u8) -> Option<ChannelRole> {
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

fn semantic_role_from_geometry(speaker: MpeghSpeaker) -> Option<ChannelRole> {
    if speaker.is_lfe {
        return Some(ChannelRole::LowFrequencyEffects);
    }
    match (speaker.azimuth_degrees, speaker.elevation_degrees) {
        (30, 0) => Some(ChannelRole::FrontLeft),
        (-30, 0) => Some(ChannelRole::FrontRight),
        (0, 0) => Some(ChannelRole::FrontCenter),
        (90 | 110, 0) => Some(ChannelRole::SurroundLeft),
        (-90 | -110, 0) => Some(ChannelRole::SurroundRight),
        (135 | 150, 0) => Some(ChannelRole::SurroundBackLeft),
        (-135 | -150, 0) => Some(ChannelRole::SurroundBackRight),
        (30 | 45, 35) => Some(ChannelRole::TopFrontLeft),
        (-30 | -45, 35) => Some(ChannelRole::TopFrontRight),
        (110 | 135, 35) => Some(ChannelRole::TopRearLeft),
        (-110 | -135, 35) => Some(ChannelRole::TopRearRight),
        _ => None,
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum MpeghRoleConformanceError {
    #[error(transparent)]
    Conformance(#[from] MpeghConformanceError),
    #[error("candidate has {channels} channels but {roles} semantic channel roles")]
    CandidateRoleCountMismatch { channels: usize, roles: usize },
    #[error("candidate has {candidate} channels but reference has {reference}")]
    ChannelCountMismatch { candidate: usize, reference: usize },
    #[error("candidate semantic role '{role}' appears more than once")]
    DuplicateCandidateRole { role: ChannelRole },
    #[error("candidate is missing semantic role '{role}' required by the reference layout")]
    MissingCandidateRole { role: ChannelRole },
    #[error("reference CICP layout {cicp_index} member {member_index} has no lossless Aurora semantic role")]
    UnresolvedCicpRole {
        cicp_index: u8,
        member_index: usize,
    },
    #[error("reference layout has {speakers} speaker descriptors for {channels} PCM channels")]
    ReferenceLayoutChannelMismatch { speakers: usize, channels: usize },
    #[error("reference channel {channel} geometry lfe={is_lfe} az={azimuth_degrees} el={elevation_degrees} has no lossless Aurora semantic role")]
    UnresolvedReferenceGeometry {
        channel: usize,
        is_lfe: bool,
        azimuth_degrees: i16,
        elevation_degrees: i16,
    },
    #[error("reference semantic role '{role}' is duplicated at channels {first_channel} and {second_channel}")]
    DuplicateReferenceRole {
        role: ChannelRole,
        first_channel: usize,
        second_channel: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_s16_stereo() -> MpeghRenderedPcm {
        let samples = [[16_384_i16, -16_384_i16], [8_192, -8_192]];
        let mut bytes = Vec::new();
        for frame in samples {
            bytes.extend_from_slice(&frame[0].to_le_bytes());
            bytes.extend_from_slice(&frame[1].to_le_bytes());
        }
        MpeghRenderedPcm {
            bytes,
            bit_depth: 16,
            channel_count: 2,
            frame_count: 2,
            sample_rate: 48_000,
        }
    }

    #[test]
    fn role_alignment_corrects_swapped_candidate_order() {
        let reference = reference_s16_stereo();
        let layout = MpeghSpeakerLayout {
            cicp_index: 2,
            layout_code: 0,
            speakers: Vec::new(),
        };
        let reference_planar = reference.decode_planar_f32().unwrap();
        let candidate = vec![reference_planar[1].clone(), reference_planar[0].clone()];
        let roles = vec![ChannelRole::FrontRight, ChannelRole::FrontLeft];
        let report = compare_mpegh_render_to_reference_by_role(
            &candidate,
            &roles,
            &reference,
            &layout,
            MpeghConformancePolicy::near_reference(),
        )
        .unwrap();
        assert!(report.passed);
    }

    #[test]
    fn layout_19_resolves_to_twelve_unique_aurora_roles() {
        let layout = MpeghSpeakerLayout {
            cicp_index: 19,
            layout_code: 0,
            speakers: Vec::new(),
        };
        let roles = reference_roles(&layout, 12).unwrap();
        assert_eq!(roles.len(), 12);
        assert!(roles.contains(&ChannelRole::TopFrontLeft));
        assert!(roles.contains(&ChannelRole::TopRearRight));
    }

    #[test]
    fn unsupported_reference_geometry_fails_closed() {
        let layout = MpeghSpeakerLayout {
            cicp_index: 0,
            layout_code: 0,
            speakers: vec![MpeghSpeaker {
                is_lfe: false,
                azimuth_degrees: 60,
                elevation_degrees: 0,
            }],
        };
        assert!(matches!(
            reference_roles(&layout, 1),
            Err(MpeghRoleConformanceError::UnresolvedReferenceGeometry { .. })
        ));
    }
}
