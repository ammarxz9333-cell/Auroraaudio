use std::collections::VecDeque;

use aurora_core::{AudioBlock, AudioFormat, ChannelRole};
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use aurora_spatial_ir::{
    BedSignalBinding, CoordinateSpace, ObjectSignalBinding, SpatialDecodedFrame, SpatialDomain,
    SpatialFrameMetadata, SpatialObjectUpdate, SpatialPosition,
};
use truehd::process::{
    decode::{DecodedAccessUnit, Decoder as TrueHdDecoder},
    extract::Extractor,
    parse::Parser,
};
use truehd::structs::{
    channel::ChannelLabel,
    oamd::{ObjectAudioMetadataPayload, SpeakerLabels, GAIN_MINUS_INFINITY},
};

const HIGHEST_PRESENTATION: usize = 3;
const PCM_24_SCALE: f32 = 8_388_608.0;
const MAX_SPATIAL_PROBE_AUS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrueHdTopology {
    bed_roles: Vec<ChannelRole>,
    object_count: usize,
}

#[derive(Debug, Clone)]
struct AbsoluteSpatialUpdate {
    absolute_sample: u64,
    update: SpatialObjectUpdate,
}

/// Native TrueHD/MLP decoder using the pinned `truehd` library.
///
/// Aurora deliberately asks for presentation 3: the upstream presentation map
/// resolves an absent request to the highest presentation that actually exists,
/// while an Atmos stream can expose the fourth/16-channel presentation. This
/// gives one deterministic policy for ordinary TrueHD and TrueHD Atmos.
pub struct TruehddDecoderAdapter {
    configured_format: Option<AudioFormat>,
    extractor: Extractor,
    parser: Parser,
    decoder: TrueHdDecoder,
    decoded_queue: VecDeque<DecodedAccessUnit>,
    sample_cursor: u64,
    discontinuity: bool,
    recovered_errors: u64,
    topology: Option<TrueHdTopology>,
    pending_updates: Vec<AbsoluteSpatialUpdate>,
}

impl Default for TruehddDecoderAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TruehddDecoderAdapter {
    pub fn new() -> Self {
        Self {
            configured_format: None,
            extractor: Extractor::default(),
            parser: Parser::default(),
            decoder: TrueHdDecoder::default(),
            decoded_queue: VecDeque::new(),
            sample_cursor: 0,
            discontinuity: true,
            recovered_errors: 0,
            topology: None,
            pending_updates: Vec::new(),
        }
    }

    /// Number of corrupt/extract/parse/decode events recovered by resynchronizing
    /// the parser and decoder in lockstep.
    pub fn recovered_error_count(&self) -> u64 {
        self.recovered_errors
    }

    /// Decode TrueHD while preserving Atmos bed/object lanes and OAMD in Aurora
    /// Spatial IR. The method shares stream state with `Decoder::decode_chunk`;
    /// callers must choose one output mode for a stream and not interleave them.
    pub fn decode_spatial_chunk(
        &mut self,
        input: &[u8],
    ) -> Result<Option<SpatialDecodedFrame>, DecoderError> {
        self.feed(input);
        let Some(decoded) = self.next_spatial_au()? else {
            return Ok(None);
        };
        let consumed = decoded.sample_length as u64;
        match self.convert_spatial(decoded) {
            Ok(frame) => Ok(Some(frame)),
            Err(error) => {
                self.sample_cursor = self.sample_cursor.saturating_add(consumed);
                self.discontinuity = true;
                Err(error)
            }
        }
    }

    fn feed(&mut self, input: &[u8]) {
        if input.is_empty() {
            return;
        }
        self.extractor.push_bytes(input);
        while let Some(frame_result) = self.extractor.next() {
            let frame = match frame_result {
                Ok(frame) => frame,
                Err(_) => {
                    self.recover_stream_state();
                    continue;
                }
            };
            let access_unit = match self.parser.parse(&frame) {
                Ok(access_unit) => access_unit,
                Err(_) => {
                    self.recover_stream_state();
                    continue;
                }
            };
            match self
                .decoder
                .decode_presentation(&access_unit, HIGHEST_PRESENTATION)
            {
                Ok(decoded) if decoded.is_duplicate => {}
                Ok(decoded) => self.decoded_queue.push_back(decoded),
                Err(_) => self.recover_stream_state(),
            }
        }
    }

    fn recover_stream_state(&mut self) {
        self.parser.reset_for_next_major_sync();
        self.decoder.reset_for_next_major_sync();
        self.recovered_errors = self.recovered_errors.saturating_add(1);
        self.discontinuity = true;
        self.topology = None;
        self.pending_updates.clear();
    }

    /// For an object presentation, do not guess spatial meaning before the first
    /// OAMD topology. We retain a short probe window; once OAMD appears, earlier
    /// unknown object AUs are dropped with a discontinuity instead of being
    /// mislabeled as physical speaker channels.
    fn next_spatial_au(&mut self) -> Result<Option<DecodedAccessUnit>, DecoderError> {
        if self.topology.is_some() {
            return Ok(self.decoded_queue.pop_front());
        }
        let Some(front) = self.decoded_queue.front() else {
            return Ok(None);
        };
        if !front.oamd.is_empty() || is_unambiguous_discrete_bed(front) {
            return Ok(self.decoded_queue.pop_front());
        }

        if let Some(oamd_index) = self
            .decoded_queue
            .iter()
            .position(|frame| !frame.oamd.is_empty())
        {
            for _ in 0..oamd_index {
                if let Some(dropped) = self.decoded_queue.pop_front() {
                    self.sample_cursor = self
                        .sample_cursor
                        .saturating_add(dropped.sample_length as u64);
                    self.recovered_errors = self.recovered_errors.saturating_add(1);
                }
            }
            self.discontinuity = true;
            return Ok(self.decoded_queue.pop_front());
        }

        if self.decoded_queue.len() > MAX_SPATIAL_PROBE_AUS {
            if let Some(dropped) = self.decoded_queue.pop_front() {
                self.sample_cursor = self
                    .sample_cursor
                    .saturating_add(dropped.sample_length as u64);
                self.recovered_errors = self.recovered_errors.saturating_add(1);
                self.discontinuity = true;
            }
        }
        Ok(None)
    }

    fn configured_rate(&self) -> Result<u32, DecoderError> {
        self.configured_format
            .map(|format| format.sample_rate)
            .ok_or(DecoderError::Unavailable(
                "TrueHD adapter must be configured before decoding",
            ))
    }

    fn verify_rate(&self, decoded: &DecodedAccessUnit) -> Result<(), DecoderError> {
        let configured = self.configured_rate()?;
        if decoded.sampling_frequency != configured {
            return Err(DecoderError::Decode(format!(
                "TrueHD sample rate changed: configured={configured}, decoded={}",
                decoded.sampling_frequency
            )));
        }
        Ok(())
    }

    fn convert_pcm(&mut self, decoded: DecodedAccessUnit) -> Result<DecodedFrame, DecoderError> {
        self.verify_rate(&decoded)?;
        let channels = planar_f32(&decoded)?;
        let frame_count = decoded.sample_length;
        let sample_rate = decoded.sampling_frequency;
        let frame_start = self.sample_cursor;
        self.sample_cursor = self.sample_cursor.saturating_add(frame_count as u64);
        let discontinuity = std::mem::replace(&mut self.discontinuity, false);
        Ok(DecodedFrame {
            audio: AudioBlock {
                channels,
                frame_count,
                presentation_time_seconds: frame_start as f64 / f64::from(sample_rate),
                discontinuity,
            },
            objects: Vec::new(),
        })
    }

    fn convert_spatial(
        &mut self,
        decoded: DecodedAccessUnit,
    ) -> Result<SpatialDecodedFrame, DecoderError> {
        self.verify_rate(&decoded)?;
        let frame_count = decoded.sample_length;
        let frame_start = self.sample_cursor;
        let frame_end = frame_start
            .checked_add(frame_count as u64)
            .ok_or_else(|| DecoderError::Decode("TrueHD sample clock overflow".into()))?;

        for payload in &decoded.oamd {
            let topology = topology_from_oamd(payload, decoded.channel_count)?;
            if let Some(previous) = &self.topology {
                if previous != &topology {
                    self.pending_updates.clear();
                    self.discontinuity = true;
                }
            }
            self.topology = Some(topology.clone());
            self.append_oamd_updates(payload, &topology, frame_start)?;
        }

        let spatial = if let Some(topology) = self.topology.clone() {
            if topology.bed_roles.len().saturating_add(topology.object_count)
                != decoded.channel_count
            {
                return Err(DecoderError::Decode(
                    "TrueHD decoded channel geometry disagrees with retained Atmos topology".into(),
                ));
            }
            let object_updates = self.take_updates_for_frame(frame_start, frame_end)?;
            metadata_from_topology(&topology, object_updates)
        } else {
            discrete_metadata(&decoded)?
        };

        let channels = planar_f32(&decoded)?;
        let discontinuity = std::mem::replace(&mut self.discontinuity, false);
        self.sample_cursor = frame_end;
        let frame = SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels,
                    frame_count,
                    presentation_time_seconds: frame_start as f64
                        / f64::from(decoded.sampling_frequency),
                    discontinuity,
                },
                objects: Vec::new(),
            },
            spatial,
        };
        frame.validate().map_err(|error| {
            DecoderError::Decode(format!("Aurora Spatial IR rejected TrueHD: {error}"))
        })?;
        Ok(frame)
    }

    fn append_oamd_updates(
        &mut self,
        payload: &ObjectAudioMetadataPayload,
        topology: &TrueHdTopology,
        frame_start: u64,
    ) -> Result<(), DecoderError> {
        let Some(object_element) = &payload.object_element else {
            return Ok(());
        };
        let timing = &object_element.md_update_info;
        if timing.block_update_info.len() != timing.num_obj_info_blocks {
            return Err(DecoderError::Decode(
                "TrueHD OAMD timing block count is inconsistent".into(),
            ));
        }
        let dynamic_start = topology.bed_roles.len();
        if object_element.object_data.len() < dynamic_start.saturating_add(topology.object_count) {
            return Err(DecoderError::Decode(
                "TrueHD OAMD object grid is smaller than the decoded object topology".into(),
            ));
        }

        for object_index in 0..topology.object_count {
            let oamd_index = dynamic_start + object_index;
            let blocks = object_element.object_data.get(oamd_index).ok_or_else(|| {
                DecoderError::Decode("TrueHD dynamic OAMD object index is missing".into())
            })?;
            if blocks.len() != timing.num_obj_info_blocks {
                return Err(DecoderError::Decode(
                    "TrueHD OAMD object/timing block geometry is inconsistent".into(),
                ));
            }

            for (block_index, block) in blocks.iter().enumerate() {
                if block.b_object_in_bed_or_isf {
                    return Err(DecoderError::Decode(
                        "TrueHD dynamic-object index resolves to bed/ISF metadata".into(),
                    ));
                }
                let render = &block.object_render_info;
                reject_unrepresented_render_metadata(render)?;
                let extension = extended_position(payload, oamd_index, block_index)?;
                let spread = object_divergence(payload, oamd_index, block_index)?;
                let x = (render.pos3d[0] + extension[0]).clamp(0.0, 1.0) as f32;
                let y = (1.0 - (render.pos3d[1] + extension[1]).clamp(0.0, 1.0)) as f32;
                let z = (render.pos3d[2] + extension[2]).clamp(-1.0, 1.0) as f32;
                let gain_db = if block.object_basic_info.object_gain == GAIN_MINUS_INFINITY {
                    f32::NEG_INFINITY
                } else {
                    f32::from(block.object_basic_info.object_gain)
                };
                let priority = block.object_basic_info.object_priority as f32;
                if !priority.is_finite() || !(0.0..=1.0).contains(&priority) {
                    return Err(DecoderError::Decode(
                        "TrueHD object priority is outside the normalized range".into(),
                    ));
                }

                let timing_block = timing.block_update_info.get(block_index).ok_or_else(|| {
                    DecoderError::Decode("TrueHD OAMD timing block is missing".into())
                })?;
                let local = u64::try_from(timing.sample_offset)
                    .map_err(|_| DecoderError::Decode("TrueHD OAMD sample offset overflow".into()))?
                    .checked_add(u64::from(timing_block.block_offset_factor_bits) * 32)
                    .and_then(|value| value.checked_add(payload.evo_sample_offset))
                    .ok_or_else(|| DecoderError::Decode("TrueHD OAMD timing overflow".into()))?;
                let absolute_sample = frame_start
                    .checked_add(local)
                    .ok_or_else(|| DecoderError::Decode("TrueHD OAMD sample clock overflow".into()))?;

                self.pending_updates.push(AbsoluteSpatialUpdate {
                    absolute_sample,
                    update: SpatialObjectUpdate {
                        object_id: truehd_object_id(object_index),
                        active: !block.b_object_not_active,
                        coordinate_space: CoordinateSpace::RoomNormalized,
                        position: SpatialPosition::Cartesian { x, y, z },
                        gain_db,
                        spread,
                        metadata_sample_offset: 0,
                        ramp_duration_samples: u32::from(timing_block.ramp_duration),
                        priority: Some(priority),
                    },
                });
            }
        }
        self.pending_updates
            .sort_by_key(|update| update.absolute_sample);
        Ok(())
    }

    fn take_updates_for_frame(
        &mut self,
        frame_start: u64,
        frame_end: u64,
    ) -> Result<Vec<SpatialObjectUpdate>, DecoderError> {
        let mut current = Vec::new();
        let mut future = Vec::with_capacity(self.pending_updates.len());
        for mut pending in self.pending_updates.drain(..) {
            if pending.absolute_sample < frame_start {
                return Err(DecoderError::Decode(
                    "TrueHD OAMD metadata arrived behind the active sample cursor".into(),
                ));
            }
            if pending.absolute_sample <= frame_end {
                pending.update.metadata_sample_offset = u32::try_from(
                    pending.absolute_sample - frame_start,
                )
                .map_err(|_| DecoderError::Decode("TrueHD OAMD frame offset overflow".into()))?;
                current.push(pending.update);
            } else {
                future.push(pending);
            }
        }
        self.pending_updates = future;
        current.sort_by_key(|update| update.metadata_sample_offset);
        Ok(current)
    }
}

impl Decoder for TruehddDecoderAdapter {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "Aurora native TrueHD/Atmos decoder",
            production_ready: false,
            maturity: "native-truehd-object-preserving-experimental",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        if output_format.sample_rate == 0 {
            return Err(DecoderError::UnsupportedInput(
                "TrueHD output sample rate must be non-zero",
            ));
        }
        self.configured_format = Some(output_format);
        Ok(())
    }

    fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        self.feed(input);
        let Some(decoded) = self.decoded_queue.pop_front() else {
            return Ok(None);
        };
        let consumed = decoded.sample_length as u64;
        match self.convert_pcm(decoded) {
            Ok(frame) => Ok(Some(frame)),
            Err(error) => {
                self.sample_cursor = self.sample_cursor.saturating_add(consumed);
                self.discontinuity = true;
                Err(error)
            }
        }
    }

    fn reset(&mut self) {
        let configured = self.configured_format;
        *self = Self::new();
        self.configured_format = configured;
    }
}

fn planar_f32(decoded: &DecodedAccessUnit) -> Result<Vec<Vec<f32>>, DecoderError> {
    if decoded.channel_count == 0 || decoded.channel_count > 16 || decoded.sample_length > 160 {
        return Err(DecoderError::Decode(
            "TrueHD decoded PCM geometry exceeds the upstream contract".into(),
        ));
    }
    let mut channels = (0..decoded.channel_count)
        .map(|_| Vec::with_capacity(decoded.sample_length))
        .collect::<Vec<_>>();
    for sample_index in 0..decoded.sample_length {
        for (channel_index, channel) in channels.iter_mut().enumerate() {
            let sample = decoded.pcm_data[sample_index][channel_index];
            if !(-8_388_608..=8_388_607).contains(&sample) {
                return Err(DecoderError::Decode(
                    "TrueHD decoder emitted a sample outside signed 24-bit PCM".into(),
                ));
            }
            channel.push(sample as f32 / PCM_24_SCALE);
        }
    }
    Ok(channels)
}

fn is_unambiguous_discrete_bed(decoded: &DecodedAccessUnit) -> bool {
    decoded.channel_count <= 8 && decoded.channel_labels.len() == decoded.channel_count
}

fn discrete_metadata(decoded: &DecodedAccessUnit) -> Result<SpatialFrameMetadata, DecoderError> {
    if decoded.channel_labels.len() != decoded.channel_count {
        return Err(DecoderError::UnsupportedInput(
            "TrueHD spatial topology is unknown until OAMD is observed",
        ));
    }
    let bed_signals = decoded
        .channel_labels
        .iter()
        .copied()
        .enumerate()
        .map(|(pcm_channel_index, label)| BedSignalBinding {
            pcm_channel_index,
            role: role_from_channel_label(label),
        })
        .collect();
    Ok(SpatialFrameMetadata {
        domain: SpatialDomain::DiscreteBed,
        bed_signals,
        object_signals: Vec::new(),
        object_updates: Vec::new(),
    })
}

fn topology_from_oamd(
    payload: &ObjectAudioMetadataPayload,
    decoded_channels: usize,
) -> Result<TrueHdTopology, DecoderError> {
    if payload.program_assignment.bed_assignment.len() > 1 {
        return Err(DecoderError::UnsupportedInput(
            "TrueHD multiple OAMD bed instances need a dedicated multi-bed binding model",
        ));
    }
    if payload.program_assignment.num_isf_objects != 0 {
        return Err(DecoderError::UnsupportedInput(
            "TrueHD intermediate spatial format objects are not admitted yet",
        ));
    }
    let bed_indices = payload
        .program_assignment
        .bed_assignment
        .first()
        .map(|bed| bed.to_index_vec())
        .unwrap_or_default();
    if bed_indices.len() != payload.program_assignment.num_bed_objects {
        return Err(DecoderError::Decode(
            "TrueHD OAMD bed assignment count is internally inconsistent".into(),
        ));
    }
    let object_count = payload.program_assignment.num_dynamic_objects;
    if bed_indices.len().saturating_add(object_count) != decoded_channels {
        return Err(DecoderError::Decode(format!(
            "TrueHD Atmos lane count mismatch: beds={} objects={} decoded={decoded_channels}",
            bed_indices.len(),
            object_count
        )));
    }
    let mut bed_roles = Vec::with_capacity(bed_indices.len());
    for index in bed_indices {
        let label = SpeakerLabels::from_u8(index as u8).ok_or_else(|| {
            DecoderError::Decode("TrueHD OAMD bed speaker label is out of range".into())
        })?;
        bed_roles.push(role_from_speaker_label(label));
    }
    Ok(TrueHdTopology {
        bed_roles,
        object_count,
    })
}

fn metadata_from_topology(
    topology: &TrueHdTopology,
    object_updates: Vec<SpatialObjectUpdate>,
) -> SpatialFrameMetadata {
    let bed_signals = topology
        .bed_roles
        .iter()
        .cloned()
        .enumerate()
        .map(|(pcm_channel_index, role)| BedSignalBinding {
            pcm_channel_index,
            role,
        })
        .collect::<Vec<_>>();
    let object_signals = (0..topology.object_count)
        .map(|object_index| ObjectSignalBinding {
            id: truehd_object_id(object_index),
            pcm_channel_index: topology.bed_roles.len() + object_index,
        })
        .collect::<Vec<_>>();
    let domain = match (bed_signals.is_empty(), object_signals.is_empty()) {
        (false, false) => SpatialDomain::BedAndObjects,
        (true, false) => SpatialDomain::ObjectSignals,
        _ => SpatialDomain::DiscreteBed,
    };
    SpatialFrameMetadata {
        domain,
        bed_signals,
        object_signals,
        object_updates,
    }
}

fn extended_position(
    payload: &ObjectAudioMetadataPayload,
    object_index: usize,
    block_index: usize,
) -> Result<[f64; 3], DecoderError> {
    let Some(extended) = &payload.extended_object_element else {
        return Ok([0.0; 3]);
    };
    if !extended.b_ext_prec_pos_block {
        return Ok([0.0; 3]);
    }
    let block = extended
        .ext_prec_pos_block
        .get(object_index)
        .and_then(|object| object.get(block_index))
        .ok_or_else(|| {
            DecoderError::Decode("TrueHD extended-position grid is inconsistent".into())
        })?;
    Ok([
        block.ext_prec_pos3d_x,
        block.ext_prec_pos3d_y,
        block.ext_prec_pos3d_z,
    ])
}

fn object_divergence(
    payload: &ObjectAudioMetadataPayload,
    object_index: usize,
    block_index: usize,
) -> Result<f32, DecoderError> {
    let Some(extended) = &payload.extended_object_element else {
        return Ok(0.0);
    };
    if !extended.b_obj_div_block {
        return Ok(0.0);
    }
    let block = extended
        .object_div_block
        .get(object_index)
        .and_then(|object| object.get(block_index))
        .ok_or_else(|| DecoderError::Decode("TrueHD divergence grid is inconsistent".into()))?;
    let divergence = if block.b_object_divergence {
        block.object_divergence as f32
    } else {
        0.0
    };
    if !divergence.is_finite() || !(0.0..=1.0).contains(&divergence) {
        return Err(DecoderError::Decode(
            "TrueHD object divergence is outside the normalized range".into(),
        ));
    }
    Ok(divergence)
}

fn reject_unrepresented_render_metadata(
    render: &truehd::structs::oamd::ObjectRenderInfo,
) -> Result<(), DecoderError> {
    if render.b_object_distance_specified || render.distance_factor.is_some() {
        return Err(DecoderError::UnsupportedInput(
            "TrueHD object distance needs an explicit Aurora Spatial IR field",
        ));
    }
    if render.zone_constraints_idx != 0 {
        return Err(DecoderError::UnsupportedInput(
            "TrueHD object zone constraints need an explicit Aurora Spatial IR field",
        ));
    }
    if render.object_size.iter().any(|value| value.abs() > f64::EPSILON) {
        return Err(DecoderError::UnsupportedInput(
            "TrueHD 3D object size needs an explicit Aurora Spatial IR extent field",
        ));
    }
    if render.b_object_use_screen_ref || render.screen_factor.abs() > f64::EPSILON {
        return Err(DecoderError::UnsupportedInput(
            "TrueHD screen-referenced objects need an explicit Aurora Spatial IR field",
        ));
    }
    if render.b_object_snap {
        return Err(DecoderError::UnsupportedInput(
            "TrueHD object snap needs an explicit Aurora Spatial IR field",
        ));
    }
    if !render.b_enable_elevation {
        return Err(DecoderError::UnsupportedInput(
            "TrueHD elevation-disabled object constraint needs explicit Spatial IR semantics",
        ));
    }
    Ok(())
}

fn truehd_object_id(index: usize) -> String {
    format!("truehd-object-{index}")
}

fn role_from_speaker_label(label: SpeakerLabels) -> ChannelRole {
    match label {
        SpeakerLabels::L => ChannelRole::FrontLeft,
        SpeakerLabels::R => ChannelRole::FrontRight,
        SpeakerLabels::C => ChannelRole::FrontCenter,
        SpeakerLabels::LFE => ChannelRole::LowFrequencyEffects,
        SpeakerLabels::Lss => ChannelRole::SurroundLeft,
        SpeakerLabels::Rss => ChannelRole::SurroundRight,
        SpeakerLabels::Lrs => ChannelRole::SurroundBackLeft,
        SpeakerLabels::Rrs => ChannelRole::SurroundBackRight,
        SpeakerLabels::Lfh => ChannelRole::TopFrontLeft,
        SpeakerLabels::Rfh => ChannelRole::TopFrontRight,
        SpeakerLabels::Lts => ChannelRole::Custom("top-side-left".into()),
        SpeakerLabels::Rts => ChannelRole::Custom("top-side-right".into()),
        SpeakerLabels::Lrh => ChannelRole::TopRearLeft,
        SpeakerLabels::Rrh => ChannelRole::TopRearRight,
        SpeakerLabels::Lw => ChannelRole::Custom("front-wide-left".into()),
        SpeakerLabels::Rw => ChannelRole::Custom("front-wide-right".into()),
        SpeakerLabels::LFE2 => ChannelRole::Custom("low-frequency-effects-2".into()),
    }
}

fn role_from_channel_label(label: ChannelLabel) -> ChannelRole {
    match label {
        ChannelLabel::L => ChannelRole::FrontLeft,
        ChannelLabel::R => ChannelRole::FrontRight,
        ChannelLabel::C => ChannelRole::FrontCenter,
        ChannelLabel::LFE => ChannelRole::LowFrequencyEffects,
        ChannelLabel::Ls => ChannelRole::SurroundLeft,
        ChannelLabel::Rs => ChannelRole::SurroundRight,
        ChannelLabel::Lb => ChannelRole::SurroundBackLeft,
        ChannelLabel::Rb => ChannelRole::SurroundBackRight,
        ChannelLabel::Tfl => ChannelRole::TopFrontLeft,
        ChannelLabel::Tfr => ChannelRole::TopFrontRight,
        ChannelLabel::Tsl => ChannelRole::Custom("top-side-left".into()),
        ChannelLabel::Tsr => ChannelRole::Custom("top-side-right".into()),
        ChannelLabel::Tbl => ChannelRole::TopRearLeft,
        ChannelLabel::Tbr => ChannelRole::TopRearRight,
        ChannelLabel::Lsc => ChannelRole::Custom("left-surround-center".into()),
        ChannelLabel::Rsc => ChannelRole::Custom("right-surround-center".into()),
        ChannelLabel::Cb => ChannelRole::Custom("center-back".into()),
        ChannelLabel::Tc => ChannelRole::Custom("top-center".into()),
        ChannelLabel::Lsd => ChannelRole::Custom("left-surround-direct".into()),
        ChannelLabel::Rsd => ChannelRole::Custom("right-surround-direct".into()),
        ChannelLabel::Lw => ChannelRole::Custom("front-wide-left".into()),
        ChannelLabel::Rw => ChannelRole::Custom("front-wide-right".into()),
        ChannelLabel::Tfc => ChannelRole::Custom("top-front-center".into()),
        ChannelLabel::LFE2 => ChannelRole::Custom("low-frequency-effects-2".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_truehd_bed_labels_have_deterministic_aurora_roles() {
        for index in 0..=SpeakerLabels::LFE2 as u8 {
            let label = SpeakerLabels::from_u8(index).unwrap();
            assert!(!role_from_speaker_label(label).as_str().is_empty());
        }
    }

    #[test]
    fn truehd_24_bit_pcm_scaling_reaches_expected_full_scale() {
        assert!((8_388_607.0_f32 / PCM_24_SCALE) < 1.0);
        assert_eq!(-8_388_608.0_f32 / PCM_24_SCALE, -1.0);
    }
}
