use aurora_core::{AudioBlock, ChannelRole};
use aurora_decoder_api::{DecodedFrame, DecoderError};
use oxideav_ac4::ajoc_substream::{AjocBodyParams, AjocSubstreamDecoder, AudioDataAjoc};
use oxideav_ac4::oamd::{InfoStatus, ObjType, ObjectGain, RenderPosition};
use oxideav_ac4::sync::parse_sync_frame_at_start;
use oxideav_ac4::toc::parse_ac4_toc;

use crate::spatial_ir::{
    BedSignalBinding, CoordinateSpace, ObjectSignalBinding, SpatialDecodedFrame,
    SpatialDomain, SpatialFrameMetadata, SpatialObjectUpdate, SpatialPosition,
};

#[derive(Debug, Clone, Copy)]
struct ResolvedObjectState {
    position: Option<(f32, f32, f32)>,
    gain_db: f32,
    priority: Option<f32>,
    active: bool,
}

impl Default for ResolvedObjectState {
    fn default() -> Self {
        Self {
            position: None,
            gain_db: 0.0,
            priority: Some(1.0),
            active: false,
        }
    }
}

/// Native AC-4 A-JOC pre-render decoder.
///
/// Unlike `NativeAc4Decoder`, this path never interprets reconstructed A-JOC
/// objects as speaker channels. OxideAV reconstructs fullband object PCM and
/// returns the parsed OAMD body in the same stateful call; Aurora then resolves
/// the supported OAMD subset into its codec-neutral Spatial IR.
pub struct NativeAc4SpatialDecoder {
    decoder: Option<(usize, usize, AjocSubstreamDecoder)>,
    state: Vec<ResolvedObjectState>,
    sample_rate: Option<u32>,
    emitted_frames: u64,
    discontinuity: bool,
}

impl NativeAc4SpatialDecoder {
    pub fn new() -> Self {
        Self {
            decoder: None,
            state: Vec::new(),
            sample_rate: None,
            emitted_frames: 0,
            discontinuity: true,
        }
    }

    pub fn configure(&mut self, sample_rate: u32) -> Result<(), DecoderError> {
        if sample_rate == 0 {
            return Err(DecoderError::UnsupportedInput(
                "AC-4 spatial decoder requires a non-zero sample rate",
            ));
        }
        self.sample_rate = Some(sample_rate);
        Ok(())
    }

    pub fn reset(&mut self) {
        let sample_rate = self.sample_rate;
        *self = Self::new();
        self.sample_rate = sample_rate;
    }

    /// Decode one complete raw AC-4 access unit or Annex-G sync frame.
    /// Returns `Ok(None)` when the AU is AC-4 but does not carry A-JOC.
    pub fn decode_access_unit(
        &mut self,
        packet: &[u8],
    ) -> Result<Option<SpatialDecodedFrame>, DecoderError> {
        if packet.is_empty() {
            return Ok(None);
        }
        let raw = match parse_sync_frame_at_start(packet) {
            Some(sync) => {
                if sync.crc_valid == Some(false) {
                    return Err(DecoderError::Decode(
                        "AC-4 Annex G CRC verification failed".into(),
                    ));
                }
                sync.payload
            }
            None => packet,
        };
        let info = parse_ac4_toc(raw)
            .map_err(|error| DecoderError::Decode(format!("AC-4 TOC parse failed: {error}")))?;
        if info.ajoc_substreams.is_empty() {
            return Ok(None);
        }
        let configured_rate = self.sample_rate.ok_or(DecoderError::Unavailable(
            "AC-4 spatial decoder is not configured",
        ))?;
        if info.sample_rate != configured_rate {
            return Err(DecoderError::UnsupportedInput(
                "AC-4 spatial sample-rate conversion is not wired yet",
            ));
        }
        if info.ajoc_substreams.len() != 1 {
            return Err(DecoderError::UnsupportedInput(
                "multi-A-JOC-substream presentation is not mapped into Aurora Spatial IR yet",
            ));
        }

        let desc = info.ajoc_substreams[0].clone();
        let params = AjocBodyParams::from_substream_info(&desc);
        let num_dmx = params.n_fullband_dmx_signals as usize;
        let num_umx = params.n_fullband_upmix_signals as usize;
        if num_umx == 0 {
            return Err(DecoderError::UnsupportedInput(
                "A-JOC access unit declares zero fullband upmix objects",
            ));
        }

        let start = (info.toc_size + info.payload_base) as usize;
        if start >= raw.len() {
            return Err(DecoderError::Decode(
                "AC-4 A-JOC frame has no substream payload".into(),
            ));
        }
        let end = info
            .substream_sizes
            .first()
            .map(|size| start.saturating_add(*size as usize).min(raw.len()))
            .unwrap_or(raw.len());
        if end <= start {
            return Err(DecoderError::Decode(
                "AC-4 A-JOC substream has invalid bounds".into(),
            ));
        }

        let geometry_changed = self
            .decoder
            .as_ref()
            .map(|(dmx, umx, _)| *dmx != num_dmx || *umx != num_umx)
            .unwrap_or(true);
        if geometry_changed {
            self.decoder = Some((num_dmx, num_umx, AjocSubstreamDecoder::new(num_dmx, num_umx)));
            self.state = vec![ResolvedObjectState::default(); num_umx];
            self.discontinuity = true;
        }
        let decoder = &mut self
            .decoder
            .as_mut()
            .ok_or_else(|| DecoderError::Decode("A-JOC decoder state was not initialized".into()))?
            .2;
        let (objects, lfe, body, _metadata) = decoder
            .decode_substream_pcm(
                &raw[start..end],
                &params,
                desc.b_iframe(),
                false,
                info.frame_length,
            )
            .map_err(|error| DecoderError::Decode(format!("A-JOC object decode failed: {error}")))?;

        if objects.len() != num_umx {
            return Err(DecoderError::Decode(format!(
                "A-JOC object geometry mismatch: descriptor={num_umx}, decoded={}",
                objects.len()
            )));
        }
        let frame_count = objects.first().map(Vec::len).unwrap_or(0);
        if frame_count == 0 || objects.iter().any(|plane| plane.len() != frame_count) {
            return Err(DecoderError::Decode(
                "A-JOC object PCM has inconsistent geometry".into(),
            ));
        }
        if params.b_lfe != lfe.is_some() {
            return Err(DecoderError::Decode(
                "A-JOC LFE presence disagrees with descriptor".into(),
            ));
        }
        if let Some(lfe_plane) = &lfe {
            if lfe_plane.len() != frame_count {
                return Err(DecoderError::Decode(
                    "A-JOC LFE PCM length differs from object PCM".into(),
                ));
            }
        }

        let spatial = self.resolve_oamd(&params, &body, frame_count)?;
        let mut channels = Vec::with_capacity(objects.len() + usize::from(lfe.is_some()));
        if let Some(lfe_plane) = lfe {
            channels.push(lfe_plane);
        }
        channels.extend(objects);

        let pts = self.emitted_frames as f64 / f64::from(configured_rate);
        self.emitted_frames = self.emitted_frames.saturating_add(frame_count as u64);
        let discontinuity = std::mem::replace(&mut self.discontinuity, false);
        let frame = SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels,
                    frame_count,
                    presentation_time_seconds: pts,
                    discontinuity,
                },
                objects: Vec::new(),
            },
            spatial,
        };
        frame
            .validate()
            .map_err(|error| DecoderError::Decode(format!("Aurora Spatial IR rejected AC-4: {error}")))?;
        Ok(Some(frame))
    }

    fn resolve_oamd(
        &mut self,
        params: &AjocBodyParams,
        body: &AudioDataAjoc,
        frame_count: usize,
    ) -> Result<SpatialFrameMetadata, DecoderError> {
        if body.umx_dyndata.alt.is_some() {
            return Err(DecoderError::UnsupportedInput(
                "AC-4 alternative OAMD sets are not admitted by the native spatial path yet",
            ));
        }
        let lfe_offset = usize::from(params.b_lfe);
        if params.obj_type_umx.len() != params.n_fullband_upmix_signals as usize + lfe_offset {
            return Err(DecoderError::Decode(
                "A-JOC OAMD object-type geometry is inconsistent".into(),
            ));
        }
        for ty in params.obj_type_umx.iter().skip(lfe_offset) {
            if *ty != ObjType::Dyn {
                return Err(DecoderError::UnsupportedInput(
                    "AC-4 Bed/ISF object mapping requires the dedicated speaker-assignment bridge",
                ));
            }
        }
        if body.umx_dyndata.object_blocks.len() != params.obj_type_umx.len() {
            return Err(DecoderError::Decode(
                "A-JOC OAMD object-block geometry does not match object types".into(),
            ));
        }

        let timing = if let Some(timing) = body.umx_timing.as_ref() {
            Some(timing)
        } else if body.b_derive_timing_from_dmx == Some(true) {
            body.dmx_timing.as_ref()
        } else {
            None
        };
        let max_blocks = body
            .umx_dyndata
            .object_blocks
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(0);
        if max_blocks > 1 && timing.is_none() {
            return Err(DecoderError::Decode(
                "multi-block A-JOC OAMD has no resolvable timing grid".into(),
            ));
        }

        let mut object_signals = Vec::with_capacity(params.n_fullband_upmix_signals as usize);
        for object_index in 0..params.n_fullband_upmix_signals as usize {
            object_signals.push(ObjectSignalBinding {
                id: object_id(object_index),
                pcm_channel_index: lfe_offset + object_index,
            });
        }

        let mut updates = Vec::new();
        for block_index in 0..max_blocks.max(1) {
            let (offset, ramp) = timing_for_block(timing, block_index)?;
            if offset > frame_count as u32 {
                return Err(DecoderError::Decode(
                    "A-JOC OAMD update lies beyond decoded access unit".into(),
                ));
            }
            let mut previous_object_gain: Option<f32> = None;
            for object_index in 0..params.n_fullband_upmix_signals as usize {
                let oamd_index = object_index + lfe_offset;
                let Some(block) = body
                    .umx_dyndata
                    .object_blocks
                    .get(oamd_index)
                    .and_then(|blocks| blocks.get(block_index))
                else {
                    continue;
                };
                if block.add_table_data.is_some() {
                    return Err(DecoderError::UnsupportedInput(
                        "AC-4 extended per-object OAMD is not mapped into Aurora Spatial IR yet",
                    ));
                }
                if let Some(render) = block.render_info.as_ref() {
                    if render.zone.is_some() || render.otherprops.is_some() {
                        return Err(DecoderError::UnsupportedInput(
                            "AC-4 object zones/divergence are not mapped into Aurora Spatial IR yet",
                        ));
                    }
                }

                let state = self.state.get_mut(object_index).ok_or_else(|| {
                    DecoderError::Decode("A-JOC object state index is out of range".into())
                })?;
                if block.b_object_not_active {
                    let was_active = state.active;
                    state.active = false;
                    previous_object_gain = Some(state.gain_db);
                    if was_active {
                        let (x, y, z) = state.position.ok_or_else(|| {
                            DecoderError::Decode(
                                "active A-JOC object became inactive without a resolved position".into(),
                            )
                        })?;
                        updates.push(SpatialObjectUpdate {
                            object_id: object_id(object_index),
                            active: false,
                            coordinate_space: CoordinateSpace::RoomNormalized,
                            position: SpatialPosition::Cartesian { x, y, z },
                            gain_db: state.gain_db,
                            spread: 0.0,
                            metadata_sample_offset: offset,
                            ramp_duration_samples: ramp,
                            priority: state.priority,
                        });
                    }
                    continue;
                }
                state.active = true;

                match block.basic_status {
                    InfoStatus::Default => {
                        state.gain_db = 0.0;
                        state.priority = Some(1.0);
                    }
                    InfoStatus::Reuse | InfoStatus::PartReuse => {}
                    InfoStatus::AllNew => {
                        let basic = block.basic_info.as_ref().ok_or_else(|| {
                            DecoderError::Decode(
                                "A-JOC ALL_NEW basic-info status has no payload".into(),
                            )
                        })?;
                        if basic.b_default {
                            state.gain_db = 0.0;
                            state.priority = Some(1.0);
                        } else {
                            state.gain_db = match basic.gain {
                                Some(ObjectGain::Value(value)) => {
                                    if value <= 14 {
                                        (15 - value as i32) as f32
                                    } else {
                                        (14 - value as i32) as f32
                                    }
                                }
                                Some(ObjectGain::NegInf) => f32::NEG_INFINITY,
                                Some(ObjectGain::PrevObject) => previous_object_gain.ok_or(
                                    DecoderError::UnsupportedInput(
                                        "AC-4 previous-object gain has no resolvable predecessor",
                                    ),
                                )?,
                                None => 0.0,
                            };
                            state.priority = basic
                                .priority_code
                                .map(|value| f32::from(value) / 31.0)
                                .or(Some(1.0));
                        }
                    }
                }

                match block.render_status {
                    InfoStatus::Default | InfoStatus::Reuse => {}
                    InfoStatus::AllNew | InfoStatus::PartReuse => {
                        if let Some(render) = block.render_info.as_ref() {
                            if let Some(position) = render.position {
                                state.position = Some(resolve_position(state.position, position)?);
                            }
                        }
                    }
                }
                previous_object_gain = Some(state.gain_db);

                let Some((x, y, z)) = state.position else {
                    return Err(DecoderError::UnsupportedInput(
                        "active AC-4 dynamic object has no resolved position",
                    ));
                };
                updates.push(SpatialObjectUpdate {
                    object_id: object_id(object_index),
                    active: true,
                    coordinate_space: CoordinateSpace::RoomNormalized,
                    position: SpatialPosition::Cartesian { x, y, z },
                    gain_db: state.gain_db,
                    // Divergence is intentionally rejected above until its exact
                    // source semantics are represented in the codec-neutral IR.
                    spread: 0.0,
                    metadata_sample_offset: offset,
                    ramp_duration_samples: ramp,
                    priority: state.priority,
                });
            }
        }
        updates.sort_by_key(|update| update.metadata_sample_offset);

        let mut bed_signals = Vec::new();
        if params.b_lfe {
            bed_signals.push(BedSignalBinding {
                pcm_channel_index: 0,
                role: ChannelRole::LowFrequencyEffects,
            });
        }
        Ok(SpatialFrameMetadata {
            domain: if params.b_lfe {
                SpatialDomain::BedAndObjects
            } else {
                SpatialDomain::ObjectSignals
            },
            bed_signals,
            object_signals,
            object_updates: updates,
        })
    }
}

impl Default for NativeAc4SpatialDecoder {
    fn default() -> Self {
        Self::new()
    }
}

fn object_id(index: usize) -> String {
    format!("ac4-object-{index}")
}

fn resolve_position(
    previous: Option<(f32, f32, f32)>,
    position: RenderPosition,
) -> Result<(f32, f32, f32), DecoderError> {
    let resolved = match position {
        RenderPosition::Abs { x, y, z_sign, z } => (
            f32::from(x) / 62.0,
            f32::from(y) / 62.0,
            if z_sign {
                f32::from(z) / 15.0
            } else {
                -f32::from(z) / 15.0
            },
        ),
        RenderPosition::Diff { x, y, z } => {
            let (px, py, pz) = previous.ok_or(DecoderError::UnsupportedInput(
                "AC-4 differential object position has no previous state",
            ))?;
            (
                px + f32::from(x) / 62.0,
                py + f32::from(y) / 62.0,
                pz + f32::from(z) / 15.0,
            )
        }
    };
    if !resolved.0.is_finite()
        || !resolved.1.is_finite()
        || !resolved.2.is_finite()
        || !(0.0..=1.0).contains(&resolved.0)
        || !(0.0..=1.0).contains(&resolved.1)
        || !(-1.0..=1.0).contains(&resolved.2)
    {
        return Err(DecoderError::Decode(
            "AC-4 object position resolved outside normalized room bounds".into(),
        ));
    }
    Ok(resolved)
}

fn timing_for_block(
    timing: Option<&oxideav_ac4::oamd::OamdTimingData>,
    block_index: usize,
) -> Result<(u32, u32), DecoderError> {
    let Some(timing) = timing else {
        if block_index == 0 {
            return Ok((0, 0));
        }
        return Err(DecoderError::Decode(
            "A-JOC OAMD block has no timing entry".into(),
        ));
    };
    let block = timing.blocks.get(block_index).ok_or_else(|| {
        DecoderError::Decode("A-JOC OAMD timing grid is shorter than object grid".into())
    })?;
    let offset = timing
        .sample_offset
        .samples()
        .saturating_add(u32::from(block.block_offset_factor).saturating_mul(32));
    Ok((offset, block.ramp_duration.samples()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_ac4_position_maps_to_room_normalized_coordinates() {
        let p = resolve_position(
            None,
            RenderPosition::Abs {
                x: 31,
                y: 62,
                z_sign: true,
                z: 15,
            },
        )
        .unwrap();
        assert_eq!(p, (0.5, 1.0, 1.0));
    }

    #[test]
    fn differential_position_requires_previous_state() {
        assert!(resolve_position(None, RenderPosition::Diff { x: 1, y: 0, z: 0 }).is_err());
    }
}
