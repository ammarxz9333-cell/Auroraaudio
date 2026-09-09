use std::collections::HashSet;

use aurora_core::{AudioBlock, ChannelRole};
use aurora_renderer_hoa::{
    generate_regularized_mode_matching_matrix, render_hoa_coefficients, HoaMatrixGenerationError,
    HoaRendererError, SpeakerDirection,
};
use aurora_spatial_transport_v2::{
    cicp_layout_member_geometry, cicp_layout_members, ResolvedBedSignalTarget,
    SpatialTransportError, TransportSceneDomain,
};
use thiserror::Error;

use crate::{reference_roles, MpeghPairedEvidence, MpeghRoleConformanceError};

const BED_GEOMETRY_TOLERANCE_DEGREES: f64 = 0.25;

#[derive(Debug, Clone, Copy)]
struct CandidateSpeaker {
    direction: SpeakerDirection,
    is_lfe: bool,
}

/// Render a pure-HOA MPEG-H scene to libmpegh's exact reference speaker order.
///
/// This compatibility wrapper intentionally keeps its original pure-HOA
/// contract. Use [`render_mpegh_hoa_candidate`] for the admitted Bed+HOA path.
pub fn render_pure_mpegh_hoa_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
) -> Result<AudioBlock, MpeghHoaCandidateError> {
    if pair.scene.domain != TransportSceneDomain::HoaTransport {
        return Err(MpeghHoaCandidateError::MixedSceneUnsupported {
            domain: format!("{:?}", pair.scene.domain),
        });
    }
    render_mpegh_hoa_candidate(pair, regularization)
}

/// Render the admitted MPEG-H HOA scene classes into the exact speaker order
/// used by the paired libmpegh reference render.
///
/// Supported domains are pure HOA and Bed+HOA. Objects are deliberately
/// rejected until Aurora can render their metadata independently. HOA never
/// contributes to LFE; a signalled bed LFE remains a direct PCM passthrough.
pub fn render_mpegh_hoa_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
) -> Result<AudioBlock, MpeghHoaCandidateError> {
    match pair.scene.domain {
        TransportSceneDomain::HoaTransport | TransportSceneDomain::BedAndHoa => {}
        domain => {
            return Err(MpeghHoaCandidateError::MixedSceneUnsupported {
                domain: format!("{domain:?}"),
            })
        }
    }
    if !pair.scene.frame.spatial.object_signals.is_empty()
        || !pair.scene.frame.spatial.object_updates.is_empty()
    {
        return Err(MpeghHoaCandidateError::ObjectStatePresent);
    }
    if pair.scene.domain == TransportSceneDomain::HoaTransport && !pair.scene.bed_signals.is_empty() {
        return Err(MpeghHoaCandidateError::DomainStateMismatch);
    }
    if pair.scene.domain == TransportSceneDomain::BedAndHoa && pair.scene.bed_signals.is_empty() {
        return Err(MpeghHoaCandidateError::DomainStateMismatch);
    }

    pair.scene
        .validate()
        .map_err(MpeghHoaCandidateError::InvalidTransportScene)?;

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
    if pair.scene.frame.decoded.audio.frame_count != frame_count {
        return Err(MpeghHoaCandidateError::FrameCountMismatch {
            scene: pair.scene.frame.decoded.audio.frame_count,
            hoa: frame_count,
        });
    }

    let mut channels = vec![vec![0.0_f32; frame_count]; speakers.len()];
    for (rendered_index, (reference_index, _)) in full_range.iter().enumerate() {
        channels[*reference_index].copy_from_slice(&full_range_render.channels[rendered_index]);
    }

    if pair.scene.domain == TransportSceneDomain::BedAndHoa {
        mix_bed_into_reference_order(pair, &speakers, &mut channels)?;
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

fn mix_bed_into_reference_order(
    pair: &MpeghPairedEvidence,
    speakers: &[CandidateSpeaker],
    output: &mut [Vec<f32>],
) -> Result<(), MpeghHoaCandidateError> {
    let frame_count = pair.scene.frame.decoded.audio.frame_count;
    let mut destinations = HashSet::with_capacity(pair.scene.bed_signals.len());
    let mut cached_roles: Option<Vec<ChannelRole>> = None;

    for bed in &pair.scene.bed_signals {
        let destination = match bed
            .resolved_target()
            .map_err(MpeghHoaCandidateError::InvalidBedTarget)?
        {
            ResolvedBedSignalTarget::SemanticRole(role) => {
                if cached_roles.is_none() {
                    cached_roles = Some(
                        reference_roles(&pair.reference_layout, speakers.len())
                            .map_err(MpeghHoaCandidateError::ReferenceRoles)?,
                    );
                }
                unique_role_destination(cached_roles.as_ref().expect("set above"), &role)?
            }
            ResolvedBedSignalTarget::Geometry(geometry) => unique_geometry_destination(
                speakers,
                f64::from(geometry.azimuth_degrees),
                f64::from(geometry.elevation_degrees),
                geometry.is_lfe,
            )?,
        };

        if !destinations.insert(destination) {
            return Err(MpeghHoaCandidateError::DuplicateBedDestination { destination });
        }

        let source = pair
            .scene
            .frame
            .decoded
            .audio
            .channels
            .get(bed.pcm_channel_index)
            .ok_or(MpeghHoaCandidateError::BedLaneOutOfRange {
                lane: bed.pcm_channel_index,
            })?;
        if source.len() != frame_count {
            return Err(MpeghHoaCandidateError::InvalidBedPlaneLength {
                lane: bed.pcm_channel_index,
                expected: frame_count,
                actual: source.len(),
            });
        }
        let destination_plane = output
            .get_mut(destination)
            .ok_or(MpeghHoaCandidateError::BedDestinationOutOfRange { destination })?;
        for (out, sample) in destination_plane.iter_mut().zip(source.iter().copied()) {
            *out += sample;
        }
    }
    Ok(())
}

fn unique_role_destination(
    roles: &[ChannelRole],
    requested: &ChannelRole,
) -> Result<usize, MpeghHoaCandidateError> {
    let matches = roles
        .iter()
        .enumerate()
        .filter_map(|(index, role)| (role == requested).then_some(index))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => Err(MpeghHoaCandidateError::MissingBedRole {
            role: requested.clone(),
        }),
        _ => Err(MpeghHoaCandidateError::AmbiguousBedRole {
            role: requested.clone(),
        }),
    }
}

fn unique_geometry_destination(
    speakers: &[CandidateSpeaker],
    azimuth_degrees: f64,
    elevation_degrees: f64,
    is_lfe: bool,
) -> Result<usize, MpeghHoaCandidateError> {
    let matches = speakers
        .iter()
        .enumerate()
        .filter_map(|(index, speaker)| {
            let azimuth_error = angular_distance_degrees(
                speaker.direction.azimuth_degrees,
                azimuth_degrees,
            );
            let elevation_error =
                (speaker.direction.elevation_degrees - elevation_degrees).abs();
            (speaker.is_lfe == is_lfe
                && azimuth_error <= BED_GEOMETRY_TOLERANCE_DEGREES
                && elevation_error <= BED_GEOMETRY_TOLERANCE_DEGREES)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => Err(MpeghHoaCandidateError::MissingBedGeometry {
            azimuth_degrees,
            elevation_degrees,
            is_lfe,
        }),
        _ => Err(MpeghHoaCandidateError::AmbiguousBedGeometry {
            azimuth_degrees,
            elevation_degrees,
            is_lfe,
        }),
    }
}

fn angular_distance_degrees(a: f64, b: f64) -> f64 {
    let mut delta = (a - b).rem_euclid(360.0);
    if delta > 180.0 {
        delta = 360.0 - delta;
    }
    delta
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
    #[error("MPEG-H HOA candidate rendering does not support scene domain {domain}")]
    MixedSceneUnsupported { domain: String },
    #[error("MPEG-H HOA candidate scene contains object signals or object updates")]
    ObjectStatePresent,
    #[error("MPEG-H HOA candidate domain does not match its bed/HOA state")]
    DomainStateMismatch,
    #[error("MPEG-H transport scene failed validation: {0}")]
    InvalidTransportScene(SpatialTransportError),
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
    #[error("MPEG-H scene contains {scene} samples but HOA coefficients contain {hoa}")]
    FrameCountMismatch { scene: usize, hoa: usize },
    #[error("MPEG-H bed target failed resolution: {0}")]
    InvalidBedTarget(SpatialTransportError),
    #[error("MPEG-H reference semantic roles cannot be resolved losslessly: {0}")]
    ReferenceRoles(MpeghRoleConformanceError),
    #[error("MPEG-H reference layout has no destination for bed role '{role}'")]
    MissingBedRole { role: ChannelRole },
    #[error("MPEG-H reference layout maps bed role '{role}' ambiguously")]
    AmbiguousBedRole { role: ChannelRole },
    #[error("MPEG-H reference layout has no speaker at lfe={is_lfe} az={azimuth_degrees} el={elevation_degrees}")]
    MissingBedGeometry {
        azimuth_degrees: f64,
        elevation_degrees: f64,
        is_lfe: bool,
    },
    #[error("MPEG-H reference layout has multiple speakers at lfe={is_lfe} az={azimuth_degrees} el={elevation_degrees}")]
    AmbiguousBedGeometry {
        azimuth_degrees: f64,
        elevation_degrees: f64,
        is_lfe: bool,
    },
    #[error("multiple MPEG-H bed signals target reference speaker {destination}")]
    DuplicateBedDestination { destination: usize },
    #[error("MPEG-H bed PCM lane {lane} is outside the scene audio block")]
    BedLaneOutOfRange { lane: usize },
    #[error("MPEG-H bed PCM lane {lane} has {actual} samples, expected {expected}")]
    InvalidBedPlaneLength {
        lane: usize,
        expected: usize,
        actual: usize,
    },
    #[error("MPEG-H bed destination {destination} is outside candidate output")]
    BedDestinationOutOfRange { destination: usize },
    #[error(transparent)]
    Matrix(#[from] HoaMatrixGenerationError),
    #[error(transparent)]
    Renderer(#[from] HoaRendererError),
    #[error("MPEG-H HOA candidate produced invalid output geometry")]
    InvalidRenderedGeometry,
}
