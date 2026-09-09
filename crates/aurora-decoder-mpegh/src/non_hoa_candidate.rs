use std::collections::HashSet;

use aurora_core::{AudioBlock, ChannelRole};
use aurora_spatial_transport_v2::{
    cicp_layout_member_geometry, cicp_layout_members, ResolvedBedSignalTarget,
    SpatialTransportError, TransportSceneDomain,
};
use thiserror::Error;

use crate::{
    evaluate_paired_mpegh_candidate, reference_roles, render_exact_mpegh_object_plane,
    MpeghConformancePolicy, MpeghObjectTimelineError, MpeghPairedEvidence, MpeghPairedGateError,
    MpeghPairedPlaybackDecision, MpeghRoleConformanceError,
};

const BED_GEOMETRY_TOLERANCE_DEGREES: f64 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghNonHoaCandidateDecision {
    pub candidate: AudioBlock,
    pub playback: MpeghPairedPlaybackDecision,
}

/// Render MPEG-H scenes that contain no HOA transport.
///
/// Supported domains are discrete bed, point-object signals, and bed+objects.
/// Bed signals are routed only to unique reference destinations. Object motion
/// is evaluated at exact sample offsets by the existing point-object renderer.
pub fn render_mpegh_non_hoa_candidate(
    pair: &MpeghPairedEvidence,
) -> Result<AudioBlock, MpeghNonHoaCandidateError> {
    match pair.scene.domain {
        TransportSceneDomain::DiscreteBed
        | TransportSceneDomain::ObjectSignals
        | TransportSceneDomain::BedAndObjects => {}
        domain => {
            return Err(MpeghNonHoaCandidateError::UnsupportedDomain {
                domain: format!("{domain:?}"),
            })
        }
    }
    if !pair.scene.hoa_signals.is_empty() || !pair.scene.hoa_groups.is_empty() {
        return Err(MpeghNonHoaCandidateError::UnexpectedHoaState);
    }
    pair.scene
        .validate()
        .map_err(|error| MpeghNonHoaCandidateError::InvalidTransport(error.to_string()))?;

    let speakers = reference_speakers(pair)?;
    let frame_count = pair.scene.frame.decoded.audio.frame_count;
    let mut candidate = match pair.scene.domain {
        TransportSceneDomain::DiscreteBed => AudioBlock {
            channels: vec![vec![0.0; frame_count]; speakers.len()],
            frame_count,
            presentation_time_seconds: pair.scene.frame.decoded.audio.presentation_time_seconds,
            discontinuity: pair.scene.frame.decoded.audio.discontinuity,
        },
        TransportSceneDomain::ObjectSignals | TransportSceneDomain::BedAndObjects => {
            render_exact_mpegh_object_plane(pair)?
        }
        _ => unreachable!("domain admitted above"),
    };

    if matches!(
        pair.scene.domain,
        TransportSceneDomain::DiscreteBed | TransportSceneDomain::BedAndObjects
    ) {
        mix_bed(pair, &speakers, &mut candidate)?;
    }

    candidate
        .validate()
        .map_err(|_| MpeghNonHoaCandidateError::InvalidRenderedGeometry)?;
    Ok(candidate)
}

pub fn evaluate_mpegh_non_hoa_candidate(
    pair: &MpeghPairedEvidence,
    policy: MpeghConformancePolicy,
) -> Result<MpeghNonHoaCandidateDecision, MpeghNonHoaCandidateError> {
    let reference = pair
        .reference
        .as_ref()
        .ok_or(MpeghNonHoaCandidateError::MissingReferenceRender)?;
    let candidate = render_mpegh_non_hoa_candidate(pair)?;
    let roles = reference_roles(&pair.reference_layout, candidate.channels.len())?;
    let playback = evaluate_paired_mpegh_candidate(
        pair,
        &candidate,
        reference.sample_rate,
        &roles,
        policy,
    )?;
    Ok(MpeghNonHoaCandidateDecision { candidate, playback })
}

fn mix_bed(
    pair: &MpeghPairedEvidence,
    speakers: &[crate::MpeghSpeaker],
    output: &mut AudioBlock,
) -> Result<(), MpeghNonHoaCandidateError> {
    let frame_count = pair.scene.frame.decoded.audio.frame_count;
    if output.frame_count != frame_count || output.channels.len() != speakers.len() {
        return Err(MpeghNonHoaCandidateError::OutputGeometryMismatch {
            output_channels: output.channels.len(),
            reference_speakers: speakers.len(),
            output_frames: output.frame_count,
            scene_frames: frame_count,
        });
    }

    let mut destinations = HashSet::with_capacity(pair.scene.bed_signals.len());
    let mut cached_roles: Option<Vec<ChannelRole>> = None;
    for bed in &pair.scene.bed_signals {
        let destination = match bed
            .resolved_target()
            .map_err(MpeghNonHoaCandidateError::InvalidBedTarget)?
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
            return Err(MpeghNonHoaCandidateError::DuplicateBedDestination { destination });
        }

        let source = pair
            .scene
            .frame
            .decoded
            .audio
            .channels
            .get(bed.pcm_channel_index)
            .ok_or(MpeghNonHoaCandidateError::BedLaneOutOfRange {
                lane: bed.pcm_channel_index,
            })?;
        if source.len() != frame_count {
            return Err(MpeghNonHoaCandidateError::InvalidBedPlaneLength {
                lane: bed.pcm_channel_index,
                expected: frame_count,
                actual: source.len(),
            });
        }
        for (out, sample) in output.channels[destination]
            .iter_mut()
            .zip(source.iter().copied())
        {
            *out += sample;
        }
    }
    Ok(())
}

fn unique_role_destination(
    roles: &[ChannelRole],
    requested: &ChannelRole,
) -> Result<usize, MpeghNonHoaCandidateError> {
    let mut found = None;
    for (index, role) in roles.iter().enumerate() {
        if role == requested {
            if found.replace(index).is_some() {
                return Err(MpeghNonHoaCandidateError::AmbiguousBedRole {
                    role: requested.clone(),
                });
            }
        }
    }
    found.ok_or_else(|| MpeghNonHoaCandidateError::MissingBedRole {
        role: requested.clone(),
    })
}

fn unique_geometry_destination(
    speakers: &[crate::MpeghSpeaker],
    azimuth_degrees: f64,
    elevation_degrees: f64,
    is_lfe: bool,
) -> Result<usize, MpeghNonHoaCandidateError> {
    let mut found = None;
    for (index, speaker) in speakers.iter().enumerate() {
        if speaker.is_lfe != is_lfe {
            continue;
        }
        let az_error = angular_distance_degrees(f64::from(speaker.azimuth_degrees), azimuth_degrees);
        let el_error = (f64::from(speaker.elevation_degrees) - elevation_degrees).abs();
        if az_error <= BED_GEOMETRY_TOLERANCE_DEGREES
            && el_error <= BED_GEOMETRY_TOLERANCE_DEGREES
        {
            if found.replace(index).is_some() {
                return Err(MpeghNonHoaCandidateError::AmbiguousBedGeometry {
                    azimuth_degrees,
                    elevation_degrees,
                    is_lfe,
                });
            }
        }
    }
    found.ok_or(MpeghNonHoaCandidateError::MissingBedGeometry {
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
) -> Result<Vec<crate::MpeghSpeaker>, MpeghNonHoaCandidateError> {
    if !pair.reference_layout.speakers.is_empty() {
        return Ok(pair.reference_layout.speakers.clone());
    }
    let cicp_index = u8::try_from(pair.reference_layout.cicp_index)
        .map_err(|_| MpeghNonHoaCandidateError::MissingReferenceSpeakerLayout)?;
    let members = cicp_layout_members(cicp_index)
        .ok_or(MpeghNonHoaCandidateError::MissingReferenceSpeakerLayout)?;
    let mut result = Vec::with_capacity(members.len());
    for member_index in 0..members.len() {
        let geometry = cicp_layout_member_geometry(cicp_index, member_index as u16).ok_or(
            MpeghNonHoaCandidateError::UnresolvedCicpSpeaker {
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
pub enum MpeghNonHoaCandidateError {
    #[error("MPEG-H non-HOA candidate does not support scene domain {domain}")]
    UnsupportedDomain { domain: String },
    #[error("non-HOA MPEG-H candidate unexpectedly contains HOA transport state")]
    UnexpectedHoaState,
    #[error("MPEG-H transport scene failed validation: {0}")]
    InvalidTransport(String),
    #[error("paired MPEG-H evidence has no libmpegh reference render")]
    MissingReferenceRender,
    #[error("paired MPEG-H evidence has no resolvable reference speaker layout")]
    MissingReferenceSpeakerLayout,
    #[error("CICP layout {cicp_index} member {member_index} has no speaker geometry")]
    UnresolvedCicpSpeaker {
        cicp_index: u8,
        member_index: usize,
    },
    #[error("MPEG-H non-HOA output geometry differs from reference: output={output_channels}ch/{output_frames}f, reference={reference_speakers}ch/{scene_frames}f")]
    OutputGeometryMismatch {
        output_channels: usize,
        reference_speakers: usize,
        output_frames: usize,
        scene_frames: usize,
    },
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
    #[error("MPEG-H non-HOA candidate produced invalid output geometry")]
    InvalidRenderedGeometry,
    #[error(transparent)]
    Objects(#[from] MpeghObjectTimelineError),
    #[error(transparent)]
    Roles(#[from] MpeghRoleConformanceError),
    #[error(transparent)]
    Playback(#[from] MpeghPairedGateError),
}
