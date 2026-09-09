use aurora_core::AudioBlock;
use aurora_decoder_api::DecodedFrame;
use aurora_spatial_ir_v2::{
    ContentKind, CoordinateSpace, ObjectDistance, ObjectExtent, ObjectSignalBinding,
    SpatialDecodedFrame, SpatialDomain, SpatialFrameMetadata, SpatialObjectUpdate,
    SpatialPosition, SpatialRenderingProperties, ZoneConstraint,
};
use aurora_spatial_transport_v2::{
    BedSignalTarget, ExplicitSpeakerGeometry, HoaGroupBinding, HoaSignalBinding,
    OpaqueBitPayload, OpaqueCodecMetadata, SpeakerElevation, SpatialTransportFrame,
    TransportBedSignalBinding, TransportSceneDomain,
};
use thiserror::Error;

use crate::{
    MpeghChannelMetadataPacket, MpeghExternalFrame, MpeghExternalLane, MpeghFlexibleSpeaker,
    MpeghHoaPacket, MpeghOamObject, MpeghOamObjectFrame, MpeghPcmTopologyError,
    MpeghSpeakerConfig,
};

impl MpeghExternalFrame {
    /// Convert one libmpegh external-render frame into Aurora's pre-render
    /// Transport V2 contract. This keeps bed/object/HOA signal ownership intact;
    /// it never asks libmpegh to speaker-render the immersive scene first.
    pub fn to_spatial_transport_v2(
        &self,
        presentation_time_seconds: f64,
        discontinuity: bool,
    ) -> Result<SpatialTransportFrame, MpeghSpatialTransportError> {
        build_spatial_transport_v2(self, presentation_time_seconds, discontinuity)
    }
}

pub fn build_spatial_transport_v2(
    source: &MpeghExternalFrame,
    presentation_time_seconds: f64,
    discontinuity: bool,
) -> Result<SpatialTransportFrame, MpeghSpatialTransportError> {
    if !presentation_time_seconds.is_finite() || presentation_time_seconds < 0.0 {
        return Err(MpeghSpatialTransportError::InvalidPresentationTime(
            presentation_time_seconds,
        ));
    }

    let pcm = source.decode_prerender_pcm()?;
    let channel_metadata = if pcm.topology.channel_lane_count == 0 {
        None
    } else {
        if source.channel_metadata.is_empty() {
            return Err(MpeghSpatialTransportError::MissingChannelMetadata);
        }
        Some(source.parse_channel_metadata().map_err(|error| {
            MpeghSpatialTransportError::InvalidChannelMetadata(error.to_string())
        })?)
    };
    if let Some(metadata) = &channel_metadata {
        if usize::from(metadata.frame_length_samples) != pcm.frame_count {
            return Err(MpeghSpatialTransportError::MetadataFrameLengthMismatch {
                plane: "channel",
                metadata: usize::from(metadata.frame_length_samples),
                pcm: pcm.frame_count,
            });
        }
    }

    let oam = if pcm.topology.object_lane_count == 0 {
        None
    } else {
        Some(source.parse_object_metadata().map_err(|error| {
            MpeghSpatialTransportError::InvalidObjectMetadataPacket(error.to_string())
        })?)
    };

    let hoa = if pcm.topology.hoa_lane_count == 0 {
        None
    } else {
        if source.hoa_metadata.is_empty() {
            return Err(MpeghSpatialTransportError::MissingHoaMetadata);
        }
        let parsed = source.parse_hoa_metadata().map_err(|error| {
            MpeghSpatialTransportError::InvalidHoaMetadata(error.to_string())
        })?;
        if usize::from(parsed.frame_length_samples) != pcm.frame_count {
            return Err(MpeghSpatialTransportError::MetadataFrameLengthMismatch {
                plane: "hoa",
                metadata: usize::from(parsed.frame_length_samples),
                pcm: pcm.frame_count,
            });
        }
        Some(parsed)
    };

    let bed_signals = match channel_metadata.as_ref() {
        Some(metadata) => beds_from_channel_metadata(metadata)?,
        None => Vec::new(),
    };
    if bed_signals.len() != pcm.topology.channel_lane_count {
        return Err(MpeghSpatialTransportError::BedLaneCountMismatch {
            metadata: bed_signals.len(),
            pcm: pcm.topology.channel_lane_count,
        });
    }

    let (object_signals, object_updates) = match oam.as_ref() {
        Some(packet) => objects_from_oam(packet, &pcm.topology.lanes, pcm.frame_count)?,
        None => (Vec::new(), Vec::new()),
    };
    let hoa_signals = pcm
        .topology
        .lanes
        .iter()
        .enumerate()
        .filter_map(|(pcm_channel_index, lane)| match lane {
            MpeghExternalLane::HoaTransport { transport_index } => Some(HoaSignalBinding {
                pcm_channel_index,
                transport_index: *transport_index,
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    let hoa_groups = match hoa.as_ref() {
        Some(packet) => hoa_groups_from_metadata(packet, &hoa_signals)?,
        None => Vec::new(),
    };

    let has_bed = !bed_signals.is_empty();
    let has_objects = !object_signals.is_empty();
    let has_hoa = !hoa_signals.is_empty();
    let domain = transport_domain(has_bed, has_objects, has_hoa)?;
    let compatibility_domain = if has_objects {
        if has_bed {
            SpatialDomain::BedAndObjects
        } else {
            SpatialDomain::ObjectSignals
        }
    } else {
        SpatialDomain::DiscreteBed
    };

    let spatial = SpatialDecodedFrame {
        decoded: DecodedFrame {
            audio: AudioBlock {
                channels: pcm.channels,
                frame_count: pcm.frame_count,
                presentation_time_seconds,
                discontinuity,
            },
            objects: Vec::new(),
        },
        spatial: SpatialFrameMetadata {
            domain: compatibility_domain,
            bed_signals: Vec::new(),
            object_signals,
            object_updates,
        },
    };

    let scene = SpatialTransportFrame {
        frame: spatial,
        domain,
        bed_signals,
        hoa_signals,
        hoa_groups,
        codec_metadata: opaque_codec_planes(source),
    };
    scene.validate().map_err(|error| {
        MpeghSpatialTransportError::TransportValidation(error.to_string())
    })?;
    Ok(scene)
}

fn beds_from_channel_metadata(
    metadata: &MpeghChannelMetadataPacket,
) -> Result<Vec<TransportBedSignalBinding>, MpeghSpatialTransportError> {
    let mut beds = Vec::with_capacity(metadata.total_signal_count());
    let mut lane = 0usize;
    for group in &metadata.groups {
        match &group.layout {
            MpeghSpeakerConfig::CicpLayout { cicp_layout_index } => {
                for member_index in 0..group.signal_count {
                    beds.push(TransportBedSignalBinding {
                        pcm_channel_index: lane,
                        target: BedSignalTarget::CicpLayoutMember {
                            layout_index: *cicp_layout_index,
                            member_index: u16::try_from(member_index)
                                .map_err(|_| MpeghSpatialTransportError::NumericOverflow)?,
                        },
                    });
                    lane += 1;
                }
            }
            MpeghSpeakerConfig::CicpSpeakerList { speaker_indices } => {
                if speaker_indices.len() != group.signal_count {
                    return Err(MpeghSpatialTransportError::SpeakerSignalCountMismatch {
                        speakers: speaker_indices.len(),
                        signals: group.signal_count,
                    });
                }
                for index in speaker_indices {
                    beds.push(TransportBedSignalBinding {
                        pcm_channel_index: lane,
                        target: BedSignalTarget::CicpSpeakerIndex(*index),
                    });
                    lane += 1;
                }
            }
            MpeghSpeakerConfig::Flexible {
                angular_precision,
                speakers,
            } => {
                if speakers.len() != group.signal_count {
                    return Err(MpeghSpatialTransportError::SpeakerSignalCountMismatch {
                        speakers: speakers.len(),
                        signals: group.signal_count,
                    });
                }
                for speaker in speakers {
                    let target = match speaker {
                        MpeghFlexibleSpeaker::Cicp { speaker_index } => {
                            BedSignalTarget::CicpSpeakerIndex(*speaker_index)
                        }
                        MpeghFlexibleSpeaker::Explicit(explicit) => {
                            let elevation = match explicit
                                .explicit_elevation_degrees(*angular_precision)
                            {
                                Some(value) => SpeakerElevation::Degrees(f32::from(value)),
                                None => SpeakerElevation::CodecClass {
                                    codec: "mpeg-h".into(),
                                    class: explicit.elevation_class,
                                },
                            };
                            BedSignalTarget::ExplicitGeometry(ExplicitSpeakerGeometry {
                                azimuth_degrees: f32::from(
                                    explicit.azimuth_degrees(*angular_precision),
                                ),
                                elevation,
                                is_lfe: explicit.is_lfe,
                            })
                        }
                    };
                    beds.push(TransportBedSignalBinding {
                        pcm_channel_index: lane,
                        target,
                    });
                    lane += 1;
                }
            }
        }
    }
    Ok(beds)
}

fn hoa_groups_from_metadata(
    packet: &MpeghHoaPacket,
    signals: &[HoaSignalBinding],
) -> Result<Vec<HoaGroupBinding>, MpeghSpatialTransportError> {
    if packet.groups.len() != 1 {
        return Err(MpeghSpatialTransportError::MultipleHoaGroupsNeedSignalMapping {
            groups: packet.groups.len(),
        });
    }
    let group = &packet.groups[0];
    let transport_indices = signals
        .iter()
        .map(|signal| signal.transport_index)
        .collect::<Vec<_>>();
    if transport_indices.is_empty() {
        return Err(MpeghSpatialTransportError::HoaMetadataWithoutTransportSignals);
    }
    let matrix = group.matrix.as_ref().map(|matrix| OpaqueBitPayload {
        bit_length: matrix.bit_length,
        bytes: matrix.bits.bytes.clone(),
    });
    Ok(vec![HoaGroupBinding {
        group_index: 0,
        transport_indices,
        order: group.order,
        fixed_position: group.fixed_position,
        priority: group.priority,
        uses_nfc: group.uses_nfc,
        nfc_reference_distance_raw: group.nfc_reference_distance_raw,
        matrix,
        screen_relative: group.screen_relative,
    }])
}

fn opaque_codec_planes(source: &MpeghExternalFrame) -> Vec<OpaqueCodecMetadata> {
    let mut planes = Vec::with_capacity(3);
    if !source.channel_metadata.is_empty() {
        planes.push(OpaqueCodecMetadata {
            codec: "mpeg-h".into(),
            kind: "external-channel-metadata".into(),
            bytes: source.channel_metadata.clone(),
        });
    }
    if !source.object_metadata.is_empty() {
        planes.push(OpaqueCodecMetadata {
            codec: "mpeg-h".into(),
            kind: "external-oam-metadata".into(),
            bytes: source.object_metadata.clone(),
        });
    }
    if !source.hoa_metadata.is_empty() {
        planes.push(OpaqueCodecMetadata {
            codec: "mpeg-h".into(),
            kind: "external-hoa-metadata".into(),
            bytes: source.hoa_metadata.clone(),
        });
    }
    planes
}

fn objects_from_oam(
    packet: &crate::MpeghOamPacket,
    lanes: &[MpeghExternalLane],
    decoded_frame_count: usize,
) -> Result<(Vec<ObjectSignalBinding>, Vec<SpatialObjectUpdate>), MpeghSpatialTransportError> {
    let mut signals = Vec::with_capacity(packet.objects.len());
    for (pcm_channel_index, lane) in lanes.iter().enumerate() {
        if let MpeghExternalLane::Object {
            object_index,
            element_id,
        } = lane
        {
            let object = packet.objects.get(*object_index).ok_or(
                MpeghSpatialTransportError::ObjectLaneReferencesMissingMetadata {
                    object_index: *object_index,
                },
            )?;
            if object.element_id != *element_id {
                return Err(MpeghSpatialTransportError::ObjectElementIdMismatch {
                    topology: *element_id,
                    metadata: object.element_id,
                });
            }
            signals.push(ObjectSignalBinding {
                id: object_id(object.element_id),
                pcm_channel_index,
            });
        }
    }
    if signals.len() != packet.objects.len() {
        return Err(MpeghSpatialTransportError::ObjectLaneCountMismatch {
            metadata: packet.objects.len(),
            pcm: signals.len(),
        });
    }

    let mut updates = Vec::new();
    for object in &packet.objects {
        append_object_updates(
            &mut updates,
            object,
            packet.frame_length_samples,
            decoded_frame_count,
        )?;
    }
    updates.sort_by_key(|update| update.metadata_sample_offset);
    Ok((signals, updates))
}

fn append_object_updates(
    updates: &mut Vec<SpatialObjectUpdate>,
    object: &MpeghOamObject,
    metadata_frame_length: u32,
    decoded_frame_count: usize,
) -> Result<(), MpeghSpatialTransportError> {
    for (frame_index, frame) in object.frames.iter().enumerate() {
        if !frame.has_metadata {
            continue;
        }
        let offset = u32::try_from(frame_index)
            .ok()
            .and_then(|index| index.checked_mul(metadata_frame_length))
            .ok_or(MpeghSpatialTransportError::NumericOverflow)?;
        if usize::try_from(offset)
            .ok()
            .filter(|offset| *offset <= decoded_frame_count)
            .is_none()
        {
            return Err(MpeghSpatialTransportError::ObjectMetadataOffsetOutOfRange {
                element_id: object.element_id,
                offset,
                frame_count: decoded_frame_count,
            });
        }
        updates.push(update_from_oam_frame(object, frame, offset)?);
    }
    Ok(())
}

fn update_from_oam_frame(
    object: &MpeghOamObject,
    frame: &MpeghOamObjectFrame,
    metadata_sample_offset: u32,
) -> Result<SpatialObjectUpdate, MpeghSpatialTransportError> {
    let azimuth = frame
        .azimuth_degrees
        .ok_or(MpeghSpatialTransportError::IncompleteObjectMetadata(object.element_id))?;
    let elevation = frame
        .elevation_degrees
        .ok_or(MpeghSpatialTransportError::IncompleteObjectMetadata(object.element_id))?;
    let radius = frame
        .radius
        .ok_or(MpeghSpatialTransportError::IncompleteObjectMetadata(object.element_id))?;
    let gain_db = frame
        .gain_db
        .ok_or(MpeghSpatialTransportError::IncompleteObjectMetadata(object.element_id))?;
    if !azimuth.is_finite()
        || !elevation.is_finite()
        || !radius.is_finite()
        || radius < 0.0
        || !(gain_db.is_finite() || gain_db == f32::NEG_INFINITY)
    {
        return Err(MpeghSpatialTransportError::InvalidObjectMetadataValue(
            object.element_id,
        ));
    }

    let width = normalized(frame.spread_width_degrees.unwrap_or(0.0), 180.0)?;
    let height = normalized(frame.spread_height_degrees.unwrap_or(0.0), 90.0)?;
    let depth = normalized(frame.spread_depth.unwrap_or(0.0), 15.5)?;
    let spread = width.max(height).max(depth);
    let priority = frame
        .dynamic_priority
        .map(|value| f32::from(value) / 7.0)
        .unwrap_or_else(|| f32::from(object.group_priority) / 7.0);

    Ok(SpatialObjectUpdate {
        object_id: object_id(object.element_id),
        active: true,
        coordinate_space: CoordinateSpace::SphericalDegrees,
        position: SpatialPosition::Spherical {
            azimuth_degrees: azimuth,
            elevation_degrees: elevation,
            distance: radius,
        },
        gain_db,
        spread,
        metadata_sample_offset,
        ramp_duration_samples: 0,
        priority: Some(priority),
        rendering: SpatialRenderingProperties {
            distance: ObjectDistance::Unspecified,
            extent: ObjectExtent {
                width,
                depth,
                height,
            },
            zone: if object.exclusion_sectors.is_empty() {
                ZoneConstraint::All
            } else {
                ZoneConstraint::CodecSpecific(0x80)
            },
            elevation_enabled: true,
            screen_reference: None,
            snap: object.fixed_position,
            binaural_intent: Default::default(),
            content_kind: ContentKind::Unspecified,
            trim_bypass: None,
        },
    })
}

fn normalized(value: f32, maximum: f32) -> Result<f32, MpeghSpatialTransportError> {
    if !value.is_finite() || value < 0.0 || maximum <= 0.0 {
        return Err(MpeghSpatialTransportError::InvalidSpreadValue(value));
    }
    Ok((value / maximum).clamp(0.0, 1.0))
}

fn object_id(element_id: u16) -> String {
    format!("mpegh-oam-{element_id}")
}

fn transport_domain(
    bed: bool,
    objects: bool,
    hoa: bool,
) -> Result<TransportSceneDomain, MpeghSpatialTransportError> {
    match (bed, objects, hoa) {
        (true, false, false) => Ok(TransportSceneDomain::DiscreteBed),
        (false, true, false) => Ok(TransportSceneDomain::ObjectSignals),
        (true, true, false) => Ok(TransportSceneDomain::BedAndObjects),
        (false, false, true) => Ok(TransportSceneDomain::HoaTransport),
        (true, false, true) => Ok(TransportSceneDomain::BedAndHoa),
        (false, true, true) => Ok(TransportSceneDomain::ObjectsAndHoa),
        (true, true, true) => Ok(TransportSceneDomain::BedObjectsAndHoa),
        (false, false, false) => Err(MpeghSpatialTransportError::EmptyScene),
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum MpeghSpatialTransportError {
    #[error(transparent)]
    Pcm(#[from] MpeghPcmTopologyError),
    #[error("presentation time {0} is invalid")]
    InvalidPresentationTime(f64),
    #[error("MPEG-H channel lanes exist but external channel metadata is absent")]
    MissingChannelMetadata,
    #[error("external channel metadata is invalid: {0}")]
    InvalidChannelMetadata(String),
    #[error("external object metadata packet is invalid: {0}")]
    InvalidObjectMetadataPacket(String),
    #[error("MPEG-H HOA lanes exist but external HOA metadata is absent")]
    MissingHoaMetadata,
    #[error("external HOA metadata is invalid: {0}")]
    InvalidHoaMetadata(String),
    #[error("{plane} metadata frame length {metadata} does not match decoded PCM frame length {pcm}")]
    MetadataFrameLengthMismatch {
        plane: &'static str,
        metadata: usize,
        pcm: usize,
    },
    #[error("channel metadata produced {metadata} bed bindings for {pcm} proved PCM channel lanes")]
    BedLaneCountMismatch { metadata: usize, pcm: usize },
    #[error("speaker layout contains {speakers} speakers for {signals} signals")]
    SpeakerSignalCountMismatch { speakers: usize, signals: usize },
    #[error("object PCM topology references missing metadata object {object_index}")]
    ObjectLaneReferencesMissingMetadata { object_index: usize },
    #[error("object element id mismatch: topology={topology}, metadata={metadata}")]
    ObjectElementIdMismatch { topology: u16, metadata: u16 },
    #[error("object metadata contains {metadata} objects but PCM topology contains {pcm} object lanes")]
    ObjectLaneCountMismatch { metadata: usize, pcm: usize },
    #[error("object element {0} contains an incomplete external metadata frame")]
    IncompleteObjectMetadata(u16),
    #[error("object element {0} contains invalid numeric metadata")]
    InvalidObjectMetadataValue(u16),
    #[error("object element {element_id} metadata offset {offset} exceeds decoded frame length {frame_count}")]
    ObjectMetadataOffsetOutOfRange {
        element_id: u16,
        offset: u32,
        frame_count: usize,
    },
    #[error("object spread value {0} is invalid")]
    InvalidSpreadValue(f32),
    #[error("MPEG-H external metadata contains {groups} HOA groups; per-group transport-lane mapping is not admitted yet")]
    MultipleHoaGroupsNeedSignalMapping { groups: usize },
    #[error("HOA metadata is present but no HOA transport signals were proved")]
    HoaMetadataWithoutTransportSignals,
    #[error("MPEG-H transport scene contains no signals")]
    EmptyScene,
    #[error("Spatial Transport V2 rejected MPEG-H scene: {0}")]
    TransportValidation(String),
    #[error("MPEG-H transport conversion arithmetic overflow")]
    NumericOverflow,
}
