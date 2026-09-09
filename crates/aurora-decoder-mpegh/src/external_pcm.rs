use thiserror::Error;

use crate::{MpeghExternalFrame, MpeghOamPacket};

const FRAME_SAMPLES: usize = 1024;
const BYTES_PER_SAMPLE: usize = 3;
const MAX_EXTERNAL_LANES: usize = 32;
const PCM_24_SCALE: f32 = 8_388_608.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghExternalTopology {
    pub frame_samples: usize,
    pub lane_count: usize,
    pub channel_lane_count: usize,
    pub object_lane_offset: Option<usize>,
    pub object_lane_count: usize,
    pub hoa_lane_offset: Option<usize>,
    pub hoa_lane_count: usize,
    pub lanes: Vec<MpeghExternalLane>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpeghExternalLane {
    Channel { channel_index: usize },
    Object { object_index: usize, element_id: u16 },
    HoaTransport { transport_index: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghPrerenderPcm {
    pub sample_rate: u32,
    pub frame_count: usize,
    pub channels: Vec<Vec<f32>>,
    pub topology: MpeghExternalTopology,
}

impl MpeghExternalFrame {
    /// The upstream `oam_sample_offset` field is named like a time offset but
    /// libmpegh assigns it from `ch_offset`; it is therefore a PCM lane offset.
    pub fn object_pcm_lane_offset(&self) -> Option<usize> {
        (!self.object_metadata.is_empty())
            .then(|| usize::try_from(self.oam_sample_offset).ok())
            .flatten()
    }

    /// The upstream `hoa_sample_offset` field is likewise a PCM lane offset.
    pub fn hoa_pcm_lane_offset(&self) -> Option<usize> {
        (!self.hoa_metadata.is_empty())
            .then(|| usize::try_from(self.hoa_sample_offset).ok())
            .flatten()
    }

    /// Decode libmpegh's 24-bit little-endian, sample-major external-render PCM
    /// and prove the channel/object/HOA lane ranges using the authoritative
    /// offsets returned by libmpegh itself.
    pub fn decode_prerender_pcm(&self) -> Result<MpeghPrerenderPcm, MpeghPcmTopologyError> {
        let oam = if self.object_metadata.is_empty() {
            None
        } else {
            Some(
                self.parse_object_metadata()
                    .map_err(|error| MpeghPcmTopologyError::InvalidOam(error.to_string()))?,
            )
        };
        decode_prerender_pcm(self, oam.as_ref())
    }
}

pub fn decode_prerender_pcm(
    frame: &MpeghExternalFrame,
    oam: Option<&MpeghOamPacket>,
) -> Result<MpeghPrerenderPcm, MpeghPcmTopologyError> {
    if frame.pcm_bit_depth != 24 {
        return Err(MpeghPcmTopologyError::UnsupportedBitDepth(frame.pcm_bit_depth));
    }
    let sample_rate = u32::try_from(frame.sample_rate)
        .ok()
        .filter(|rate| *rate > 0)
        .ok_or(MpeghPcmTopologyError::InvalidSampleRate(frame.sample_rate))?;

    let bytes_per_lane = FRAME_SAMPLES
        .checked_mul(BYTES_PER_SAMPLE)
        .ok_or(MpeghPcmTopologyError::NumericOverflow)?;
    if frame.prerender_pcm.is_empty() || frame.prerender_pcm.len() % bytes_per_lane != 0 {
        return Err(MpeghPcmTopologyError::InvalidPcmLength {
            bytes: frame.prerender_pcm.len(),
        });
    }
    let lane_count = frame.prerender_pcm.len() / bytes_per_lane;
    if lane_count == 0 || lane_count > MAX_EXTERNAL_LANES {
        return Err(MpeghPcmTopologyError::InvalidLaneCount {
            actual: lane_count,
            maximum: MAX_EXTERNAL_LANES,
        });
    }

    let topology = prove_topology(frame, oam, lane_count)?;
    let mut channels = (0..lane_count)
        .map(|_| Vec::with_capacity(FRAME_SAMPLES))
        .collect::<Vec<_>>();
    for sample in frame.prerender_pcm.chunks_exact(lane_count * BYTES_PER_SAMPLE) {
        for (lane, destination) in channels.iter_mut().enumerate() {
            let offset = lane * BYTES_PER_SAMPLE;
            let raw = i32::from(sample[offset])
                | (i32::from(sample[offset + 1]) << 8)
                | (i32::from(sample[offset + 2]) << 16);
            let signed = if raw & 0x0080_0000 != 0 {
                raw | !0x00ff_ffff
            } else {
                raw
            };
            destination.push(signed as f32 / PCM_24_SCALE);
        }
    }

    Ok(MpeghPrerenderPcm {
        sample_rate,
        frame_count: FRAME_SAMPLES,
        channels,
        topology,
    })
}

fn prove_topology(
    frame: &MpeghExternalFrame,
    oam: Option<&MpeghOamPacket>,
    lane_count: usize,
) -> Result<MpeghExternalTopology, MpeghPcmTopologyError> {
    let object_count = oam.map_or(0, |packet| packet.objects.len());
    let object_offset = if object_count > 0 {
        Some(
            usize::try_from(frame.oam_sample_offset)
                .map_err(|_| MpeghPcmTopologyError::InvalidObjectLaneOffset(frame.oam_sample_offset))?,
        )
    } else {
        None
    };
    let hoa_present = !frame.hoa_metadata.is_empty();
    let hoa_offset = if hoa_present {
        Some(
            usize::try_from(frame.hoa_sample_offset)
                .map_err(|_| MpeghPcmTopologyError::InvalidHoaLaneOffset(frame.hoa_sample_offset))?,
        )
    } else {
        None
    };

    let (channel_count, hoa_count) = match (object_offset, hoa_offset) {
        (None, None) => (lane_count, 0),
        (Some(objects), None) => {
            let end = objects
                .checked_add(object_count)
                .ok_or(MpeghPcmTopologyError::NumericOverflow)?;
            if end != lane_count {
                return Err(MpeghPcmTopologyError::NonCoveringLaneRanges);
            }
            (objects, 0)
        }
        (None, Some(hoa)) => {
            if hoa > lane_count {
                return Err(MpeghPcmTopologyError::InvalidHoaLaneOffset(frame.hoa_sample_offset));
            }
            (hoa, lane_count - hoa)
        }
        (Some(objects), Some(hoa)) if objects < hoa => {
            let object_end = objects
                .checked_add(object_count)
                .ok_or(MpeghPcmTopologyError::NumericOverflow)?;
            if object_end != hoa || hoa > lane_count {
                return Err(MpeghPcmTopologyError::NonCoveringLaneRanges);
            }
            (objects, lane_count - hoa)
        }
        (Some(objects), Some(hoa)) if hoa < objects => {
            let object_end = objects
                .checked_add(object_count)
                .ok_or(MpeghPcmTopologyError::NumericOverflow)?;
            if object_end != lane_count || hoa > objects {
                return Err(MpeghPcmTopologyError::NonCoveringLaneRanges);
            }
            (hoa, objects - hoa)
        }
        (Some(_), Some(_)) => return Err(MpeghPcmTopologyError::OverlappingObjectAndHoaRanges),
    };

    if channel_count > lane_count {
        return Err(MpeghPcmTopologyError::NonCoveringLaneRanges);
    }

    let mut lanes = (0..channel_count)
        .map(|channel_index| MpeghExternalLane::Channel { channel_index })
        .collect::<Vec<_>>();
    lanes.resize(lane_count, MpeghExternalLane::HoaTransport { transport_index: 0 });

    if let Some(offset) = object_offset {
        for (object_index, object) in oam
            .expect("object offset is only present when parsed OAM exists")
            .objects
            .iter()
            .enumerate()
        {
            let lane = offset + object_index;
            let slot = lanes
                .get_mut(lane)
                .ok_or(MpeghPcmTopologyError::NonCoveringLaneRanges)?;
            *slot = MpeghExternalLane::Object {
                object_index,
                element_id: object.element_id,
            };
        }
    }
    if let Some(offset) = hoa_offset {
        for transport_index in 0..hoa_count {
            let lane = offset + transport_index;
            let slot = lanes
                .get_mut(lane)
                .ok_or(MpeghPcmTopologyError::NonCoveringLaneRanges)?;
            if matches!(slot, MpeghExternalLane::Object { .. }) {
                return Err(MpeghPcmTopologyError::OverlappingObjectAndHoaRanges);
            }
            *slot = MpeghExternalLane::HoaTransport { transport_index };
        }
    }

    let uncovered = lanes.iter().enumerate().any(|(index, lane)| {
        index >= channel_count
            && !matches!(
                lane,
                MpeghExternalLane::Object { .. } | MpeghExternalLane::HoaTransport { .. }
            )
    });
    if uncovered {
        return Err(MpeghPcmTopologyError::NonCoveringLaneRanges);
    }

    Ok(MpeghExternalTopology {
        frame_samples: FRAME_SAMPLES,
        lane_count,
        channel_lane_count: channel_count,
        object_lane_offset: object_offset,
        object_lane_count: object_count,
        hoa_lane_offset: hoa_offset,
        hoa_lane_count: hoa_count,
        lanes,
    })
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghPcmTopologyError {
    #[error("libmpegh external-render PCM bit depth {0} is unsupported; expected 24")]
    UnsupportedBitDepth(i32),
    #[error("libmpegh external-render sample rate {0} is invalid")]
    InvalidSampleRate(i32),
    #[error("libmpegh external-render PCM byte length {bytes} is not a whole 1024-sample 24-bit lane set")]
    InvalidPcmLength { bytes: usize },
    #[error("libmpegh external-render lane count {actual} is outside 1..={maximum}")]
    InvalidLaneCount { actual: usize, maximum: usize },
    #[error("libmpegh OAM lane offset {0} is invalid")]
    InvalidObjectLaneOffset(i32),
    #[error("libmpegh HOA lane offset {0} is invalid")]
    InvalidHoaLaneOffset(i32),
    #[error("libmpegh external-render object and HOA lane ranges overlap")]
    OverlappingObjectAndHoaRanges,
    #[error("libmpegh external-render lane ranges do not exactly cover the pre-render PCM lanes")]
    NonCoveringLaneRanges,
    #[error("external OAM metadata is invalid: {0}")]
    InvalidOam(String),
    #[error("external PCM topology arithmetic overflow")]
    NumericOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MpeghOamObject, MpeghOamPacket};

    fn empty_object(element_id: u16) -> MpeghOamObject {
        MpeghOamObject {
            element_id,
            dynamic_priority_present: false,
            uniform_spread_present: true,
            frames: Vec::new(),
            fixed_position: false,
            group_priority: 0,
            diffuseness_code: 0,
            divergence_code: 0,
            divergence_azimuth_range_code: 0,
            exclusion_sectors: Vec::new(),
        }
    }

    fn oam(objects: usize) -> MpeghOamPacket {
        MpeghOamPacket {
            frame_length_samples: 1024,
            audio_truncation_code: 0,
            truncated_samples: None,
            objects: (0..objects)
                .map(|index| empty_object(100 + index as u16))
                .collect(),
            extensions: Vec::new(),
            consumed_bits: 0,
        }
    }

    fn pcm_bytes(lanes: usize) -> Vec<u8> {
        vec![0; FRAME_SAMPLES * lanes * BYTES_PER_SAMPLE]
    }

    #[test]
    fn topology_uses_authoritative_offsets_when_objects_precede_hoa() {
        let frame = MpeghExternalFrame {
            channel_metadata: vec![1],
            object_metadata: vec![1],
            hoa_metadata: vec![1],
            prerender_pcm: pcm_bytes(8),
            pcm_bit_depth: 24,
            sample_rate: 48_000,
            oam_sample_offset: 2,
            hoa_sample_offset: 5,
            speaker_layout: crate::MpeghSpeakerLayout {
                cicp_index: 0,
                layout_code: 0,
                speakers: Vec::new(),
            },
        };
        let topology = prove_topology(&frame, Some(&oam(3)), 8).unwrap();
        assert_eq!(topology.channel_lane_count, 2);
        assert_eq!(topology.object_lane_offset, Some(2));
        assert_eq!(topology.object_lane_count, 3);
        assert_eq!(topology.hoa_lane_offset, Some(5));
        assert_eq!(topology.hoa_lane_count, 3);
    }

    #[test]
    fn topology_supports_hoa_before_objects_without_guessing() {
        let frame = MpeghExternalFrame {
            channel_metadata: vec![1],
            object_metadata: vec![1],
            hoa_metadata: vec![1],
            prerender_pcm: pcm_bytes(8),
            pcm_bit_depth: 24,
            sample_rate: 48_000,
            oam_sample_offset: 5,
            hoa_sample_offset: 2,
            speaker_layout: crate::MpeghSpeakerLayout {
                cicp_index: 0,
                layout_code: 0,
                speakers: Vec::new(),
            },
        };
        let topology = prove_topology(&frame, Some(&oam(3)), 8).unwrap();
        assert_eq!(topology.channel_lane_count, 2);
        assert_eq!(topology.hoa_lane_offset, Some(2));
        assert_eq!(topology.hoa_lane_count, 3);
        assert_eq!(topology.object_lane_offset, Some(5));
    }

    #[test]
    fn deinterleaves_signed_24_bit_little_endian_sample_major_pcm() {
        let mut bytes = pcm_bytes(2);
        // sample 0 lane 0 = +0.5, lane 1 = -0.5 in signed 24-bit.
        bytes[0..3].copy_from_slice(&[0x00, 0x00, 0x40]);
        bytes[3..6].copy_from_slice(&[0x00, 0x00, 0xC0]);
        let frame = MpeghExternalFrame {
            channel_metadata: Vec::new(),
            object_metadata: Vec::new(),
            hoa_metadata: Vec::new(),
            prerender_pcm: bytes,
            pcm_bit_depth: 24,
            sample_rate: 48_000,
            oam_sample_offset: 0,
            hoa_sample_offset: 0,
            speaker_layout: crate::MpeghSpeakerLayout {
                cicp_index: 0,
                layout_code: 0,
                speakers: Vec::new(),
            },
        };
        let decoded = decode_prerender_pcm(&frame, None).unwrap();
        assert!((decoded.channels[0][0] - 0.5).abs() < 1.0e-6);
        assert!((decoded.channels[1][0] + 0.5).abs() < 1.0e-6);
    }
}
