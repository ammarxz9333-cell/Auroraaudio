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

use crate::{
    evaluate_paired_mpegh_candidate, reference_roles, render_stateful_mpegh_object_plane,
    MpeghConformancePolicy, MpeghObjectStateCache, MpeghPairedEvidence, MpeghPairedGateError,
    MpeghPairedPlaybackDecision, MpeghRoleConformanceError, MpeghStatefulObjectError,
};

const BED_GEOMETRY_TOLERANCE_DEGREES: f64 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghStatefulSceneDecision {
    pub candidate: AudioBlock,
    pub playback: MpeghPairedPlaybackDecision,
}

/// Render any currently admitted object-bearing MPEG-H scene while carrying
/// point-object state across access units.
///
/// HOA is added only for HOA domains, bed signals only for bed domains, and all
/// contributions are produced in libmpegh's exact reference speaker order.
/// Rich OAM semantics not yet independently modeled by Aurora still fail closed
/// and are expected to select the paired reference at the higher evidence layer.
pub fn render_stateful_mpegh_scene_candidate(
    pair: &MpeghPairedEvidence,
    object_state: &mut MpeghObjectStateCache,
    regularization: f64,
) -> Result<AudioBlock, MpeghStatefulSceneError> {
    let (has_bed, has_hoa) = match pair.scene.domain {
        TransportSceneDomain::ObjectSignals => (false, false),
        TransportSceneDomain::BedAndObjects => (true, false),
        TransportSceneDomain::ObjectsAndHoa => (false, true),
        TransportSceneDomain::BedObjectsAndHoa => (true, true),
        domain => {
            return Err(MpeghStatefulSceneError::UnsupportedDomain {
                domain: format!("{domain:?}"),
            })
        }
    };
    pair.scene
        .validate()
        .map_err(|error| MpeghStatefulSceneError::InvalidTransport(error.to_string()))?;

    let object_plane = render_stateful_mpegh_object_plane(pair, object_state)?;
    let mut candidate = if has_hoa {
        let mut hoa = render_hoa_plane(pair, regularization)?;
        ensure_same_geometry(&object_plane, &hoa)?;
        add_plane(&mut hoa, &object_plane);
        hoa
    } else {
        object_plane
    };

    if has_bed {
        let speakers = reference_speakers(pair)?;
        mix_bed(pair, &speakers, &mut candidate)?;
    }

    candidate
        .validate()
        .map_err(|_| MpeghStatefulSceneError::InvalidRenderedGeometry)?;
    Ok(candidate)
}

pub fn evaluate_stateful_mpegh_scene_candidate(
    pair: &MpeghPairedEvidence,
    object_state: &mut MpeghObjectStateCache,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<MpeghStatefulSceneDecision, MpeghStatefulSceneError> {
    let reference = pair
        .reference
        .as_ref()
        .ok_or(MpeghStatefulSceneError::MissingReferenceRender)?;
    let candidate = render_stateful_mpegh_scene_candidate(pair, object_state, regularization)?;
    let roles = reference_roles(&pair.reference_layout, candidate.channels.len())?;
    let playback = evaluate_paired_mpegh_candidate(
        pair,
        &candidate,
        reference.sample_rate,
        &roles,
        policy,
    )?;
    Ok(MpeghStatefulSceneDecision { candidate, playback })
}

fn render_hoa_plane(
    pair: &MpeghPairedEvidence,
    regularization: f64,
) -> Result<AudioBlock, MpeghStatefulSceneError> {
    let coefficients = pair
        .hoa_coefficients
        .as_ref()
        .ok_or(MpeghStatefulSceneError::MissingHoaCoefficients)?;
    coefficients
        .validate()
        .map_err(|error| MpeghStatefulSceneError::InvalidHoaCoefficients(error.to_string()))?;
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
        return Err(MpeghStatefulSceneError::NoFullRangeSpeakers);
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

fn ensure_same_geometry(
    object_plane: &AudioBlock,
    hoa_plane: &AudioBlock,
) -> Result<(), MpeghStatefulSceneError> {
    if object_plane.frame_count != hoa_plane.frame_count
        || object_plane.channels.len() != hoa_plane.channels.len()
    {
        return Err(MpeghStatefulSceneError::PlaneGeometryMismatch {
            object_channels: object_plane.channels.len(),
            hoa_channels: hoa_plane.channels.len(),
            object_frames: object_plane.frame_count,
            hoa_frames: hoa_plane.frame_count,
        });
    }
    Ok(())
}

fn add_plane(destination: &mut AudioBlock, source: &AudioBlock) {
    for (destination_channel, source_channel) in
        destination.channels.iter_mut().zip(&source.channels)
    {
        for (out, sample) in destination_channel
            .iter_mut()
            .zip(source_channel.iter().copied())
        {
            *out += sample;
        }
    }
}

fn mix_bed(
    pair: &MpeghPairedEvidence,
    speakers: &[crate::MpeghSpeaker],
    output: &mut AudioBlock,
) -> Result<(), MpeghStatefulSceneError> {
    let frame_count = pair.scene.frame.decoded.audio.frame_count;
    if output.frame_count != frame_count || output.channels.len() != speakers.len() {
        return Err(MpeghStatefulSceneError::OutputGeometryMismatch {
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
            .map_err(MpeghStatefulSceneError::InvalidBedTarget)?
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
            return Err(MpeghStatefulSceneError::DuplicateBedDestination { destination });
        }
        let source = pair
            .scene
            .frame
            .decoded
            .audio
            .channels
            .get(bed.pcm_channel_index)
            .ok_or(MpeghStatefulSceneError::BedLaneOutOfRange {
                lane: bed.pcm_channel_index,
            })?;
        if source.len() != frame_count {
            return Err(MpeghStatefulSceneError::InvalidBedPlaneLength {
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
) -> Result<usize, MpeghStatefulSceneError> {
    let mut found = None;
    for (index, role) in roles.iter().enumerate() {
        if role == requested {
            if found.replace(index).is_some() {
                return Err(MpeghStatefulSceneError::AmbiguousBedRole {
                    role: requested.clone(),
                });
            }
        }
    }
    found.ok_or_else(|| MpeghStatefulSceneError::MissingBedRole {
        role: requested.clone(),
    })
}

fn unique_geometry_destination(
    speakers: &[crate::MpeghSpeaker],
    azimuth_degrees: f64,
    elevation_degrees: f64,
    is_lfe: bool,
) -> Result<usize, MpeghStatefulSceneError> {
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
                return Err(MpeghStatefulSceneError::AmbiguousBedGeometry {
                    azimuth_degrees,
                    elevation_degrees,
                    is_lfe,
                });
            }
        }
    }
    found.ok_or(MpeghStatefulSceneError::MissingBedGeometry {
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
) -> Result<Vec<crate::MpeghSpeaker>, MpeghStatefulSceneError> {
    if !pair.reference_layout.speakers.is_empty() {
        return Ok(pair.reference_layout.speakers.clone());
    }
    let cicp_index = u8::try_from(pair.reference_layout.cicp_index)
        .map_err(|_| MpeghStatefulSceneError::MissingReferenceSpeakerLayout)?;
    let members = cicp_layout_members(cicp_index)
        .ok_or(MpeghStatefulSceneError::MissingReferenceSpeakerLayout)?;
    let mut result = Vec::with_capacity(members.len());
    for member_index in 0..members.len() {
        let geometry = cicp_layout_member_geometry(cicp_index, member_index as u16).ok_or(
            MpeghStatefulSceneError::UnresolvedCicpSpeaker {
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
pub enum MpeghStatefulSceneError {
    #[error("MPEG-H stateful scene candidate does not support scene domain {domain}")]
    UnsupportedDomain { domain: String },
    #[error("paired MPEG-H evidence has no libmpegh reference render")]
    MissingReferenceRender,
    #[error("paired MPEG-H evidence has no decoded HOA coefficient frame")]
    MissingHoaCoefficients,
    #[error("paired MPEG-H HOA coefficients failed validation: {0}")]
    InvalidHoaCoefficients(String),
    #[error("MPEG-H transport scene failed validation: {0}")]
    InvalidTransport(String),
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
    #[error("MPEG-H stateful output geometry differs from reference: output={output_channels}ch/{output_frames}f, reference={reference_speakers}ch/{scene_frames}f")]
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
    #[error("stateful MPEG-H scene candidate produced invalid output geometry")]
    InvalidRenderedGeometry,
    #[error(transparent)]
    Objects(#[from] MpeghStatefulObjectError),
    #[error(transparent)]
    Matrix(#[from] HoaMatrixGenerationError),
    #[error(transparent)]
    Renderer(#[from] HoaRendererError),
    #[error(transparent)]
    Roles(#[from] MpeghRoleConformanceError),
    #[error(transparent)]
    Playback(#[from] MpeghPairedGateError),
}
