use std::collections::HashSet;

use aurora_core::{AudioBlock, ChannelRole};
use aurora_spatial_transport_v2::{
    cicp_layout_member_geometry, cicp_layout_members, ResolvedBedSignalTarget,
    SpatialTransportError, TransportSceneDomain,
};
use thiserror::Error;

use crate::{
    evaluate_paired_mpegh_candidate, reference_roles, render_static_mpegh_objects_hoa_candidate,
    MpeghConformancePolicy, MpeghObjectsHoaCandidateError, MpeghPairedEvidence,
    MpeghPairedGateError, MpeghPairedPlaybackDecision, MpeghRoleConformanceError,
};

const BED_GEOMETRY_TOLERANCE_DEGREES: f64 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghFullSceneCandidateDecision {
    pub candidate: AudioBlock,
    pub playback: MpeghPairedPlaybackDecision,
}

/// Render the currently admitted complete MPEG-H Bed+Objects+HOA scene and
/// evidence-gate it against libmpegh's render from the same access unit.
///
/// Object admission remains intentionally conservative and is delegated to the
/// static point-object plane. Bed signals are direct-routed only when their
/// semantic role or exact geometry has one unique destination in the reference
/// speaker order. HOA never contributes to LFE; a signalled bed LFE does.
pub fn evaluate_static_mpegh_full_scene_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<MpeghFullSceneCandidateDecision, MpeghFullSceneCandidateError> {
    let reference = pair
        .reference
        .as_ref()
        .ok_or(MpeghFullSceneCandidateError::MissingReferenceRender)?;
    let candidate = render_static_mpegh_full_scene_candidate(pair, regularization)?;
    let roles = reference_roles(&pair.reference_layout, candidate.channels.len())?;
    let playback = evaluate_paired_mpegh_candidate(
        pair,
        &candidate,
        reference.sample_rate,
        &roles,
        policy,
    )?;
    Ok(MpeghFullSceneCandidateDecision { candidate, playback })
}

pub fn render_static_mpegh_full_scene_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
) -> Result<AudioBlock, MpeghFullSceneCandidateError> {
    if pair.scene.domain != TransportSceneDomain::BedObjectsAndHoa {
        return Err(MpeghFullSceneCandidateError::UnsupportedDomain {
            domain: format!("{:?}", pair.scene.domain),
        });
    }
    pair.scene
        .validate()
        .map_err(|error| MpeghFullSceneCandidateError::InvalidTransport(error.to_string()))?;

    let mut candidate = render_static_mpegh_objects_hoa_candidate(pair, regularization)?;
    let speakers = reference_speakers(pair)?;
    if speakers.len() != candidate.channels.len() {
        return Err(MpeghFullSceneCandidateError::ReferenceChannelMismatch {
            speakers: speakers.len(),
            candidate: candidate.channels.len(),
        });
    }
    mix_bed(pair, &speakers, &mut candidate)?;
    candidate
        .validate()
        .map_err(|_| MpeghFullSceneCandidateError::InvalidRenderedGeometry)?;
    Ok(candidate)
}

fn mix_bed(
    pair: &MpeghPairedEvidence,
    speakers: &[crate::MpeghSpeaker],
    output: &mut AudioBlock,
) -> Result<(), MpeghFullSceneCandidateError> {
    let frame_count = pair.scene.frame.decoded.audio.frame_count;
    let mut destinations = HashSet::with_capacity(pair.scene.bed_signals.len());
    let mut cached_roles: Option<Vec<ChannelRole>> = None;

    for bed in &pair.scene.bed_signals {
        let destination = match bed
            .resolved_target()
            .map_err(MpeghFullSceneCandidateError::InvalidBedTarget)?
        {
            ResolvedBedSignalTarget::SemanticRole(role) => {
                if cached_roles.is_none() {
                    cached_roles = Some(reference_roles(&pair.reference_layout, speakers.len())?);
                }
                unique_role_destination(cached_roles.as_ref().expect("initialized"), &role)?
            }
            ResolvedBedSignalTarget::Geometry(geometry) => unique_geometry_destination(
                speakers,
                f64::from(geometry.azimuth_degrees),
                f64::from(geometry.elevation_degrees),
                geometry.is_lfe,
            )?,
        };
        if !destinations.insert(destination) {
            return Err(MpeghFullSceneCandidateError::DuplicateBedDestination { destination });
        }
        let source = pair
            .scene
            .frame
            .decoded
            .audio
            .channels
            .get(bed.pcm_channel_index)
            .ok_or(MpeghFullSceneCandidateError::BedLaneOutOfRange {
                lane: bed.pcm_channel_index,
            })?;
        if source.len() != frame_count {
            return Err(MpeghFullSceneCandidateError::InvalidBedPlaneLength {
                lane: bed.pcm_channel_index,
                expected: frame_count,
                actual: source.len(),
            });
        }
        let destination_plane = output
            .channels
            .get_mut(destination)
            .ok_or(MpeghFullSceneCandidateError::BedDestinationOutOfRange { destination })?;
        for (out, sample) in destination_plane.iter_mut().zip(source.iter().copied()) {
            *out += sample;
        }
    }
    Ok(())
}

fn unique_role_destination(
    roles: &[ChannelRole],
    requested: &ChannelRole,
) -> Result<usize, MpeghFullSceneCandidateError> {
    let mut found = None;
    for (index, role) in roles.iter().enumerate() {
        if role != requested {
            continue;
        }
        if found.replace(index).is_some() {
            return Err(MpeghFullSceneCandidateError::AmbiguousBedRole {
                role: requested.clone(),
            });
        }
    }
    found.ok_or_else(|| MpeghFullSceneCandidateError::MissingBedRole {
        role: requested.clone(),
    })
}

fn unique_geometry_destination(
    speakers: &[crate::MpeghSpeaker],
    azimuth_degrees: f64,
    elevation_degrees: f64,
    is_lfe: bool,
) -> Result<usize, MpeghFullSceneCandidateError> {
    let mut found = None;
    for (index, speaker) in speakers.iter().enumerate() {
        if speaker.is_lfe != is_lfe {
            continue;
        }
        let azimuth_error = angular_distance_degrees(
            f64::from(speaker.azimuth_degrees),
            azimuth_degrees,
        );
        let elevation_error =
            (f64::from(speaker.elevation_degrees) - elevation_degrees).abs();
        if azimuth_error <= BED_GEOMETRY_TOLERANCE_DEGREES
            && elevation_error <= BED_GEOMETRY_TOLERANCE_DEGREES
        {
            if found.replace(index).is_some() {
                return Err(MpeghFullSceneCandidateError::AmbiguousBedGeometry {
                    azimuth_degrees,
                    elevation_degrees,
                    is_lfe,
                });
            }
        }
    }
    found.ok_or(MpeghFullSceneCandidateError::MissingBedGeometry {
        azimuth_degrees,
        elevation_degrees,
        is_lfe,
    })
}

fn angular_distance_degrees(a: f64, b: f64) -> f64 {
    let mut delta = (a - b).rem_euclid(360.0);
    if delta > 180.0 {
        delta = 360.0 - delta;
    }
    delta
}

fn reference_speakers(
    pair: &MpeghPairedEvidence,
) -> Result<Vec<crate::MpeghSpeaker>, MpeghFullSceneCandidateError> {
    if !pair.reference_layout.speakers.is_empty() {
        return Ok(pair.reference_layout.speakers.clone());
    }
    let cicp_index = u8::try_from(pair.reference_layout.cicp_index)
        .map_err(|_| MpeghFullSceneCandidateError::MissingReferenceSpeakerLayout)?;
    let members = cicp_layout_members(cicp_index)
        .ok_or(MpeghFullSceneCandidateError::MissingReferenceSpeakerLayout)?;
    let mut result = Vec::with_capacity(members.len());
    for member_index in 0..members.len() {
        let geometry = cicp_layout_member_geometry(cicp_index, member_index as u16).ok_or(
            MpeghFullSceneCandidateError::UnresolvedCicpSpeaker {
                cicp_index,
                member_index,
            },
        )?;
        result.push(crate::MpeghSpeaker {
            is_lfe: geometry.is_lfe,
            azimuth_degrees: geometry.azimuth_degrees,
            elevation_degrees: geometry.elevation_degrees,
        });
    }
    Ok(result)
}

#[derive(Debug, Error)]
pub enum MpeghFullSceneCandidateError {
    #[error("MPEG-H full candidate requires BedObjectsAndHoa, got {domain}")]
    UnsupportedDomain { domain: String },
    #[error("paired MPEG-H evidence has no libmpegh reference render")]
    MissingReferenceRender,
    #[error("MPEG-H transport scene failed validation: {0}")]
    InvalidTransport(String),
    #[error("paired MPEG-H evidence has no resolvable reference speaker layout")]
    MissingReferenceSpeakerLayout,
    #[error("CICP layout {cicp_index} member {member_index} has no speaker geometry")]
    UnresolvedCicpSpeaker {
        cicp_index: u8,
        member_index: usize,
    },
    #[error("reference layout has {speakers} speakers but candidate has {candidate} channels")]
    ReferenceChannelMismatch { speakers: usize, candidate: usize },
    #[error("MPEG-H bed target failed resolution: {0}")]
    InvalidBedTarget(SpatialTransportError),
    #[error("reference layout has no destination for bed role '{role}'")]
    MissingBedRole { role: ChannelRole },
    #[error("reference layout maps bed role '{role}' ambiguously")]
    AmbiguousBedRole { role: ChannelRole },
    #[error("reference layout has no speaker at lfe={is_lfe} az={azimuth_degrees} el={elevation_degrees}")]
    MissingBedGeometry {
        azimuth_degrees: f64,
        elevation_degrees: f64,
        is_lfe: bool,
    },
    #[error("reference layout has multiple speakers at lfe={is_lfe} az={azimuth_degrees} el={elevation_degrees}")]
    AmbiguousBedGeometry {
        azimuth_degrees: f64,
        elevation_degrees: f64,
        is_lfe: bool,
    },
    #[error("multiple bed signals target reference speaker {destination}")]
    DuplicateBedDestination { destination: usize },
    #[error("bed PCM lane {lane} is outside the scene audio block")]
    BedLaneOutOfRange { lane: usize },
    #[error("bed PCM lane {lane} has {actual} samples, expected {expected}")]
    InvalidBedPlaneLength {
        lane: usize,
        expected: usize,
        actual: usize,
    },
    #[error("bed destination {destination} is outside candidate output")]
    BedDestinationOutOfRange { destination: usize },
    #[error("full MPEG-H candidate produced invalid output geometry")]
    InvalidRenderedGeometry,
    #[error(transparent)]
    ObjectsHoa(#[from] MpeghObjectsHoaCandidateError),
    #[error(transparent)]
    Roles(#[from] MpeghRoleConformanceError),
    #[error(transparent)]
    Playback(#[from] MpeghPairedGateError),
}
