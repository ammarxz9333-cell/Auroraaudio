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
    evaluate_paired_mpegh_candidate, reference_roles, render_exact_mpegh_object_plane,
    MpeghConformancePolicy, MpeghObjectTimelineError, MpeghPairedEvidence, MpeghPairedGateError,
    MpeghPairedPlaybackDecision, MpeghRoleConformanceError,
};

const BED_GEOMETRY_TOLERANCE_DEGREES: f64 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghExactSceneDecision {
    pub candidate: AudioBlock,
    pub playback: MpeghPairedPlaybackDecision,
}

/// Render the currently admitted exact point-object MPEG-H scene.
///
/// Supports Objects+HOA and Bed+Objects+HOA. Object OAM state changes are
/// applied at exact sample offsets. HOA coefficients are rendered into the
/// exact libmpegh reference speaker order and never into LFE. Bed signals are
/// direct-routed only when their destination is unique. Rich OAM properties,
/// ramps and cross-access-unit state remain fail-closed in the object plane.
pub fn render_exact_mpegh_scene_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
) -> Result<AudioBlock, MpeghExactSceneError> {
    match pair.scene.domain {
        TransportSceneDomain::ObjectsAndHoa | TransportSceneDomain::BedObjectsAndHoa => {}
        domain => {
            return Err(MpeghExactSceneError::UnsupportedDomain {
                domain: format!("{domain:?}"),
            })
        }
    }
    pair.scene
        .validate()
        .map_err(|error| MpeghExactSceneError::InvalidTransport(error.to_string()))?;

    let object_plane = render_exact_mpegh_object_plane(pair)?;
    let mut candidate = render_hoa_plane(pair, regularization)?;
    ensure_same_geometry(&object_plane, &candidate)?;
    add_plane(&mut candidate, &object_plane);

    if pair.scene.domain == TransportSceneDomain::BedObjectsAndHoa {
        let speakers = reference_speakers(pair)?;
        mix_bed(pair, &speakers, &mut candidate)?;
    }

    candidate
        .validate()
        .map_err(|_| MpeghExactSceneError::InvalidRenderedGeometry)?;
    Ok(candidate)
}

/// Evidence-gate the complete exact scene only. The candidate is never
/// authorized for playback without a passing same-access-unit libmpegh
/// comparison.
pub fn evaluate_exact_mpegh_scene_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<MpeghExactSceneDecision, MpeghExactSceneError> {
    let reference = pair
        .reference
        .as_ref()
        .ok_or(MpeghExactSceneError::MissingReferenceRender)?;
    let candidate = render_exact_mpegh_scene_candidate(pair, regularization)?;
    let roles = reference_roles(&pair.reference_layout, candidate.channels.len())?;
    let playback = evaluate_paired_mpegh_candidate(
        pair,
        &candidate,
        reference.sample_rate,
        &roles,
        policy,
    )?;
    Ok(MpeghExactSceneDecision { candidate, playback })
}

fn render_hoa_plane(
    pair: &MpeghPairedEvidence,
    regularization: f64,
) -> Result<AudioBlock, MpeghExactSceneError> {
    let coefficients = pair
        .hoa_coefficients
        .as_ref()
        .ok_or(MpeghExactSceneError::MissingHoaCoefficients)?;
    coefficients
        .validate()
        .map_err(|error| MpeghExactSceneError::InvalidHoaCoefficients(error.to_string()))?;
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
        return Err(MpeghExactSceneError::NoFullRangeSpeakers);
    }
    let directions = full_range.iter().map(|(_, direction)| *direction).collect::<Vec<_>>();
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

fn ensure_same_geometry(a: &AudioBlock, b: &AudioBlock) -> Result<(), MpeghExactSceneError> {
    if a.frame_count != b.frame_count || a.channels.len() != b.channels.len() {
        return Err(MpeghExactSceneError::PlaneGeometryMismatch {
            object_channels: a.channels.len(),
            hoa_channels: b.channels.len(),
            object_frames: a.frame_count,
            hoa_frames: b.frame_count,
        });
    }
    Ok(())
}

fn add_plane(destination: &mut AudioBlock, source: &AudioBlock) {
    for (destination_channel, source_channel) in destination.channels.iter_mut().zip(&source.channels) {
        for (out, sample) in destination_channel.iter_mut().zip(source_channel.iter().copied()) {
            *out += sample;
        }
    }
}

fn mix_bed(
    pair: &MpeghPairedEvidence,
    speakers: &[crate::MpeghSpeaker],
    output: &mut AudioBlock,
) -> Result<(), MpeghExactSceneError> {
    let frame_count = pair.scene.frame.decoded.audio.frame_count;
    let mut destinations = HashSet::with_capacity(pair.scene.bed_signals.len());
    let mut cached_roles: Option<Vec<ChannelRole>> = None;
    for bed in &pair.scene.bed_signals {
        let destination = match bed
            .resolved_target()
            .map_err(MpeghExactSceneError::InvalidBedTarget)?
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
            return Err(MpeghExactSceneError::DuplicateBedDestination { destination });
        }
        let source = pair
            .scene
            .frame
            .decoded
            .audio
            .channels
            .get(bed.pcm_channel_index)
            .ok_or(MpeghExactSceneError::BedLaneOutOfRange {
                lane: bed.pcm_channel_index,
            })?;
        if source.len() != frame_count {
            return Err(MpeghExactSceneError::InvalidBedPlaneLength {
                lane: bed.pcm_channel_index,
                expected: frame_count,
                actual: source.len(),
            });
        }
        for (out, sample) in output.channels[destination].iter_mut().zip(source.iter().copied()) {
            *out += sample;
        }
    }
    Ok(())
}

fn unique_role_destination(
    roles: &[ChannelRole],
    requested: &ChannelRole,
) -> Result<usize, MpeghExactSceneError> {
    let mut found = None;
    for (index, role) in roles.iter().enumerate() {
        if role == requested {
            if found.replace(index).is_some() {
                return Err(MpeghExactSceneError::AmbiguousBedRole {
                    role: requested.clone(),
                });
            }
        }
    }
    found.ok_or_else(|| MpeghExactSceneError::MissingBedRole {
        role: requested.clone(),
    })
}

fn unique_geometry_destination(
    speakers: &[crate::MpeghSpeaker],
    azimuth_degrees: f64,
    elevation_degrees: f64,
    is_lfe: bool,
) -> Result<usize, MpeghExactSceneError> {
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
                return Err(MpeghExactSceneError::AmbiguousBedGeometry {
                    azimuth_degrees,
                    elevation_degrees,
                    is_lfe,
                });
            }
        }
    }
    found.ok_or(MpeghExactSceneError::MissingBedGeometry {
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
) -> Result<Vec<crate::MpeghSpeaker>, MpeghExactSceneError> {
    if !pair.reference_layout.speakers.is_empty() {
        return Ok(pair.reference_layout.speakers.clone());
    }
    let cicp_index = u8::try_from(pair.reference_layout.cicp_index)
        .map_err(|_| MpeghExactSceneError::MissingReferenceSpeakerLayout)?;
    let members = cicp_layout_members(cicp_index)
        .ok_or(MpeghExactSceneError::MissingReferenceSpeakerLayout)?;
    let mut result = Vec::with_capacity(members.len());
    for member_index in 0..members.len() {
        let geometry = cicp_layout_member_geometry(cicp_index, member_index as u16).ok_or(
            MpeghExactSceneError::UnresolvedCicpSpeaker {
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
pub enum MpeghExactSceneError {
    #[error("MPEG-H exact scene candidate does not support scene domain {domain}")]
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
    UnresolvedCicpSpeaker { cicp_index: u8, member_index: usize },
    #[error("MPEG-H reference layout contains no full-range speakers")]
    NoFullRangeSpeakers,
    #[error("object and HOA candidate planes disagree: objects={object_channels}ch/{object_frames}f, HOA={hoa_channels}ch/{hoa_frames}f")]
    PlaneGeometryMismatch {
        object_channels: usize,
        hoa_channels: usize,
        object_frames: usize,
        hoa_frames: usize,
    },
    #[error("MPEG-H bed target failed resolution: {0}")]
    InvalidBedTarget(SpatialTransportError),
    #[error("reference layout has no destination for bed role '{role}'")]
    MissingBedRole { role: ChannelRole },
    #[error("reference layout maps bed role '{role}' ambiguously")]
    AmbiguousBedRole { role: ChannelRole },
    #[error("reference layout has no speaker at lfe={is_lfe} az={azimuth_degrees} el={elevation_degrees}")]
    MissingBedGeometry { azimuth_degrees: f64, elevation_degrees: f64, is_lfe: bool },
    #[error("reference layout has multiple speakers at lfe={is_lfe} az={azimuth_degrees} el={elevation_degrees}")]
    AmbiguousBedGeometry { azimuth_degrees: f64, elevation_degrees: f64, is_lfe: bool },
    #[error("multiple bed signals target reference speaker {destination}")]
    DuplicateBedDestination { destination: usize },
    #[error("bed PCM lane {lane} is outside the scene audio block")]
    BedLaneOutOfRange { lane: usize },
    #[error("bed PCM lane {lane} has {actual} samples, expected {expected}")]
    InvalidBedPlaneLength { lane: usize, expected: usize, actual: usize },
    #[error("exact MPEG-H scene candidate produced invalid output geometry")]
    InvalidRenderedGeometry,
    #[error(transparent)]
    Objects(#[from] MpeghObjectTimelineError),
    #[error(transparent)]
    Matrix(#[from] HoaMatrixGenerationError),
    #[error(transparent)]
    Renderer(#[from] HoaRendererError),
    #[error(transparent)]
    Roles(#[from] MpeghRoleConformanceError),
    #[error(transparent)]
    Playback(#[from] MpeghPairedGateError),
}
