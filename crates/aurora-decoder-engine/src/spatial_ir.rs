use aurora_core::ChannelRole;
use aurora_decoder_api::DecodedFrame;
use thiserror::Error;

/// Semantic shape of the decoded spatial signal set.
///
/// This is intentionally codec-neutral. A backend may decode Dolby JOC,
/// AC-4/OAMD, IAMF, MPEG-H, or another object format, but it must describe the
/// resulting signals through one of these domains before Aurora renders them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialDomain {
    /// The backend already consumed object metadata and rendered physical
    /// speaker channels. Object metadata may be retained for diagnostics, but
    /// no object is allowed to claim a live PCM signal lane.
    SpeakerRendered,
    /// Every PCM lane is a discrete speaker/bed signal. No object signals are
    /// present.
    DiscreteBed,
    /// The decoded PCM contains both discrete bed lanes and independently
    /// renderable object-signal lanes.
    BedAndObjects,
    /// Every decoded spatial signal is an independently renderable object.
    ObjectSignals,
}

/// Coordinate system carried by one spatial object.
///
/// Aurora keeps the source coordinate system explicit instead of silently
/// interpreting normalized codec coordinates as meters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinateSpace {
    /// Aurora room coordinates expressed in meters.
    AuroraMeters,
    /// Codec/metadata coordinates normalized to a room-relative unit space.
    /// The renderer must map these into the active room model before panning.
    RoomNormalized,
    /// Spherical coordinates expressed in degrees plus a non-negative distance
    /// scalar whose unit is defined by the source adapter.
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

/// Codec-neutral object metadata plus the optional PCM signal carrying that
/// object's audio.
///
/// `pcm_channel_index` is mandatory in object-preserving domains and absent in
/// `SpeakerRendered`, where the backend already consumed the object signal.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialObject {
    pub id: String,
    pub pcm_channel_index: Option<usize>,
    pub coordinate_space: CoordinateSpace,
    pub position: SpatialPosition,
    pub gain_db: f32,
    pub spread: f32,
    /// Offset of this metadata update relative to the beginning of the decoded
    /// block, in audio samples.
    pub metadata_sample_offset: u32,
    /// Metadata interpolation/ramp duration in audio samples.
    pub ramp_duration_samples: u32,
    /// Optional normalized source priority, conventionally 0.0..=1.0.
    pub priority: Option<f32>,
}

/// Spatial metadata attached to a decoded PCM block.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialFrameMetadata {
    pub domain: SpatialDomain,
    pub bed_signals: Vec<BedSignalBinding>,
    pub objects: Vec<SpatialObject>,
}

/// Aurora's object-preserving decoder output.
///
/// The legacy `DecodedFrame.objects` field is deliberately not used as the
/// signal-binding contract; this IR is the authoritative spatial description
/// because it carries coordinate-space and PCM-lane identity explicitly.
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
    #[error("spatial object '{id}' requires a bound PCM lane in this domain")]
    MissingObjectSignal { id: String },
    #[error("speaker-rendered object '{id}' must not expose a PCM object lane")]
    SpeakerRenderedObjectHasSignal { id: String },
    #[error("spatial domain {domain:?} does not permit bed signal bindings")]
    BedSignalsForbidden { domain: SpatialDomain },
    #[error("discrete-bed domain does not permit spatial objects")]
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
}

impl SpatialDecodedFrame {
    /// Validate signal ownership, coordinate semantics and basic numeric safety.
    ///
    /// Validation is allocation-light and intentionally fail-closed. It does not
    /// guess channel roles, convert coordinate systems, or fabricate missing
    /// object-signal bindings.
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
            SpatialDomain::SpeakerRendered if !self.spatial.bed_signals.is_empty() => {
                return Err(SpatialIrError::BedSignalsForbidden {
                    domain: SpatialDomain::SpeakerRendered,
                });
            }
            SpatialDomain::ObjectSignals if !self.spatial.bed_signals.is_empty() => {
                return Err(SpatialIrError::BedSignalsForbidden {
                    domain: SpatialDomain::ObjectSignals,
                });
            }
            SpatialDomain::DiscreteBed if !self.spatial.objects.is_empty() => {
                return Err(SpatialIrError::ObjectsForbiddenInDiscreteBed);
            }
            _ => {}
        }

        let channels = self.decoded.audio.channels.len();
        let mut used_lanes = Vec::with_capacity(
            self.spatial.bed_signals.len().saturating_add(self.spatial.objects.len()),
        );

        for bed in &self.spatial.bed_signals {
            validate_lane(bed.pcm_channel_index, channels, &used_lanes)?;
            used_lanes.push(bed.pcm_channel_index);
        }

        for object in &self.spatial.objects {
            validate_object_numeric(object)?;
            validate_coordinate_pair(object)?;

            match self.spatial.domain {
                SpatialDomain::SpeakerRendered => {
                    if object.pcm_channel_index.is_some() {
                        return Err(SpatialIrError::SpeakerRenderedObjectHasSignal {
                            id: object.id.clone(),
                        });
                    }
                }
                SpatialDomain::BedAndObjects | SpatialDomain::ObjectSignals => {
                    let lane = object.pcm_channel_index.ok_or_else(|| {
                        SpatialIrError::MissingObjectSignal {
                            id: object.id.clone(),
                        }
                    })?;
                    validate_lane(lane, channels, &used_lanes)?;
                    used_lanes.push(lane);
                }
                SpatialDomain::DiscreteBed => unreachable!("objects rejected above"),
            }
        }

        Ok(())
    }
}

fn validate_lane(
    lane: usize,
    channels: usize,
    used_lanes: &[usize],
) -> Result<(), SpatialIrError> {
    if lane >= channels {
        return Err(SpatialIrError::PcmLaneOutOfRange { lane, channels });
    }
    if used_lanes.contains(&lane) {
        return Err(SpatialIrError::DuplicatePcmLane { lane });
    }
    Ok(())
}

fn validate_coordinate_pair(object: &SpatialObject) -> Result<(), SpatialIrError> {
    let matches = matches!(
        (object.coordinate_space, object.position),
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
            id: object.id.clone(),
        })
    }
}

fn validate_object_numeric(object: &SpatialObject) -> Result<(), SpatialIrError> {
    if !object.position.is_finite() {
        return Err(SpatialIrError::InvalidPosition {
            id: object.id.clone(),
        });
    }
    if !object.spread.is_finite() || !(0.0..=1.0).contains(&object.spread) {
        return Err(SpatialIrError::InvalidSpread {
            id: object.id.clone(),
        });
    }
    if !(object.gain_db.is_finite() || object.gain_db == f32::NEG_INFINITY) {
        return Err(SpatialIrError::InvalidGain {
            id: object.id.clone(),
        });
    }
    if let Some(priority) = object.priority {
        if !priority.is_finite() || !(0.0..=1.0).contains(&priority) {
            return Err(SpatialIrError::InvalidPriority {
                id: object.id.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::{AudioBlock, ChannelRole};

    fn decoded(channels: usize) -> DecodedFrame {
        DecodedFrame {
            audio: AudioBlock {
                channels: (0..channels).map(|_| vec![0.0; 40]).collect(),
                frame_count: 40,
                presentation_time_seconds: 0.0,
                discontinuity: false,
            },
            objects: Vec::new(),
        }
    }

    fn object(id: &str, lane: Option<usize>) -> SpatialObject {
        SpatialObject {
            id: id.to_owned(),
            pcm_channel_index: lane,
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
        }
    }

    #[test]
    fn valid_seven_one_four_bed_passes() {
        let roles = [
            ChannelRole::FrontLeft,
            ChannelRole::FrontRight,
            ChannelRole::FrontCenter,
            ChannelRole::LowFrequencyEffects,
            ChannelRole::SurroundLeft,
            ChannelRole::SurroundRight,
            ChannelRole::SurroundBackLeft,
            ChannelRole::SurroundBackRight,
            ChannelRole::TopFrontLeft,
            ChannelRole::TopFrontRight,
            ChannelRole::TopRearLeft,
            ChannelRole::TopRearRight,
        ];
        let frame = SpatialDecodedFrame {
            decoded: decoded(12),
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::DiscreteBed,
                bed_signals: roles
                    .into_iter()
                    .enumerate()
                    .map(|(pcm_channel_index, role)| BedSignalBinding {
                        pcm_channel_index,
                        role,
                    })
                    .collect(),
                objects: Vec::new(),
            },
        };
        assert_eq!(frame.validate(), Ok(()));
    }

    #[test]
    fn duplicate_bed_and_object_lane_is_rejected() {
        let frame = SpatialDecodedFrame {
            decoded: decoded(2),
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::BedAndObjects,
                bed_signals: vec![BedSignalBinding {
                    pcm_channel_index: 0,
                    role: ChannelRole::FrontLeft,
                }],
                objects: vec![object("dialog", Some(0))],
            },
        };
        assert_eq!(
            frame.validate(),
            Err(SpatialIrError::DuplicatePcmLane { lane: 0 })
        );
    }

    #[test]
    fn room_normalized_object_signal_is_preserved_without_unit_guessing() {
        let frame = SpatialDecodedFrame {
            decoded: decoded(1),
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                objects: vec![object("object-0", Some(0))],
            },
        };
        assert_eq!(frame.validate(), Ok(()));
    }

    #[test]
    fn object_domain_without_signal_lane_fails_closed() {
        let frame = SpatialDecodedFrame {
            decoded: decoded(1),
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                objects: vec![object("object-0", None)],
            },
        };
        assert_eq!(
            frame.validate(),
            Err(SpatialIrError::MissingObjectSignal {
                id: "object-0".to_owned(),
            })
        );
    }

    #[test]
    fn speaker_rendered_metadata_must_not_claim_object_pcm_lane() {
        let frame = SpatialDecodedFrame {
            decoded: decoded(12),
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::SpeakerRendered,
                bed_signals: Vec::new(),
                objects: vec![object("already-rendered", Some(4))],
            },
        };
        assert_eq!(
            frame.validate(),
            Err(SpatialIrError::SpeakerRenderedObjectHasSignal {
                id: "already-rendered".to_owned(),
            })
        );
    }

    #[test]
    fn spherical_coordinate_space_rejects_cartesian_payload() {
        let mut obj = object("bad-space", Some(0));
        obj.coordinate_space = CoordinateSpace::SphericalDegrees;
        let frame = SpatialDecodedFrame {
            decoded: decoded(1),
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                objects: vec![obj],
            },
        };
        assert_eq!(
            frame.validate(),
            Err(SpatialIrError::CoordinateSpaceMismatch {
                id: "bad-space".to_owned(),
            })
        );
    }
}
