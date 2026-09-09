use aurora_core::AudioBlock;
use aurora_renderer_hoa::{
    generate_regularized_mode_matching_matrix, render_hoa_coefficients, HoaMatrixGenerationError,
    HoaRendererError, SpeakerDirection,
};
use aurora_spatial_transport_v2::{
    cicp_layout_member_geometry, cicp_layout_members, TransportSceneDomain,
};
use thiserror::Error;

use crate::{
    evaluate_paired_mpegh_candidate, reference_roles, render_static_mpegh_object_plane,
    MpeghConformancePolicy, MpeghObjectCandidateError, MpeghPairedEvidence, MpeghPairedGateError,
    MpeghPairedPlaybackDecision, MpeghRoleConformanceError,
};

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghObjectsHoaCandidateDecision {
    pub candidate: AudioBlock,
    pub playback: MpeghPairedPlaybackDecision,
}

/// Render the admitted static point-object + HOA contribution and evidence-gate
/// it when used as the complete Objects+HOA scene. For Bed+Objects+HOA this
/// routine intentionally omits bed signals so a higher-level full-scene mixer
/// can add them exactly once.
pub fn evaluate_static_mpegh_objects_hoa_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<MpeghObjectsHoaCandidateDecision, MpeghObjectsHoaCandidateError> {
    if pair.scene.domain != TransportSceneDomain::ObjectsAndHoa {
        return Err(MpeghObjectsHoaCandidateError::IncompleteSceneCannotBeGated {
            domain: format!("{:?}", pair.scene.domain),
        });
    }
    let reference = pair
        .reference
        .as_ref()
        .ok_or(MpeghObjectsHoaCandidateError::MissingReferenceRender)?;
    let candidate = render_static_mpegh_objects_hoa_candidate(pair, regularization)?;
    let roles = reference_roles(&pair.reference_layout, candidate.channels.len())?;
    let playback = evaluate_paired_mpegh_candidate(
        pair,
        &candidate,
        reference.sample_rate,
        &roles,
        policy,
    )?;
    Ok(MpeghObjectsHoaCandidateDecision { candidate, playback })
}

pub fn render_static_mpegh_objects_hoa_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
) -> Result<AudioBlock, MpeghObjectsHoaCandidateError> {
    match pair.scene.domain {
        TransportSceneDomain::ObjectsAndHoa | TransportSceneDomain::BedObjectsAndHoa => {}
        domain => {
            return Err(MpeghObjectsHoaCandidateError::UnsupportedDomain {
                domain: format!("{domain:?}"),
            })
        }
    }
    let object_plane = render_static_mpegh_object_plane(pair)?;
    let hoa_plane = render_hoa_plane(pair, regularization)?;
    if object_plane.frame_count != hoa_plane.frame_count
        || object_plane.channels.len() != hoa_plane.channels.len()
    {
        return Err(MpeghObjectsHoaCandidateError::PlaneGeometryMismatch {
            object_channels: object_plane.channels.len(),
            hoa_channels: hoa_plane.channels.len(),
            object_frames: object_plane.frame_count,
            hoa_frames: hoa_plane.frame_count,
        });
    }

    let mut candidate = hoa_plane;
    for (destination, source) in candidate.channels.iter_mut().zip(object_plane.channels.iter()) {
        for (out, sample) in destination.iter_mut().zip(source.iter().copied()) {
            *out += sample;
        }
    }
    candidate
        .validate()
        .map_err(|_| MpeghObjectsHoaCandidateError::InvalidRenderedGeometry)?;
    Ok(candidate)
}

fn render_hoa_plane(
    pair: &MpeghPairedEvidence,
    regularization: f64,
) -> Result<AudioBlock, MpeghObjectsHoaCandidateError> {
    let coefficients = pair
        .hoa_coefficients
        .as_ref()
        .ok_or(MpeghObjectsHoaCandidateError::MissingHoaCoefficients)?;
    coefficients
        .validate()
        .map_err(|error| MpeghObjectsHoaCandidateError::InvalidHoaCoefficients(error.to_string()))?;

    let speakers = reference_speakers(pair)?;
    let full_range = speakers
        .iter()
        .enumerate()
        .filter(|(_, speaker)| !speaker.is_lfe)
        .map(|(index, speaker)| {
            (
                index,
                SpeakerDirection {
                    azimuth_degrees: f64::from(speaker.azimuth_degrees),
                    elevation_degrees: f64::from(speaker.elevation_degrees),
                },
            )
        })
        .collect::<Vec<_>>();
    if full_range.is_empty() {
        return Err(MpeghObjectsHoaCandidateError::NoFullRangeSpeakers);
    }
    let directions = full_range
        .iter()
        .map(|(_, direction)| *direction)
        .collect::<Vec<_>>();
    let matrix = generate_regularized_mode_matching_matrix(
        coefficients.order,
        &directions,
        regularization,
    )?;
    let mut timed = coefficients.clone();
    timed.audio.presentation_time_seconds = pair.scene.frame.decoded.audio.presentation_time_seconds;
    timed.audio.discontinuity = pair.scene.frame.decoded.audio.discontinuity;
    let rendered = render_hoa_coefficients(&timed, &matrix)?;
    let mut channels = vec![vec![0.0_f32; rendered.frame_count]; speakers.len()];
    for (rendered_index, (reference_index, _)) in full_range.iter().enumerate() {
        channels[*reference_index].copy_from_slice(&rendered.channels[rendered_index]);
    }
    Ok(AudioBlock {
        channels,
        frame_count: rendered.frame_count,
        presentation_time_seconds: rendered.presentation_time_seconds,
        discontinuity: rendered.discontinuity,
    })
}

fn reference_speakers(
    pair: &MpeghPairedEvidence,
) -> Result<Vec<crate::MpeghSpeaker>, MpeghObjectsHoaCandidateError> {
    if !pair.reference_layout.speakers.is_empty() {
        return Ok(pair.reference_layout.speakers.clone());
    }
    let cicp_index = u8::try_from(pair.reference_layout.cicp_index)
        .map_err(|_| MpeghObjectsHoaCandidateError::MissingReferenceSpeakerLayout)?;
    let members = cicp_layout_members(cicp_index)
        .ok_or(MpeghObjectsHoaCandidateError::MissingReferenceSpeakerLayout)?;
    let mut result = Vec::with_capacity(members.len());
    for member_index in 0..members.len() {
        let geometry = cicp_layout_member_geometry(cicp_index, member_index as u16).ok_or(
            MpeghObjectsHoaCandidateError::UnresolvedCicpSpeaker {
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
pub enum MpeghObjectsHoaCandidateError {
    #[error("MPEG-H static object+HOA plane does not support scene domain {domain}")]
    UnsupportedDomain { domain: String },
    #[error("object+HOA is only a partial contribution for scene domain {domain}; full-scene evidence gating is required")]
    IncompleteSceneCannotBeGated { domain: String },
    #[error("paired MPEG-H evidence has no decoded HOA coefficient frame")]
    MissingHoaCoefficients,
    #[error("paired MPEG-H evidence has no libmpegh reference render")]
    MissingReferenceRender,
    #[error("paired MPEG-H HOA coefficients failed validation: {0}")]
    InvalidHoaCoefficients(String),
    #[error("paired MPEG-H evidence has no resolvable reference speaker layout")]
    MissingReferenceSpeakerLayout,
    #[error("CICP layout {cicp_index} member {member_index} has no speaker geometry")]
    UnresolvedCicpSpeaker {
        cicp_index: u8,
        member_index: usize,
    },
    #[error("MPEG-H reference layout contains no full-range speakers")]
    NoFullRangeSpeakers,
    #[error("object and HOA candidate planes disagree: objects={object_channels}ch/{object_frames}f, HOA={hoa_channels}ch/{hoa_frames}f")]
    PlaneGeometryMismatch {
        object_channels: usize,
        hoa_channels: usize,
        object_frames: usize,
        hoa_frames: usize,
    },
    #[error("combined MPEG-H object+HOA candidate has invalid output geometry")]
    InvalidRenderedGeometry,
    #[error(transparent)]
    Object(#[from] MpeghObjectCandidateError),
    #[error(transparent)]
    Matrix(#[from] HoaMatrixGenerationError),
    #[error(transparent)]
    Renderer(#[from] HoaRendererError),
    #[error(transparent)]
    Roles(#[from] MpeghRoleConformanceError),
    #[error(transparent)]
    Playback(#[from] MpeghPairedGateError),
}
