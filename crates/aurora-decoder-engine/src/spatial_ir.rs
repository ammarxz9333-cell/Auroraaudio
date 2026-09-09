use std::collections::HashSet;

use aurora_core::ChannelRole;
use aurora_decoder_api::DecodedFrame;
use thiserror::Error;

/// Semantic shape of the decoded spatial signal set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialDomain {
    /// Object metadata was already consumed by a backend renderer. The PCM is
    /// speaker-rendered; metadata updates may remain for diagnostics only.
    SpeakerRendered,
    /// Every PCM lane is a discrete speaker/bed signal.
    DiscreteBed,
    /// The PCM contains both discrete bed lanes and independently renderable
    /// object-signal lanes.
    BedAndObjects,
    /// Every decoded spatial signal is an independently renderable object.
    ObjectSignals,
}

/// Coordinate system carried by one metadata update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinateSpace {
    /// Aurora room coordinates expressed in meters.
    AuroraMeters,
    /// Codec coordinates normalized to a room-relative unit space.
    RoomNormalized,
    /// Spherical coordinates expressed in degrees plus non-negative distance.
    SphericalDegrees,
}

/// Position payload without an implicit unit conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpatialPosition {
    Cartesian { x: f32, y: f32, z: f32 },
    Spherical {
        azimuth_degrees: f32,
        elevation_degrees: f32,
        distance: f32,
    },
}

impl SpatialPosition {
    fn is_finite(self) -> bool {
        match self {
            Self::Cartesian { x, y, z } => x.is_finite() && y.is_finite() && z.is_finite(),
            Self::Spherical {
                azimuth_degrees,
                elevation_degrees,
                distance,
            } => {
                azimuth_degrees.is_finite()
                    && elevation_degrees.is_finite()
                    && distance.is_finite()
                    && distance >= 0.0
            }
        }
    }
}

/// One discrete speaker/bed lane in the decoded PCM block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BedSignalBinding {
    pub pcm_channel_index: usize,
    pub role: ChannelRole,
}

/// Stable identity of one independently renderable PCM object signal.
///
/// Signal identity is deliberately separated from metadata updates because
/// formats such as AC-4 OAMD can update the same object multiple times inside a
/// single audio access unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectSignalBinding {
    pub id: String,
    pub pcm_channel_index: usize,
}

/// One time-stamped metadata update for a spatial object.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialObjectUpdate {
    pub object_id: String,
    pub active: bool,
    pub coordinate_space: CoordinateSpace,
    pub position: SpatialPosition,
    pub gain_db: f32,
    pub spread: f32,
    /// Offset relative to the beginning of this decoded access unit/block.
    pub metadata_sample_offset: u32,
    /// Interpolation/ramp duration in audio samples.
    pub ramp_duration_samples: u32,
    /// Optional normalized source priority, conventionally 0.0..=1.0.
    pub priority: Option<f32>,
}

/// Codec-neutral spatial metadata attached to decoded PCM.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialFrameMetadata {
    pub domain: SpatialDomain,
    pub bed_signals: Vec<BedSignalBinding>,
    pub object_signals: Vec<ObjectSignalBinding>,
    /// Ordered metadata events. Multiple entries for the same object are valid
    /// and preserve sub-frame object motion.
    pub object_updates: Vec<SpatialObjectUpdate>,
}

/// Aurora object-preserving decoder output.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialDecodedFrame {
    pub decoded: DecodedFrame,
    pub spatial: SpatialFrameMetadata,
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum SpatialIrError {
    #[error("decoded audio block is internally inconsistent")]
    InvalidAudioGeometry,
    #[error("legacy DecodedFrame object metadata is ambiguous with Spatial IR ({count} objects)")]
    LegacyObjectsPresent { count: usize },
    #[error("PCM lane {lane} is outside the decoded {channels}-channel block")]
    PcmLaneOutOfRange { lane: usize, channels: usize },
    #[error("PCM lane {lane} is bound more than once in the spatial scene")]
    DuplicatePcmLane { lane: usize },
    #[error("object signal id '{id}' is bound more than once")]
    DuplicateObjectSignalId { id: String },
    #[error("metadata update references unknown object signal '{id}'")]
    UnknownObjectSignal { id: String },
    #[error("speaker-rendered domain must not expose live object PCM signals")]
    SpeakerRenderedObjectSignals,
    #[error("spatial domain {domain:?} does not permit bed signal bindings")]
    BedSignalsForbidden { domain: SpatialDomain },
    #[error("discrete-bed domain does not permit object signals or updates")]
    ObjectsForbiddenInDiscreteBed,
    #[error("spatial object '{id}' coordinate space does not match its position representation")]
    CoordinateSpaceMismatch { id: String },
    #[error("spatial object '{id}' contains a non-finite or invalid position")]
    InvalidPosition { id: String },
    #[error("spatial object '{id}' spread must be finite and within 0.0..=1.0")]
    InvalidSpread { id: String },
    #[error("spatial object '{id}' gain must be finite or negative infinity")]
    InvalidGain { id: String },
    #[error("spatial object '{id}' priority must be finite and within 0.0..=1.0")]
    InvalidPriority { id: String },
    #[error("spatial object '{id}' metadata offset exceeds decoded frame length")]
    MetadataOffsetOutOfRange { id: String },
}

impl SpatialDecodedFrame {
    /// Validate signal ownership, metadata references, coordinate semantics and
    /// numeric safety. This is deliberately fail-closed and does no guessing.
    pub fn validate(&self) -> Result<(), SpatialIrError> {
        self.decoded
            .audio
            .validate()
            .map_err(|_| SpatialIrError::InvalidAudioGeometry)?;
        if !self.decoded.objects.is_empty() {
            return Err(SpatialIrError::LegacyObjectsPresent {
                count: self.decoded.objects.len(),
            });
        }

        match self.spatial.domain {
            SpatialDomain::SpeakerRendered => {
                if !self.spatial.bed_signals.is_empty() {
                    return Err(SpatialIrError::BedSignalsForbidden {
                        domain: SpatialDomain::SpeakerRendered,
                    });
                }
                if !self.spatial.object_signals.is_empty() {
                    return Err(SpatialIrError::SpeakerRenderedObjectSignals);
                }
            }
            SpatialDomain::ObjectSignals if !self.spatial.bed_signals.is_empty() => {
                return Err(SpatialIrError::BedSignalsForbidden {
                    domain: SpatialDomain::ObjectSignals,
                });
            }
            SpatialDomain::DiscreteBed
                if !self.spatial.object_signals.is_empty()
                    || !self.spatial.object_updates.is_empty() =>
            {
                return Err(SpatialIrError::ObjectsForbiddenInDiscreteBed);
            }
            _ => {}
        }

        let channels = self.decoded.audio.channels.len();
        let mut used_lanes = HashSet::with_capacity(
            self.spatial
                .bed_signals
                .len()
                .saturating_add(self.spatial.object_signals.len()),
        );
        for bed in &self.spatial.bed_signals {
            validate_lane(bed.pcm_channel_index, channels, &mut used_lanes)?;
        }

        let mut signal_ids = HashSet::with_capacity(self.spatial.object_signals.len());
        for signal in &self.spatial.object_signals {
            validate_lane(signal.pcm_channel_index, channels, &mut used_lanes)?;
            if !signal_ids.insert(signal.id.as_str()) {
                return Err(SpatialIrError::DuplicateObjectSignalId {
                    id: signal.id.clone(),
                });
            }
        }

        for update in &self.spatial.object_updates {
            validate_update_numeric(update)?;
            validate_coordinate_pair(update)?;
            if update.metadata_sample_offset > self.decoded.audio.frame_count as u32 {
                return Err(SpatialIrError::MetadataOffsetOutOfRange {
                    id: update.object_id.clone(),
                });
            }
            if matches!(
                self.spatial.domain,
                SpatialDomain::BedAndObjects | SpatialDomain::ObjectSignals
            ) && !signal_ids.contains(update.object_id.as_str())
            {
                return Err(SpatialIrError::UnknownObjectSignal {
                    id: update.object_id.clone(),
                });
            }
        }
        Ok(())
    }
}

fn validate_lane(
    lane: usize,
    channels: usize,
    used_lanes: &mut HashSet<usize>,
) -> Result<(), SpatialIrError> {
    if lane >= channels {
        return Err(SpatialIrError::PcmLaneOutOfRange { lane, channels });
    }
    if !used_lanes.insert(lane) {
        return Err(SpatialIrError::DuplicatePcmLane { lane });
    }
    Ok(())
}

fn validate_coordinate_pair(update: &SpatialObjectUpdate) -> Result<(), SpatialIrError> {
    let matches = matches!(
        (update.coordinate_space, update.position),
        (
            CoordinateSpace::AuroraMeters | CoordinateSpace::RoomNormalized,
            SpatialPosition::Cartesian { .. }
        ) | (
            CoordinateSpace::SphericalDegrees,
            SpatialPosition::Spherical { .. }
        )
    );
    if matches {
        Ok(())
    } else {
        Err(SpatialIrError::CoordinateSpaceMismatch {
            id: update.object_id.clone(),
        })
    }
}

fn validate_update_numeric(update: &SpatialObjectUpdate) -> Result<(), SpatialIrError> {
    if !update.position.is_finite() {
        return Err(SpatialIrError::InvalidPosition {
            id: update.object_id.clone(),
        });
    }
    if !update.spread.is_finite() || !(0.0..=1.0).contains(&update.spread) {
        return Err(SpatialIrError::InvalidSpread {
            id: update.object_id.clone(),
        });
    }
    if !(update.gain_db.is_finite() || update.gain_db == f32::NEG_INFINITY) {
        return Err(SpatialIrError::InvalidGain {
            id: update.object_id.clone(),
        });
    }
    if let Some(priority) = update.priority {
        if !priority.is_finite() || !(0.0..=1.0).contains(&priority) {
            return Err(SpatialIrError::InvalidPriority {
                id: update.object_id.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::AudioBlock;

    fn decoded(channels: usize, frames: usize) -> DecodedFrame {
        DecodedFrame {
            audio: AudioBlock {
                channels: (0..channels).map(|_| vec![0.0; frames]).collect(),
                frame_count: frames,
                presentation_time_seconds: 0.0,
                discontinuity: false,
            },
            objects: Vec::new(),
        }
    }

    fn update(id: &str, offset: u32) -> SpatialObjectUpdate {
        SpatialObjectUpdate {
            object_id: id.to_owned(),
            active: true,
            coordinate_space: CoordinateSpace::RoomNormalized,
            position: SpatialPosition::Cartesian {
                x: 0.5,
                y: 0.5,
                z: 0.0,
            },
            gain_db: 0.0,
            spread: 0.0,
            metadata_sample_offset: offset,
            ramp_duration_samples: 40,
            priority: Some(1.0),
        }
    }

    #[test]
    fn multiple_updates_can_share_one_object_signal_lane() {
        let frame = SpatialDecodedFrame {
            decoded: decoded(1, 80),
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                object_signals: vec![ObjectSignalBinding {
                    id: "object-0".into(),
                    pcm_channel_index: 0,
                }],
                object_updates: vec![update("object-0", 0), update("object-0", 40)],
            },
        };
        assert_eq!(frame.validate(), Ok(()));
    }

    #[test]
    fn duplicate_signal_lane_is_rejected() {
        let frame = SpatialDecodedFrame {
            decoded: decoded(2, 40),
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::BedAndObjects,
                bed_signals: vec![BedSignalBinding {
                    pcm_channel_index: 0,
                    role: ChannelRole::FrontLeft,
                }],
                object_signals: vec![ObjectSignalBinding {
                    id: "dialog".into(),
                    pcm_channel_index: 0,
                }],
                object_updates: vec![update("dialog", 0)],
            },
        };
        assert_eq!(frame.validate(), Err(SpatialIrError::DuplicatePcmLane { lane: 0 }));
    }

    #[test]
    fn unknown_object_update_fails_closed() {
        let frame = SpatialDecodedFrame {
            decoded: decoded(1, 40),
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                object_signals: vec![ObjectSignalBinding {
                    id: "known".into(),
                    pcm_channel_index: 0,
                }],
                object_updates: vec![update("unknown", 0)],
            },
        };
        assert_eq!(
            frame.validate(),
            Err(SpatialIrError::UnknownObjectSignal {
                id: "unknown".into()
            })
        );
    }

    #[test]
    fn metadata_offset_cannot_exceed_audio_access_unit() {
        let frame = SpatialDecodedFrame {
            decoded: decoded(1, 40),
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                object_signals: vec![ObjectSignalBinding {
                    id: "object-0".into(),
                    pcm_channel_index: 0,
                }],
                object_updates: vec![update("object-0", 41)],
            },
        };
        assert_eq!(
            frame.validate(),
            Err(SpatialIrError::MetadataOffsetOutOfRange {
                id: "object-0".into()
            })
        );
    }
}
