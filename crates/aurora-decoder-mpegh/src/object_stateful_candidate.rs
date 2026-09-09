use std::collections::HashMap;

use aurora_core::{AudioBlock, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererError, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::Vbap3dRenderer;
use aurora_spatial_ir_v2::{
    CoordinateSpace, SpatialObjectUpdate, SpatialPosition, SpatialRenderingProperties,
};
use aurora_spatial_transport_v2::{
    cicp_layout_member_geometry, cicp_layout_members, TransportSceneDomain,
};
use thiserror::Error;

use crate::{reference_roles, MpeghPairedEvidence, MpeghRoleConformanceError};

const EPSILON: f32 = 1.0e-8;
const FALLBACK_SAMPLE_RATE: u32 = 48_000;

/// Cross-access-unit point-object state for MPEG-H OAM candidate rendering.
///
/// The cache contains only renderer-admitted point state. Rich metadata that
/// Aurora does not yet model independently is rejected before it can enter this
/// cache. A transport/audio discontinuity clears all carried state.
#[derive(Debug, Default, Clone)]
pub struct MpeghObjectStateCache {
    states: HashMap<String, RenderObject>,
}

impl MpeghObjectStateCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.states.clear();
    }

    pub fn len(&self) -> usize {
        self.states.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

/// Exact-per-sample MPEG-H point-object rendering with state carried across
/// access-unit boundaries.
///
/// Unlike the stateless reference candidate, an object does not need a new
/// sample-zero update when the previous accepted access unit already established
/// its state. Metadata at exactly `frame_count` is applied after the current
/// PCM block and becomes the initial state of the next access unit.
pub fn render_stateful_mpegh_object_plane(
    pair: &MpeghPairedEvidence,
    cache: &mut MpeghObjectStateCache,
) -> Result<AudioBlock, MpeghStatefulObjectError> {
    match pair.scene.domain {
        TransportSceneDomain::ObjectSignals
        | TransportSceneDomain::BedAndObjects
        | TransportSceneDomain::ObjectsAndHoa
        | TransportSceneDomain::BedObjectsAndHoa => {}
        domain => {
            return Err(MpeghStatefulObjectError::UnsupportedDomain {
                domain: format!("{domain:?}"),
            })
        }
    }
    pair.scene
        .validate()
        .map_err(|error| MpeghStatefulObjectError::InvalidTransport(error.to_string()))?;

    let frame = &pair.scene.frame;
    if frame.decoded.audio.discontinuity {
        cache.reset();
    }
    let frame_count = frame.decoded.audio.frame_count;
    let object_count = frame.spatial.object_signals.len();
    if object_count == 0 {
        return Err(MpeghStatefulObjectError::NoObjects);
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
            position: spherical_to_cartesian(
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

    let sample_rate = pair
        .reference
        .as_ref()
        .map(|reference| reference.sample_rate)
        .filter(|rate| *rate > 0)
        .unwrap_or(FALLBACK_SAMPLE_RATE);
    let listener = Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 0.0,
    };
    let mut renderer = Vbap3dRenderer::new().with_smoothing(1.0);
    renderer.configure(speakers, sample_rate, frame_count.max(1), object_count)?;
    renderer.prepare_listener(&listener)?;
    let mut scratch = RendererScratch::new(renderer.required_scratch_size()?);
    let speaker_count = renderer.output_channel_count();
    let mut gains = vec![SpeakerGain::default(); object_count * speaker_count];

    let object_index = frame
        .spatial
        .object_signals
        .iter()
        .enumerate()
        .map(|(index, signal)| (signal.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut updates = frame.spatial.object_updates.iter().collect::<Vec<_>>();
    updates.sort_by_key(|update| update.metadata_sample_offset);
    validate_updates(&updates, &object_index, frame_count)?;

    let mut render_objects = vec![
        RenderObject {
            position: Vector3::ZERO,
            gain: 0.0,
        };
        object_count
    ];
    let mut update_index = 0usize;
    let mut output = AudioBlock {
        channels: vec![vec![0.0; frame_count]; speaker_count],
        frame_count,
        presentation_time_seconds: frame.decoded.audio.presentation_time_seconds,
        discontinuity: frame.decoded.audio.discontinuity,
    };

    for sample_index in 0..frame_count {
        while let Some(update) = updates.get(update_index) {
            if update.metadata_sample_offset as usize != sample_index {
                break;
            }
            let state = render_object_from_update(update)?;
            cache.states.insert(update.object_id.clone(), state);
            update_index += 1;
        }

        for (slot, signal) in frame.spatial.object_signals.iter().enumerate() {
            render_objects[slot] = cache
                .states
                .get(&signal.id)
                .copied()
                .ok_or_else(|| MpeghStatefulObjectError::MissingCarriedObjectState {
                    id: signal.id.clone(),
                })?;
        }
        renderer.render_gains(&listener, &render_objects, &mut gains, &mut scratch)?;

        for (slot, signal) in frame.spatial.object_signals.iter().enumerate() {
            let source = frame
                .decoded
                .audio
                .channels
                .get(signal.pcm_channel_index)
                .and_then(|channel| channel.get(sample_index))
                .copied()
                .ok_or(MpeghStatefulObjectError::PcmSampleOutOfRange {
                    lane: signal.pcm_channel_index,
                    sample: sample_index,
                })?;
            let object_gains = &gains[slot * speaker_count..(slot + 1) * speaker_count];
            for (speaker_index, gain) in object_gains.iter().enumerate() {
                output.channels[speaker_index][sample_index] += source * gain.gain;
            }
        }
    }

    // Apply exact frame-boundary updates only after rendering the current PCM.
    while let Some(update) = updates.get(update_index) {
        if update.metadata_sample_offset as usize != frame_count {
            return Err(MpeghStatefulObjectError::UnconsumedMetadata {
                id: update.object_id.clone(),
                offset: update.metadata_sample_offset,
            });
        }
        let state = render_object_from_update(update)?;
        cache.states.insert(update.object_id.clone(), state);
        update_index += 1;
    }

    output
        .validate()
        .map_err(|_| MpeghStatefulObjectError::InvalidRenderedGeometry)?;
    Ok(output)
}

fn validate_updates(
    updates: &[&SpatialObjectUpdate],
    object_index: &HashMap<&str, usize>,
    frame_count: usize,
) -> Result<(), MpeghStatefulObjectError> {
    for update in updates {
        if !object_index.contains_key(update.object_id.as_str()) {
            return Err(MpeghStatefulObjectError::UnknownObject {
                id: update.object_id.clone(),
            });
        }
        if update.metadata_sample_offset as usize > frame_count {
            return Err(MpeghStatefulObjectError::MetadataOffsetOutOfRange {
                id: update.object_id.clone(),
                offset: update.metadata_sample_offset,
                frame_count,
            });
        }
        if update.ramp_duration_samples != 0 {
            return Err(MpeghStatefulObjectError::RampUnsupported {
                id: update.object_id.clone(),
            });
        }
        if update.spread > EPSILON || update.rendering != SpatialRenderingProperties::default() {
            return Err(MpeghStatefulObjectError::RichObjectMetadata {
                id: update.object_id.clone(),
            });
        }
        if !matches!(
            (update.coordinate_space, update.position),
            (
                CoordinateSpace::SphericalDegrees,
                SpatialPosition::Spherical { .. }
            )
        ) {
            return Err(MpeghStatefulObjectError::UnsupportedCoordinateSpace {
                id: update.object_id.clone(),
            });
        }
    }
    Ok(())
}

fn render_object_from_update(
    update: &SpatialObjectUpdate,
) -> Result<RenderObject, MpeghStatefulObjectError> {
    let SpatialPosition::Spherical {
        azimuth_degrees,
        elevation_degrees,
        distance,
    } = update.position
    else {
        return Err(MpeghStatefulObjectError::UnsupportedCoordinateSpace {
            id: update.object_id.clone(),
        });
    };
    Ok(RenderObject {
        position: spherical_to_cartesian(azimuth_degrees, elevation_degrees, distance),
        gain: if update.active {
            db_to_linear(update.gain_db)
        } else {
            0.0
        },
    })
}

fn reference_speakers(
    pair: &MpeghPairedEvidence,
) -> Result<Vec<crate::MpeghSpeaker>, MpeghStatefulObjectError> {
    if !pair.reference_layout.speakers.is_empty() {
        return Ok(pair.reference_layout.speakers.clone());
    }
    let cicp_index = u8::try_from(pair.reference_layout.cicp_index)
        .map_err(|_| MpeghStatefulObjectError::MissingReferenceSpeakerLayout)?;
    let members = cicp_layout_members(cicp_index)
        .ok_or(MpeghStatefulObjectError::MissingReferenceSpeakerLayout)?;
    let mut result = Vec::with_capacity(members.len());
    for member_index in 0..members.len() {
        let geometry = cicp_layout_member_geometry(cicp_index, member_index as u16).ok_or(
            MpeghStatefulObjectError::UnresolvedCicpSpeaker {
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

fn spherical_to_cartesian(
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
pub enum MpeghStatefulObjectError {
    #[error("MPEG-H stateful point-object plane does not support scene domain {domain}")]
    UnsupportedDomain { domain: String },
    #[error("MPEG-H transport scene failed validation: {0}")]
    InvalidTransport(String),
    #[error("MPEG-H scene contains no object signals")]
    NoObjects,
    #[error("paired MPEG-H evidence has no resolvable reference speaker layout")]
    MissingReferenceSpeakerLayout,
    #[error("CICP layout {cicp_index} member {member_index} has no speaker geometry")]
    UnresolvedCicpSpeaker {
        cicp_index: u8,
        member_index: usize,
    },
    #[error("metadata references unknown object '{id}'")]
    UnknownObject { id: String },
    #[error("object '{id}' metadata offset {offset} exceeds frame length {frame_count}")]
    MetadataOffsetOutOfRange {
        id: String,
        offset: u32,
        frame_count: usize,
    },
    #[error("object '{id}' requires a ramp that the stateful point renderer does not admit")]
    RampUnsupported { id: String },
    #[error("object '{id}' carries spread or rich renderer metadata not admitted by the stateful point renderer")]
    RichObjectMetadata { id: String },
    #[error("object '{id}' does not use admitted listener-relative spherical coordinates")]
    UnsupportedCoordinateSpace { id: String },
    #[error("object '{id}' has no carried state and no update before its first rendered sample")]
    MissingCarriedObjectState { id: String },
    #[error("decoded PCM lane {lane} sample {sample} is unavailable")]
    PcmSampleOutOfRange { lane: usize, sample: usize },
    #[error("metadata for object '{id}' at offset {offset} was not consumed deterministically")]
    UnconsumedMetadata { id: String, offset: u32 },
    #[error("MPEG-H stateful object timeline produced invalid output geometry")]
    InvalidRenderedGeometry,
    #[error(transparent)]
    Roles(#[from] MpeghRoleConformanceError),
    #[error(transparent)]
    Renderer(#[from] RendererError),
}
