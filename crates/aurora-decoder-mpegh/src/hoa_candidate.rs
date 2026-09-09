use aurora_core::AudioBlock;
use aurora_renderer_hoa::{
    generate_regularized_mode_matching_matrix, render_hoa_coefficients, HoaMatrixGenerationError,
    HoaRendererError, SpeakerDirection,
};
use aurora_spatial_transport_v2::{
    cicp_layout_member_geometry, cicp_layout_members, TransportSceneDomain,
};
use thiserror::Error;

use crate::MpeghPairedEvidence;

#[derive(Debug, Clone, Copy)]
struct CandidateSpeaker {
    direction: SpeakerDirection,
    is_lfe: bool,
}

/// Render the HOA plane of a *pure HOA* MPEG-H scene to the exact speaker order
/// exposed by libmpegh's reference layout.
///
/// Full-range speakers participate in the candidate decode matrix. LFE rows are
/// explicitly zero because spherical-harmonic coefficients are not sent to LFE
/// by this candidate path. Mixed bed/object/HOA scenes are rejected until their
/// independent contributions can be combined losslessly before comparison.
pub fn render_pure_mpegh_hoa_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
) -> Result<AudioBlock, MpeghHoaCandidateError> {
    if pair.scene.domain != TransportSceneDomain::HoaTransport {
        return Err(MpeghHoaCandidateError::MixedSceneUnsupported {
            domain: format!("{:?}", pair.scene.domain),
        });
    }
    if !pair.scene.bed_signals.is_empty()
        || !pair.scene.frame.spatial.object_signals.is_empty()
        || !pair.scene.frame.spatial.object_updates.is_empty()
    {
        return Err(MpeghHoaCandidateError::MixedSceneStatePresent);
    }

    let coefficients = pair
        .hoa_coefficients
        .as_ref()
        .ok_or(MpeghHoaCandidateError::MissingHoaCoefficients)?;
    coefficients
        .validate()
        .map_err(|error| MpeghHoaCandidateError::InvalidHoaCoefficients(error.to_string()))?;

    let speakers = candidate_speakers(pair)?;
    let full_range = speakers
        .iter()
        .enumerate()
        .filter(|(_, speaker)| !speaker.is_lfe)
        .map(|(index, speaker)| (index, speaker.direction))
        .collect::<Vec<_>>();
    if full_range.is_empty() {
        return Err(MpeghHoaCandidateError::NoFullRangeSpeakers);
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

    let mut timed_coefficients = coefficients.clone();
    timed_coefficients.audio.presentation_time_seconds =
        pair.scene.frame.decoded.audio.presentation_time_seconds;
    timed_coefficients.audio.discontinuity = pair.scene.frame.decoded.audio.discontinuity;

    let full_range_render = render_hoa_coefficients(&timed_coefficients, &matrix)?;
    let frame_count = full_range_render.frame_count;
    let mut channels = vec![vec![0.0_f32; frame_count]; speakers.len()];
    for (rendered_index, (reference_index, _)) in full_range.iter().enumerate() {
        channels[*reference_index].copy_from_slice(&full_range_render.channels[rendered_index]);
    }

    let rendered = AudioBlock {
        channels,
        frame_count,
        presentation_time_seconds: full_range_render.presentation_time_seconds,
        discontinuity: full_range_render.discontinuity,
    };
    rendered
        .validate()
        .map_err(|_| MpeghHoaCandidateError::InvalidRenderedGeometry)?;
    Ok(rendered)
}

fn candidate_speakers(
    pair: &MpeghPairedEvidence,
) -> Result<Vec<CandidateSpeaker>, MpeghHoaCandidateError> {
    if !pair.reference_layout.speakers.is_empty() {
        return Ok(pair
            .reference_layout
            .speakers
            .iter()
            .map(|speaker| CandidateSpeaker {
                direction: SpeakerDirection {
                    azimuth_degrees: f64::from(speaker.azimuth_degrees),
                    elevation_degrees: f64::from(speaker.elevation_degrees),
                },
                is_lfe: speaker.is_lfe,
            })
            .collect());
    }

    let cicp_index = u8::try_from(pair.reference_layout.cicp_index)
        .map_err(|_| MpeghHoaCandidateError::MissingReferenceSpeakerLayout)?;
    let members = cicp_layout_members(cicp_index)
        .ok_or(MpeghHoaCandidateError::MissingReferenceSpeakerLayout)?;
    let mut speakers = Vec::with_capacity(members.len());
    for member_index in 0..members.len() {
        let geometry = cicp_layout_member_geometry(cicp_index, member_index as u16).ok_or(
            MpeghHoaCandidateError::UnresolvedCicpSpeaker {
                cicp_index,
                member_index,
            },
        )?;
        speakers.push(CandidateSpeaker {
            direction: SpeakerDirection {
                azimuth_degrees: f64::from(geometry.azimuth_degrees),
                elevation_degrees: f64::from(geometry.elevation_degrees),
            },
            is_lfe: geometry.is_lfe,
        });
    }
    Ok(speakers)
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum MpeghHoaCandidateError {
    #[error("MPEG-H HOA candidate rendering currently requires a pure HOA scene, got {domain}")]
    MixedSceneUnsupported { domain: String },
    #[error("MPEG-H scene declares pure HOA but still contains bed/object render state")]
    MixedSceneStatePresent,
    #[error("paired MPEG-H evidence has no decoded HOA coefficient frame")]
    MissingHoaCoefficients,
    #[error("paired MPEG-H HOA coefficients failed validation: {0}")]
    InvalidHoaCoefficients(String),
    #[error("paired MPEG-H evidence has no resolvable reference speaker geometry")]
    MissingReferenceSpeakerLayout,
    #[error("CICP layout {cicp_index} member {member_index} has no resolvable speaker geometry")]
    UnresolvedCicpSpeaker {
        cicp_index: u8,
        member_index: usize,
    },
    #[error("MPEG-H reference layout contains no full-range speakers")]
    NoFullRangeSpeakers,
    #[error(transparent)]
    Matrix(#[from] HoaMatrixGenerationError),
    #[error(transparent)]
    Renderer(#[from] HoaRendererError),
    #[error("MPEG-H HOA candidate produced invalid output geometry")]
    InvalidRenderedGeometry,
}
