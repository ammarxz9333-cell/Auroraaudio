use std::collections::HashMap;

use aurora_core::{AudioBlock, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererError, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::Vbap3dRenderer;
use aurora_spatial_ir_v2::{CoordinateSpace, SpatialObjectUpdate, SpatialPosition, SpatialRenderingProperties};
use aurora_spatial_transport_v2::{cicp_layout_member_geometry, cicp_layout_members, TransportSceneDomain};
use thiserror::Error;

use crate::{reference_roles, MpeghPairedEvidence, MpeghRoleConformanceError};

const EPSILON: f32 = 1.0e-8;

/// Correctness-first sample-accurate point-object renderer for MPEG-H OAM.
///
/// Multiple metadata updates per access unit are admitted and applied at their
/// exact sample offsets. Ramped motion, spread/extent, exclusion zones, snap,
/// screen anchoring and other rich properties still fail closed. Every object
/// must establish state at sample zero because this candidate is intentionally
/// stateless across access units; the libmpegh reference remains the fallback
/// for scenes that require carried state.
pub fn render_exact_mpegh_object_plane(
    pair: &MpeghPairedEvidence,
) -> Result<AudioBlock, MpeghObjectTimelineError> {
    match pair.scene.domain {
        TransportSceneDomain::ObjectsAndHoa | TransportSceneDomain::BedObjectsAndHoa => {}
        domain => {
            return Err(MpeghObjectTimelineError::UnsupportedDomain {
                domain: format!("{domain:?}"),
            })
        }
    }
    pair.scene
        .validate()
        .map_err(|error| MpeghObjectTimelineError::InvalidTransport(error.to_string()))?;

    let frame = &pair.scene.frame;
    let frame_count = frame.decoded.audio.frame_count;
    let object_count = frame.spatial.object_signals.len();
    if object_count == 0 {
        return Err(MpeghObjectTimelineError::NoObjects);
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

    let listener = Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 0.0,
    };
    let mut renderer = Vbap3dRenderer::new().with_smoothing(1.0);
    renderer.configure(speakers, 48_000, frame_count.max(1), object_count)?;
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

    let mut states = vec![None; object_count];
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
            let slot = object_index[update.object_id.as_str()];
            states[slot] = Some(render_object_from_update(update)?);
            update_index += 1;
        }

        for (slot, signal) in frame.spatial.object_signals.iter().enumerate() {
            render_objects[slot] = states[slot].ok_or_else(|| {
                MpeghObjectTimelineError::MissingInitialObjectState {
                    id: signal.id.clone(),
                }
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
                .ok_or(MpeghObjectTimelineError::PcmSampleOutOfRange {
                    lane: signal.pcm_channel_index,
                    sample: sample_index,
                })?;
            let object_gains = &gains[slot * speaker_count..(slot + 1) * speaker_count];
            for (speaker_index, gain) in object_gains.iter().enumerate() {
                output.channels[speaker_index][sample_index] += source * gain.gain;
            }
        }
    }

    // Metadata at exactly frame_count applies to the next access unit. This
    // stateless candidate cannot carry it, so reject rather than silently drop.
    if update_index != updates.len() {
        return Err(MpeghObjectTimelineError::CrossFrameStateRequired);
    }

    output
        .validate()
        .map_err(|_| MpeghObjectTimelineError::InvalidRenderedGeometry)?;
    Ok(output)
}

fn validate_updates(
    updates: &[&SpatialObjectUpdate],
    object_index: &HashMap<&str, usize>,
    frame_count: usize,
) -> Result<(), MpeghObjectTimelineError> {
    let mut has_zero = vec![false; object_index.len()];
    for update in updates {
        let Some(slot) = object_index.get(update.object_id.as_str()).copied() else {
            return Err(MpeghObjectTimelineError::UnknownObject {
                id: update.object_id.clone(),
            });
        };
        if update.metadata_sample_offset as usize > frame_count {
            return Err(MpeghObjectTimelineError::MetadataOffsetOutOfRange {
                id: update.object_id.clone(),
                offset: update.metadata_sample_offset,
                frame_count,
            });
        }
        if update.metadata_sample_offset == 0 {
            has_zero[slot] = true;
        }
        if update.ramp_duration_samples != 0 {
            return Err(MpeghObjectTimelineError::RampUnsupported {
                id: update.object_id.clone(),
            });
        }
        if update.spread > EPSILON || update.rendering != SpatialRenderingProperties::default() {
            return Err(MpeghObjectTimelineError::RichObjectMetadata {
                id: update.object_id.clone(),
            });
        }
        if !matches!(
            (update.coordinate_space, update.position),
            (CoordinateSpace::SphericalDegrees, SpatialPosition::Spherical { .. })
        ) {
            return Err(MpeghObjectTimelineError::UnsupportedCoordinateSpace {
                id: update.object_id.clone(),
            });
        }
    }
    for (id, slot) in object_index {
        if !has_zero[*slot] {
            return Err(MpeghObjectTimelineError::MissingInitialObjectState {
                id: (*id).to_owned(),
            });
        }
    }
    Ok(())
}

fn render_object_from_update(
    update: &SpatialObjectUpdate,
) -> Result<RenderObject, MpeghObjectTimelineError> {
    let SpatialPosition::Spherical {
        azimuth_degrees,
        elevation_degrees,
        distance,
    } = update.position
    else {
        return Err(MpeghObjectTimelineError::UnsupportedCoordinateSpace {
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
) -> Result<Vec<crate::MpeghSpeaker>, MpeghObjectTimelineError> {
    if !pair.reference_layout.speakers.is_empty() {
        return Ok(pair.reference_layout.speakers.clone());
    }
    let cicp_index = u8::try_from(pair.reference_layout.cicp_index)
        .map_err(|_| MpeghObjectTimelineError::MissingReferenceSpeakerLayout)?;
    let members = cicp_layout_members(cicp_index)
        .ok_or(MpeghObjectTimelineError::MissingReferenceSpeakerLayout)?;
    let mut result = Vec::with_capacity(members.len());
    for member_index in 0..members.len() {
        let geometry = cicp_layout_member_geometry(cicp_index, member_index as u16).ok_or(
            MpeghObjectTimelineError::UnresolvedCicpSpeaker {
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
pub enum MpeghObjectTimelineError {
    #[error("MPEG-H exact point-object plane does not support scene domain {domain}")]
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
    #[error("object '{id}' requires a ramp that the exact step-timeline candidate does not admit")]
    RampUnsupported { id: String },
    #[error("object '{id}' carries spread or rich renderer metadata not admitted by the point-object timeline")]
    RichObjectMetadata { id: String },
    #[error("object '{id}' does not use admitted listener-relative spherical coordinates")]
    UnsupportedCoordinateSpace { id: String },
    #[error("object '{id}' has no state at sample zero; cross-frame state would be required")]
    MissingInitialObjectState { id: String },
    #[error("metadata at frame boundary requires state carry into the next access unit")]
    CrossFrameStateRequired,
    #[error("decoded PCM lane {lane} sample {sample} is unavailable")]
    PcmSampleOutOfRange { lane: usize, sample: usize },
    #[error("MPEG-H exact object timeline produced invalid output geometry")]
    InvalidRenderedGeometry,
    #[error(transparent)]
    Roles(#[from] MpeghRoleConformanceError),
    #[error(transparent)]
    Renderer(#[from] RendererError),
}
