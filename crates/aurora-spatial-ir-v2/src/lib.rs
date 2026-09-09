//! Aurora Spatial IR V2.
//!
//! V2 is an additive, codec-neutral scene contract for immersive decoders. It
//! preserves renderer intent that V1 could not represent (distance, extent,
//! zones, screen reference, snap/elevation policy, dialogue and headphone
//! intent) while retaining explicit PCM signal ownership and sample-accurate
//! metadata timing.
//!
//! V1 remains supported during migration. A lossless V1 -> V2 conversion is
//! provided; V2 -> V1 is deliberately not automatic because that could discard
//! renderer metadata silently.

#![forbid(unsafe_code)]

use std::collections::HashSet;

use aurora_core::ChannelRole;
use aurora_decoder_api::DecodedFrame;
use thiserror::Error;

pub use aurora_spatial_ir::{CoordinateSpace, SpatialDomain, SpatialPosition};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedSignalBinding {
    pub pcm_channel_index: usize,
    pub role: ChannelRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectSignalBinding {
    pub id: String,
    pub pcm_channel_index: usize,
}

/// Physical or semantic source distance.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ObjectDistance {
    #[default]
    Unspecified,
    Meters(f32),
    Infinity,
}

/// Normalized object dimensions/extent. Zero means point-like in that axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectExtent {
    pub width: f32,
    pub depth: f32,
    pub height: f32,
}

impl Default for ObjectExtent {
    fn default() -> Self {
        Self {
            width: 0.0,
            depth: 0.0,
            height: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoneConstraint {
    #[default]
    All,
    NoBack,
    NoSides,
    CenterBack,
    ScreenOnly,
    SurroundOnly,
    /// Exact codec value retained when Aurora has no standardized semantic name.
    CodecSpecific(u8),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenReference {
    /// Normalized source-to-screen anchoring strength.
    pub factor: f32,
    /// Codec-defined normalized depth factor.
    pub depth_factor: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BinauralIntent {
    #[default]
    Unspecified,
    Off,
    Near,
    Far,
    /// Exact codec mode retained when no common semantic mapping is admitted.
    CodecSpecific(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentKind {
    #[default]
    Unspecified,
    Dialogue,
    Music,
    Effects,
    /// Exact codec classification retained for future renderer/policy mapping.
    CodecSpecific(u8),
}

/// Renderer-facing properties that are independent from instantaneous XYZ/gain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialRenderingProperties {
    pub distance: ObjectDistance,
    pub extent: ObjectExtent,
    pub zone: ZoneConstraint,
    pub elevation_enabled: bool,
    pub screen_reference: Option<ScreenReference>,
    pub snap: bool,
    pub binaural_intent: BinauralIntent,
    pub content_kind: ContentKind,
    /// Whether output-layout trim logic may be bypassed for this object.
    pub trim_bypass: Option<bool>,
}

impl Default for SpatialRenderingProperties {
    fn default() -> Self {
        Self {
            distance: ObjectDistance::Unspecified,
            extent: ObjectExtent::default(),
            zone: ZoneConstraint::All,
            elevation_enabled: true,
            screen_reference: None,
            snap: false,
            binaural_intent: BinauralIntent::Unspecified,
            content_kind: ContentKind::Unspecified,
            trim_bypass: None,
        }
    }
}

/// One sample-addressed object state update.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialObjectUpdate {
    pub object_id: String,
    pub active: bool,
    pub coordinate_space: CoordinateSpace,
    pub position: SpatialPosition,
    pub gain_db: f32,
    pub spread: f32,
    pub metadata_sample_offset: u32,
    pub ramp_duration_samples: u32,
    pub priority: Option<f32>,
    pub rendering: SpatialRenderingProperties,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpatialFrameMetadata {
    pub domain: SpatialDomain,
    pub bed_signals: Vec<BedSignalBinding>,
    pub object_signals: Vec<ObjectSignalBinding>,
    pub object_updates: Vec<SpatialObjectUpdate>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpatialDecodedFrame {
    pub decoded: DecodedFrame,
    pub spatial: SpatialFrameMetadata,
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum SpatialIrV2Error {
    #[error("decoded audio block is internally inconsistent")]
    InvalidAudioGeometry,
    #[error("legacy DecodedFrame objects are ambiguous with Spatial IR V2")]
    LegacyObjectsPresent,
    #[error("PCM lane {lane} is outside decoded {channels}-channel audio")]
    PcmLaneOutOfRange { lane: usize, channels: usize },
    #[error("PCM lane {lane} is bound more than once")]
    DuplicatePcmLane { lane: usize },
    #[error("object signal id '{id}' is duplicated")]
    DuplicateObjectId { id: String },
    #[error("metadata update references unknown object '{id}'")]
    UnknownObject { id: String },
    #[error("metadata offset for object '{id}' exceeds decoded frame")]
    MetadataOffsetOutOfRange { id: String },
    #[error("object '{id}' has invalid position/coordinate pairing")]
    InvalidPosition { id: String },
    #[error("object '{id}' has invalid gain")]
    InvalidGain { id: String },
    #[error("object '{id}' has invalid spread")]
    InvalidSpread { id: String },
    #[error("object '{id}' has invalid priority")]
    InvalidPriority { id: String },
    #[error("object '{id}' has invalid distance")]
    InvalidDistance { id: String },
    #[error("object '{id}' has invalid extent")]
    InvalidExtent { id: String },
    #[error("object '{id}' has invalid screen reference")]
    InvalidScreenReference { id: String },
    #[error("spatial domain and signal bindings are inconsistent")]
    InvalidDomain,
}

impl SpatialDecodedFrame {
    pub fn validate(&self) -> Result<(), SpatialIrV2Error> {
        self.decoded
            .audio
            .validate()
            .map_err(|_| SpatialIrV2Error::InvalidAudioGeometry)?;
        if !self.decoded.objects.is_empty() {
            return Err(SpatialIrV2Error::LegacyObjectsPresent);
        }
        validate_domain(&self.spatial)?;

        let channels = self.decoded.audio.channels.len();
        let mut lanes = HashSet::new();
        for bed in &self.spatial.bed_signals {
            validate_lane(bed.pcm_channel_index, channels, &mut lanes)?;
        }

        let mut ids = HashSet::new();
        for object in &self.spatial.object_signals {
            validate_lane(object.pcm_channel_index, channels, &mut lanes)?;
            if !ids.insert(object.id.as_str()) {
                return Err(SpatialIrV2Error::DuplicateObjectId {
                    id: object.id.clone(),
                });
            }
        }

        for update in &self.spatial.object_updates {
            if !ids.contains(update.object_id.as_str())
                && matches!(
                    self.spatial.domain,
                    SpatialDomain::BedAndObjects | SpatialDomain::ObjectSignals
                )
            {
                return Err(SpatialIrV2Error::UnknownObject {
                    id: update.object_id.clone(),
                });
            }
            if update.metadata_sample_offset > self.decoded.audio.frame_count as u32 {
                return Err(SpatialIrV2Error::MetadataOffsetOutOfRange {
                    id: update.object_id.clone(),
                });
            }
            validate_update(update)?;
        }
        Ok(())
    }
}

fn validate_domain(metadata: &SpatialFrameMetadata) -> Result<(), SpatialIrV2Error> {
    match metadata.domain {
        SpatialDomain::SpeakerRendered => {
            if !metadata.bed_signals.is_empty() || !metadata.object_signals.is_empty() {
                return Err(SpatialIrV2Error::InvalidDomain);
            }
        }
        SpatialDomain::DiscreteBed => {
            if !metadata.object_signals.is_empty() || !metadata.object_updates.is_empty() {
                return Err(SpatialIrV2Error::InvalidDomain);
            }
        }
        SpatialDomain::ObjectSignals => {
            if !metadata.bed_signals.is_empty() {
                return Err(SpatialIrV2Error::InvalidDomain);
            }
        }
        SpatialDomain::BedAndObjects => {}
    }
    Ok(())
}

fn validate_lane(
    lane: usize,
    channels: usize,
    used: &mut HashSet<usize>,
) -> Result<(), SpatialIrV2Error> {
    if lane >= channels {
        return Err(SpatialIrV2Error::PcmLaneOutOfRange { lane, channels });
    }
    if !used.insert(lane) {
        return Err(SpatialIrV2Error::DuplicatePcmLane { lane });
    }
    Ok(())
}

fn validate_update(update: &SpatialObjectUpdate) -> Result<(), SpatialIrV2Error> {
    let position_ok = match (update.coordinate_space, update.position) {
        (
            CoordinateSpace::AuroraMeters | CoordinateSpace::RoomNormalized,
            SpatialPosition::Cartesian { x, y, z },
        ) => x.is_finite() && y.is_finite() && z.is_finite(),
        (
            CoordinateSpace::SphericalDegrees,
            SpatialPosition::Spherical {
                azimuth_degrees,
                elevation_degrees,
                distance,
            },
        ) => {
            azimuth_degrees.is_finite()
                && elevation_degrees.is_finite()
                && distance.is_finite()
                && distance >= 0.0
        }
        _ => false,
    };
    if !position_ok {
        return Err(SpatialIrV2Error::InvalidPosition {
            id: update.object_id.clone(),
        });
    }
    if !(update.gain_db.is_finite() || update.gain_db == f32::NEG_INFINITY) {
        return Err(SpatialIrV2Error::InvalidGain {
            id: update.object_id.clone(),
        });
    }
    if !update.spread.is_finite() || !(0.0..=1.0).contains(&update.spread) {
        return Err(SpatialIrV2Error::InvalidSpread {
            id: update.object_id.clone(),
        });
    }
    if let Some(priority) = update.priority {
        if !priority.is_finite() || !(0.0..=1.0).contains(&priority) {
            return Err(SpatialIrV2Error::InvalidPriority {
                id: update.object_id.clone(),
            });
        }
    }

    match update.rendering.distance {
        ObjectDistance::Unspecified | ObjectDistance::Infinity => {}
        ObjectDistance::Meters(distance) if distance.is_finite() && distance >= 0.0 => {}
        _ => {
            return Err(SpatialIrV2Error::InvalidDistance {
                id: update.object_id.clone(),
            })
        }
    }

    let extent = update.rendering.extent;
    if ![extent.width, extent.depth, extent.height]
        .into_iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
    {
        return Err(SpatialIrV2Error::InvalidExtent {
            id: update.object_id.clone(),
        });
    }

    if let Some(screen) = update.rendering.screen_reference {
        if !screen.factor.is_finite()
            || !(0.0..=1.0).contains(&screen.factor)
            || !screen.depth_factor.is_finite()
            || screen.depth_factor < 0.0
        {
            return Err(SpatialIrV2Error::InvalidScreenReference {
                id: update.object_id.clone(),
            });
        }
    }
    Ok(())
}

impl From<aurora_spatial_ir::SpatialDecodedFrame> for SpatialDecodedFrame {
    fn from(frame: aurora_spatial_ir::SpatialDecodedFrame) -> Self {
        Self {
            decoded: frame.decoded,
            spatial: SpatialFrameMetadata {
                domain: frame.spatial.domain,
                bed_signals: frame
                    .spatial
                    .bed_signals
                    .into_iter()
                    .map(|bed| BedSignalBinding {
                        pcm_channel_index: bed.pcm_channel_index,
                        role: bed.role,
                    })
                    .collect(),
                object_signals: frame
                    .spatial
                    .object_signals
                    .into_iter()
                    .map(|object| ObjectSignalBinding {
                        id: object.id,
                        pcm_channel_index: object.pcm_channel_index,
                    })
                    .collect(),
                object_updates: frame
                    .spatial
                    .object_updates
                    .into_iter()
                    .map(|update| SpatialObjectUpdate {
                        object_id: update.object_id,
                        active: update.active,
                        coordinate_space: update.coordinate_space,
                        position: update.position,
                        gain_db: update.gain_db,
                        spread: update.spread,
                        metadata_sample_offset: update.metadata_sample_offset,
                        ramp_duration_samples: update.ramp_duration_samples,
                        priority: update.priority,
                        rendering: SpatialRenderingProperties::default(),
                    })
                    .collect(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::AudioBlock;

    fn frame_with(properties: SpatialRenderingProperties) -> SpatialDecodedFrame {
        SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels: vec![vec![0.0; 40]],
                    frame_count: 40,
                    presentation_time_seconds: 0.0,
                    discontinuity: false,
                },
                objects: Vec::new(),
            },
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                object_signals: vec![ObjectSignalBinding {
                    id: "o0".into(),
                    pcm_channel_index: 0,
                }],
                object_updates: vec![SpatialObjectUpdate {
                    object_id: "o0".into(),
                    active: true,
                    coordinate_space: CoordinateSpace::RoomNormalized,
                    position: SpatialPosition::Cartesian {
                        x: 0.5,
                        y: 0.5,
                        z: 0.0,
                    },
                    gain_db: 0.0,
                    spread: 0.0,
                    metadata_sample_offset: 0,
                    ramp_duration_samples: 32,
                    priority: Some(1.0),
                    rendering: properties,
                }],
            },
        }
    }

    #[test]
    fn v2_preserves_rich_render_intent() {
        let properties = SpatialRenderingProperties {
            distance: ObjectDistance::Meters(2.5),
            extent: ObjectExtent {
                width: 0.5,
                depth: 0.25,
                height: 0.1,
            },
            zone: ZoneConstraint::ScreenOnly,
            elevation_enabled: false,
            screen_reference: Some(ScreenReference {
                factor: 0.75,
                depth_factor: 0.5,
            }),
            snap: true,
            binaural_intent: BinauralIntent::Near,
            content_kind: ContentKind::Dialogue,
            trim_bypass: Some(true),
        };
        let frame = frame_with(properties);
        assert_eq!(frame.validate(), Ok(()));
        assert_eq!(frame.spatial.object_updates[0].rendering, properties);
    }

    #[test]
    fn v1_upgrade_never_invents_rich_metadata() {
        let v1 = aurora_spatial_ir::SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels: vec![vec![0.0; 40]],
                    frame_count: 40,
                    presentation_time_seconds: 0.0,
                    discontinuity: false,
                },
                objects: Vec::new(),
            },
            spatial: aurora_spatial_ir::SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                object_signals: vec![aurora_spatial_ir::ObjectSignalBinding {
                    id: "o0".into(),
                    pcm_channel_index: 0,
                }],
                object_updates: vec![aurora_spatial_ir::SpatialObjectUpdate {
                    object_id: "o0".into(),
                    active: true,
                    coordinate_space: CoordinateSpace::RoomNormalized,
                    position: SpatialPosition::Cartesian {
                        x: 0.5,
                        y: 0.5,
                        z: 0.0,
                    },
                    gain_db: 0.0,
                    spread: 0.0,
                    metadata_sample_offset: 0,
                    ramp_duration_samples: 0,
                    priority: Some(1.0),
                }],
            },
        };
        let v2 = SpatialDecodedFrame::from(v1);
        assert_eq!(
            v2.spatial.object_updates[0].rendering,
            SpatialRenderingProperties::default()
        );
    }
}
