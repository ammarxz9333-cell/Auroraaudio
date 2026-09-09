use std::collections::HashMap;

use aurora_core::{AudioBlock, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererError, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::Vbap3dRenderer;
use aurora_spatial_ir_v2::{
    CoordinateSpace, SpatialPosition, SpatialRenderingProperties,
};
use aurora_spatial_transport_v2::{
    cicp_layout_member_geometry, cicp_layout_members, TransportSceneDomain,
};
use thiserror::Error;

use crate::{reference_roles, MpeghPairedEvidence, MpeghRoleConformanceError};

const EPSILON: f32 = 1.0e-8;

/// Render the object contribution of a conservative MPEG-H Objects+HOA scene.
///
/// Admission is intentionally narrow: every object must be active for the whole
/// access unit, have exactly one metadata update at sample zero, carry no ramp,
/// spread, zone/exclusion, snap, screen or other V2-only render property, and
/// use listener-relative MPEG-H spherical coordinates. Dynamic/rich OAM remains
/// on the libmpegh reference path until Aurora's stateful object renderer is
/// admitted for the same semantics.
pub fn render_static_mpegh_object_plane(
    pair: &MpeghPairedEvidence,
) -> Result<AudioBlock, MpeghObjectCandidateError> {
    if pair.scene.domain != TransportSceneDomain::ObjectsAndHoa {
        return Err(MpeghObjectCandidateError::UnsupportedDomain {
            domain: format!("{:?}", pair.scene.domain),
        });
    }
    if !pair.scene.bed_signals.is_empty() {
        return Err(MpeghObjectCandidateError::BedStatePresent);
    }
    pair.scene
        .validate()
        .map_err(|error| MpeghObjectCandidateError::InvalidTransport(error.to_string()))?;

    let frame = &pair.scene.frame;
    let object_count = frame.spatial.object_signals.len();
    if object_count == 0 {
        return Err(MpeghObjectCandidateError::NoObjects);
    }

    let reference_speakers = reference_speakers(pair)?;
    let roles = reference_roles(&pair.reference_layout, reference_speakers.len())?;
    let speakers = reference_speakers
        .iter()
        .zip(roles)
        .enumerate()
        .map(|(index, (geometry, role))| Speaker {
            id: format!("mpegh-reference-{index}"),
            label: format!("MPEG-H reference {index}"),
            channel_role: role,
            position: spherical_direction_to_cartesian(
                f32::from(geometry.azimuth_degrees),
                f32::from(geometry.elevation_degrees),
                1.0,
            ),
            orientation: Vector3::ZERO,
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        })
        .collect::<Vec<_>>();

    let listener = Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 0.0,
    };
    let frame_count = frame.decoded.audio.frame_count;
    let mut renderer = Vbap3dRenderer::new().with_smoothing(1.0);
    renderer.configure(
        speakers,
        48_000,
        frame_count.max(1),
        object_count,
    )?;
    renderer.prepare_listener(&listener)?;
    let mut scratch = RendererScratch::new(renderer.required_scratch_size()?);
    let speaker_count = renderer.output_channel_count();
    let mut gains = vec![SpeakerGain::default(); object_count * speaker_count];

    let updates = static_object_updates(pair)?;
    let mut render_objects = Vec::with_capacity(object_count);
    for signal in &frame.spatial.object_signals {
        let update = updates
            .get(signal.id.as_str())
            .copied()
            .ok_or_else(|| MpeghObjectCandidateError::MissingObjectUpdate {
                id: signal.id.clone(),
            })?;
        let position = match (update.coordinate_space, update.position) {
            (
                CoordinateSpace::SphericalDegrees,
                SpatialPosition::Spherical {
                    azimuth_degrees,
                    elevation_degrees,
                    distance,
                },
            ) => spherical_direction_to_cartesian(
                azimuth_degrees,
                elevation_degrees,
                distance.max(EPSILON),
            ),
            _ => {
                return Err(MpeghObjectCandidateError::UnsupportedCoordinateSpace {
                    id: signal.id.clone(),
                })
            }
        };
        render_objects.push(RenderObject {
            position,
            gain: db_to_linear(update.gain_db),
        });
    }

    renderer.render_gains(&listener, &render_objects, &mut gains, &mut scratch)?;

    let mut output = AudioBlock {
        channels: vec![vec![0.0; frame_count]; speaker_count],
        frame_count,
        presentation_time_seconds: frame.decoded.audio.presentation_time_seconds,
        discontinuity: frame.decoded.audio.discontinuity,
    };
    for (object_index, signal) in frame.spatial.object_signals.iter().enumerate() {
        let source = frame
            .decoded
            .audio
            .channels
            .get(signal.pcm_channel_index)
            .ok_or(MpeghObjectCandidateError::PcmLaneOutOfRange {
                lane: signal.pcm_channel_index,
            })?;
        if source.len() != frame_count {
            return Err(MpeghObjectCandidateError::InvalidPcmPlaneLength {
                lane: signal.pcm_channel_index,
                expected: frame_count,
                actual: source.len(),
            });
        }
        let object_gains = &gains[object_index * speaker_count..(object_index + 1) * speaker_count];
        for (speaker_index, gain) in object_gains.iter().enumerate() {
            for (destination, sample) in output.channels[speaker_index]
                .iter_mut()
                .zip(source.iter().copied())
            {
                *destination += sample * gain.gain;
            }
        }
    }
    output
        .validate()
        .map_err(|_| MpeghObjectCandidateError::InvalidRenderedGeometry)?;
    Ok(output)
}

fn static_object_updates<'a>(
    pair: &'a MpeghPairedEvidence,
) -> Result<HashMap<&'a str, &'a aurora_spatial_ir_v2::SpatialObjectUpdate>, MpeghObjectCandidateError>
{
    let mut updates = HashMap::with_capacity(pair.scene.frame.spatial.object_signals.len());
    for update in &pair.scene.frame.spatial.object_updates {
        if update.metadata_sample_offset != 0 || update.ramp_duration_samples != 0 || !update.active {
            return Err(MpeghObjectCandidateError::DynamicObjectMetadata {
                id: update.object_id.clone(),
            });
        }
        if update.spread > EPSILON || update.rendering != SpatialRenderingProperties::default() {
            return Err(MpeghObjectCandidateError::RichObjectMetadata {
                id: update.object_id.clone(),
            });
        }
        if updates.insert(update.object_id.as_str(), update).is_some() {
            return Err(MpeghObjectCandidateError::MultipleObjectUpdates {
                id: update.object_id.clone(),
            });
        }
    }
    if updates.len() != pair.scene.frame.spatial.object_signals.len() {
        return Err(MpeghObjectCandidateError::IncompleteObjectState {
            signals: pair.scene.frame.spatial.object_signals.len(),
            updates: updates.len(),
        });
    }
    Ok(updates)
}

fn reference_speakers(pair: &MpeghPairedEvidence) -> Result<Vec<crate::MpeghSpeaker>, MpeghObjectCandidateError> {
    if !pair.reference_layout.speakers.is_empty() {
        return Ok(pair.reference_layout.speakers.clone());
    }
    let cicp_index = u8::try_from(pair.reference_layout.cicp_index)
        .map_err(|_| MpeghObjectCandidateError::MissingReferenceSpeakerLayout)?;
    let members = cicp_layout_members(cicp_index)
        .ok_or(MpeghObjectCandidateError::MissingReferenceSpeakerLayout)?;
    let mut result = Vec::with_capacity(members.len());
    for member_index in 0..members.len() {
        let geometry = cicp_layout_member_geometry(cicp_index, member_index as u16).ok_or(
            MpeghObjectCandidateError::UnresolvedCicpSpeaker {
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

fn spherical_direction_to_cartesian(
    azimuth_degrees: f32,
    elevation_degrees: f32,
    distance: f32,
) -> Vector3 {
    let azimuth = azimuth_degrees.to_radians();
    let elevation = elevation_degrees.to_radians();
    let horizontal = elevation.cos() * distance;
    Vector3::new(
        -azimuth.sin() * horizontal,
        azimuth.cos() * horizontal,
        elevation.sin() * distance,
    )
}

fn db_to_linear(db: f32) -> f32 {
    if db == f32::NEG_INFINITY {
        0.0
    } else {
        10.0_f32.powf(db / 20.0)
    }
}

#[derive(Debug, Error)]
pub enum MpeghObjectCandidateError {
    #[error("MPEG-H point-object candidate currently requires ObjectsAndHoa, got {domain}")]
    UnsupportedDomain { domain: String },
    #[error("MPEG-H ObjectsAndHoa candidate unexpectedly contains bed signals")]
    BedStatePresent,
    #[error("MPEG-H transport scene failed validation: {0}")]
    InvalidTransport(String),
    #[error("MPEG-H ObjectsAndHoa scene contains no objects")]
    NoObjects,
    #[error("paired MPEG-H evidence has no resolvable reference speaker layout")]
    MissingReferenceSpeakerLayout,
    #[error("CICP layout {cicp_index} member {member_index} has no speaker geometry")]
    UnresolvedCicpSpeaker {
        cicp_index: u8,
        member_index: usize,
    },
    #[error("object '{id}' has no admitted metadata update")]
    MissingObjectUpdate { id: String },
    #[error("object '{id}' carries dynamic metadata not admitted by the static point-object candidate")]
    DynamicObjectMetadata { id: String },
    #[error("object '{id}' carries spread or rich renderer metadata not admitted by the static point-object candidate")]
    RichObjectMetadata { id: String },
    #[error("object '{id}' has more than one metadata update")]
    MultipleObjectUpdates { id: String },
    #[error("object signal/update ownership is incomplete: {signals} signals, {updates} updates")]
    IncompleteObjectState { signals: usize, updates: usize },
    #[error("object '{id}' does not use admitted listener-relative spherical coordinates")]
    UnsupportedCoordinateSpace { id: String },
    #[error("decoded PCM lane {lane} is unavailable")]
    PcmLaneOutOfRange { lane: usize },
    #[error("decoded PCM lane {lane} has {actual} samples, expected {expected}")]
    InvalidPcmPlaneLength {
        lane: usize,
        expected: usize,
        actual: usize,
    },
    #[error("MPEG-H object candidate produced invalid output geometry")]
    InvalidRenderedGeometry,
    #[error(transparent)]
    Roles(#[from] MpeghRoleConformanceError),
    #[error(transparent)]
    Renderer(#[from] RendererError),
}
