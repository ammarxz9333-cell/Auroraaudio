//! Additive pre-render transport scene contract above Aurora Spatial IR V2.
//!
//! Spatial IR V2 remains the object/render-intent contract used by existing
//! AC-4 and TrueHD paths. This layer adds signal ownership that V2 could not
//! represent losslessly: CICP/flexible bed targets and raw HOA transport lanes.
//! It is deliberately fail-closed and requires every decoded PCM lane to have a
//! unique owner.

#![forbid(unsafe_code)]

use std::collections::HashSet;

use aurora_core::ChannelRole;
use aurora_spatial_ir_v2::{SpatialDecodedFrame, SpatialDomain};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportSceneDomain {
    DiscreteBed,
    ObjectSignals,
    BedAndObjects,
    HoaTransport,
    BedAndHoa,
    ObjectsAndHoa,
    BedObjectsAndHoa,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpatialTransportFrame {
    /// PCM plus object metadata/render intent. `spatial.bed_signals` must be
    /// empty: bed ownership lives in `bed_signals` below so custom geometry is
    /// never collapsed into a semantic role.
    pub frame: SpatialDecodedFrame,
    pub domain: TransportSceneDomain,
    pub bed_signals: Vec<TransportBedSignalBinding>,
    pub hoa_signals: Vec<HoaSignalBinding>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransportBedSignalBinding {
    pub pcm_channel_index: usize,
    pub target: BedSignalTarget,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BedSignalTarget {
    /// Existing Aurora semantic bed target.
    SemanticRole(ChannelRole),
    /// One member of a standardized CICP loudspeaker layout. The member index
    /// is preserved even when Aurora does not yet carry the corresponding CICP
    /// geometry table.
    CicpLayoutMember {
        layout_index: u8,
        member_index: u16,
    },
    /// Direct CICP loudspeaker index.
    CicpSpeakerIndex(u8),
    /// Explicit/flexible speaker description.
    ExplicitGeometry(ExplicitSpeakerGeometry),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExplicitSpeakerGeometry {
    pub azimuth_degrees: f32,
    pub elevation: SpeakerElevation,
    pub is_lfe: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpeakerElevation {
    Degrees(f32),
    /// Some immersive formats encode an elevation class rather than an explicit
    /// angle. Preserve the class losslessly until a codec/CICP geometry table
    /// maps it into degrees.
    CodecClass { codec: String, class: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoaSignalBinding {
    pub pcm_channel_index: usize,
    pub transport_index: usize,
}

impl SpatialTransportFrame {
    pub fn validate(&self) -> Result<(), SpatialTransportError> {
        self.frame
            .validate()
            .map_err(|error| SpatialTransportError::InvalidSpatialV2(error.to_string()))?;

        if !self.frame.spatial.bed_signals.is_empty() {
            return Err(SpatialTransportError::LegacyBedBindingsPresent);
        }

        let channels = self.frame.decoded.audio.channels.len();
        let mut lanes = HashSet::with_capacity(channels);
        let mut object_ids = HashSet::with_capacity(self.frame.spatial.object_signals.len());
        for object in &self.frame.spatial.object_signals {
            validate_lane(object.pcm_channel_index, channels, &mut lanes)?;
            object_ids.insert(object.id.as_str());
        }

        for bed in &self.bed_signals {
            validate_lane(bed.pcm_channel_index, channels, &mut lanes)?;
            validate_bed_target(&bed.target)?;
        }

        let mut hoa_indices = HashSet::with_capacity(self.hoa_signals.len());
        for hoa in &self.hoa_signals {
            validate_lane(hoa.pcm_channel_index, channels, &mut lanes)?;
            if !hoa_indices.insert(hoa.transport_index) {
                return Err(SpatialTransportError::DuplicateHoaTransportIndex {
                    index: hoa.transport_index,
                });
            }
        }

        if lanes.len() != channels {
            return Err(SpatialTransportError::UnownedPcmLanes {
                owned: lanes.len(),
                channels,
            });
        }

        let expected = derive_domain(
            !self.bed_signals.is_empty(),
            !object_ids.is_empty(),
            !self.hoa_signals.is_empty(),
        )?;
        if expected != self.domain {
            return Err(SpatialTransportError::DomainMismatch {
                declared: self.domain,
                expected,
            });
        }

        // The embedded V2 domain is a compatibility projection only. Validate
        // that it does not contradict the object signal set carried by V2.
        let projected = if object_ids.is_empty() {
            SpatialDomain::DiscreteBed
        } else if self.bed_signals.is_empty() {
            SpatialDomain::ObjectSignals
        } else {
            SpatialDomain::BedAndObjects
        };
        if self.frame.spatial.domain != projected {
            return Err(SpatialTransportError::InvalidCompatibilityProjection {
                declared: self.frame.spatial.domain,
                expected: projected,
            });
        }

        Ok(())
    }
}

fn validate_lane(
    lane: usize,
    channels: usize,
    used: &mut HashSet<usize>,
) -> Result<(), SpatialTransportError> {
    if lane >= channels {
        return Err(SpatialTransportError::PcmLaneOutOfRange { lane, channels });
    }
    if !used.insert(lane) {
        return Err(SpatialTransportError::DuplicatePcmLane { lane });
    }
    Ok(())
}

fn validate_bed_target(target: &BedSignalTarget) -> Result<(), SpatialTransportError> {
    match target {
        BedSignalTarget::SemanticRole(_) => Ok(()),
        BedSignalTarget::CicpLayoutMember { layout_index, .. } => {
            if *layout_index == 0 || *layout_index > 63 {
                return Err(SpatialTransportError::InvalidCicpLayoutIndex(*layout_index));
            }
            Ok(())
        }
        BedSignalTarget::CicpSpeakerIndex(index) => {
            if *index > 127 {
                return Err(SpatialTransportError::InvalidCicpSpeakerIndex(*index));
            }
            Ok(())
        }
        BedSignalTarget::ExplicitGeometry(geometry) => {
            if !geometry.azimuth_degrees.is_finite()
                || !(-180.0..=180.0).contains(&geometry.azimuth_degrees)
            {
                return Err(SpatialTransportError::InvalidExplicitAzimuth(
                    geometry.azimuth_degrees,
                ));
            }
            match &geometry.elevation {
                SpeakerElevation::Degrees(value)
                    if value.is_finite() && (-90.0..=90.0).contains(value) => {}
                SpeakerElevation::Degrees(value) => {
                    return Err(SpatialTransportError::InvalidExplicitElevation(*value))
                }
                SpeakerElevation::CodecClass { codec, .. } if codec.trim().is_empty() => {
                    return Err(SpatialTransportError::EmptyCodecClassName)
                }
                SpeakerElevation::CodecClass { .. } => {}
            }
            Ok(())
        }
    }
}

fn derive_domain(
    bed: bool,
    objects: bool,
    hoa: bool,
) -> Result<TransportSceneDomain, SpatialTransportError> {
    match (bed, objects, hoa) {
        (true, false, false) => Ok(TransportSceneDomain::DiscreteBed),
        (false, true, false) => Ok(TransportSceneDomain::ObjectSignals),
        (true, true, false) => Ok(TransportSceneDomain::BedAndObjects),
        (false, false, true) => Ok(TransportSceneDomain::HoaTransport),
        (true, false, true) => Ok(TransportSceneDomain::BedAndHoa),
        (false, true, true) => Ok(TransportSceneDomain::ObjectsAndHoa),
        (true, true, true) => Ok(TransportSceneDomain::BedObjectsAndHoa),
        (false, false, false) => Err(SpatialTransportError::EmptyScene),
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum SpatialTransportError {
    #[error("embedded Spatial IR V2 frame is invalid: {0}")]
    InvalidSpatialV2(String),
    #[error("transport frame must not retain legacy semantic bed bindings inside Spatial IR V2")]
    LegacyBedBindingsPresent,
    #[error("PCM lane {lane} is outside decoded {channels}-channel audio")]
    PcmLaneOutOfRange { lane: usize, channels: usize },
    #[error("PCM lane {lane} is owned more than once")]
    DuplicatePcmLane { lane: usize },
    #[error("HOA transport index {index} is duplicated")]
    DuplicateHoaTransportIndex { index: usize },
    #[error("only {owned} of {channels} decoded PCM lanes have transport ownership")]
    UnownedPcmLanes { owned: usize, channels: usize },
    #[error("transport scene has no bed, object, or HOA signals")]
    EmptyScene,
    #[error("transport domain mismatch: declared={declared:?}, expected={expected:?}")]
    DomainMismatch {
        declared: TransportSceneDomain,
        expected: TransportSceneDomain,
    },
    #[error("Spatial IR V2 compatibility projection mismatch: declared={declared:?}, expected={expected:?}")]
    InvalidCompatibilityProjection {
        declared: SpatialDomain,
        expected: SpatialDomain,
    },
    #[error("invalid CICP layout index {0}")]
    InvalidCicpLayoutIndex(u8),
    #[error("invalid CICP speaker index {0}")]
    InvalidCicpSpeakerIndex(u8),
    #[error("explicit speaker azimuth {0} is invalid")]
    InvalidExplicitAzimuth(f32),
    #[error("explicit speaker elevation {0} is invalid")]
    InvalidExplicitElevation(f32),
    #[error("codec-specific elevation class has an empty codec name")]
    EmptyCodecClassName,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::AudioBlock;
    use aurora_decoder_api::DecodedFrame;
    use aurora_spatial_ir_v2::{
        ObjectSignalBinding, SpatialFrameMetadata, SpatialObjectUpdate,
    };

    fn decoded(channels: usize) -> DecodedFrame {
        DecodedFrame {
            audio: AudioBlock {
                channels: vec![vec![0.0; 40]; channels],
                frame_count: 40,
                presentation_time_seconds: 0.0,
                discontinuity: false,
            },
            objects: Vec::new(),
        }
    }

    #[test]
    fn admits_bed_objects_and_hoa_without_collapsing_geometry() {
        let frame = SpatialTransportFrame {
            frame: SpatialDecodedFrame {
                decoded: decoded(4),
                spatial: SpatialFrameMetadata {
                    domain: SpatialDomain::BedAndObjects,
                    bed_signals: Vec::new(),
                    object_signals: vec![ObjectSignalBinding {
                        id: "o0".into(),
                        pcm_channel_index: 1,
                    }],
                    object_updates: Vec::<SpatialObjectUpdate>::new(),
                },
            },
            domain: TransportSceneDomain::BedObjectsAndHoa,
            bed_signals: vec![TransportBedSignalBinding {
                pcm_channel_index: 0,
                target: BedSignalTarget::ExplicitGeometry(ExplicitSpeakerGeometry {
                    azimuth_degrees: -30.0,
                    elevation: SpeakerElevation::Degrees(0.0),
                    is_lfe: false,
                }),
            }],
            hoa_signals: vec![
                HoaSignalBinding {
                    pcm_channel_index: 2,
                    transport_index: 0,
                },
                HoaSignalBinding {
                    pcm_channel_index: 3,
                    transport_index: 1,
                },
            ],
        };
        assert_eq!(frame.validate(), Ok(()));
    }

    #[test]
    fn rejects_unowned_pcm_lane() {
        let frame = SpatialTransportFrame {
            frame: SpatialDecodedFrame {
                decoded: decoded(2),
                spatial: SpatialFrameMetadata {
                    domain: SpatialDomain::DiscreteBed,
                    bed_signals: Vec::new(),
                    object_signals: Vec::new(),
                    object_updates: Vec::new(),
                },
            },
            domain: TransportSceneDomain::DiscreteBed,
            bed_signals: vec![TransportBedSignalBinding {
                pcm_channel_index: 0,
                target: BedSignalTarget::SemanticRole(ChannelRole::FrontLeft),
            }],
            hoa_signals: Vec::new(),
        };
        assert!(matches!(
            frame.validate(),
            Err(SpatialTransportError::UnownedPcmLanes { .. })
        ));
    }
}
