use std::collections::BTreeMap;

use aurora_core::{AudioBlock, ChannelRole, Listener, Speaker, Vector3};
use aurora_decoder_engine::scene_timeline::{
    RoomTransform, SceneTimeline, SceneTimelineError, SpatialRenderPlan,
};
use aurora_renderer_api::{
    RenderObject, Renderer, RendererError, RendererScratch, SpeakerGain,
};
use aurora_renderer_vbap::Vbap3dRenderer;
use aurora_spatial_ir::{
    BedSignalBinding as BedSignalBindingV1, ObjectSignalBinding as ObjectSignalBindingV1,
    SpatialDecodedFrame as SpatialDecodedFrameV1, SpatialFrameMetadata as SpatialFrameMetadataV1,
    SpatialObjectUpdate as SpatialObjectUpdateV1,
};
use aurora_spatial_ir_v2::{
    ObjectDistance, SpatialDecodedFrame as SpatialDecodedFrameV2, SpatialObjectUpdate,
    SpatialRenderingProperties, ZoneConstraint,
};
use aurora_spatial_runtime::SpatialRuntimeConfig;
use thiserror::Error;

const INFINITY_RENDER_DISTANCE_METERS: f32 = 1_000_000.0;
const EPSILON: f32 = 1.0e-8;

/// Native Spatial IR V2 speaker renderer.
///
/// This is intentionally a correctness/reference path: metadata updates are
/// applied at exact sample offsets and 3D VBAP is evaluated per sample. Rich
/// properties are consumed here instead of being stripped to V1. Properties
/// without admitted speaker-render semantics still fail closed.
pub struct NativeV2SpatialRuntime {
    timeline: SceneTimeline,
    renderer: Vbap3dRenderer,
    scratch: RendererScratch,
    room: RoomTransform,
    listener: Listener,
    output_speakers: Vec<Speaker>,
    geometry_speakers: Vec<Speaker>,
    geometry_to_output: Vec<usize>,
    max_objects: usize,
    object_slots: BTreeMap<String, usize>,
    render_objects: Vec<RenderObject>,
    speaker_gains: Vec<SpeakerGain>,
    property_state: BTreeMap<String, SpatialRenderingProperties>,
}

impl NativeV2SpatialRuntime {
    pub fn new(config: SpatialRuntimeConfig) -> Result<Self, NativeV2SpatialRuntimeError> {
        if config.sample_rate == 0 || config.renderer_block_size == 0 || config.max_objects == 0 {
            return Err(NativeV2SpatialRuntimeError::InvalidConfiguration(
                "sample rate, renderer block size and max objects must be non-zero".into(),
            ));
        }

        let output_speakers = config
            .speakers
            .iter()
            .filter(|speaker| speaker.enabled)
            .cloned()
            .collect::<Vec<_>>();
        if output_speakers.is_empty() {
            return Err(NativeV2SpatialRuntimeError::InvalidConfiguration(
                "speaker layout has no enabled outputs".into(),
            ));
        }
        ensure_unique_output_roles(&output_speakers)?;

        // LFE is a discrete bed destination, never a VBAP geometry vertex for
        // independently renderable objects.
        let geometry_to_output = output_speakers
            .iter()
            .enumerate()
            .filter_map(|(index, speaker)| {
                (speaker.channel_role != ChannelRole::LowFrequencyEffects).then_some(index)
            })
            .collect::<Vec<_>>();
        if geometry_to_output.len() < 3 {
            return Err(NativeV2SpatialRuntimeError::InvalidConfiguration(
                "native V2 3D rendering requires at least three non-LFE speakers".into(),
            ));
        }
        let mut geometry_speakers = geometry_to_output
            .iter()
            .map(|index| output_speakers[*index].clone())
            .collect::<Vec<_>>();
        for speaker in &mut geometry_speakers {
            speaker.gain_db = 0.0;
            speaker.delay_samples = 0.0;
        }

        let mut renderer = Vbap3dRenderer::new().with_smoothing(1.0);
        renderer.configure(
            geometry_speakers.clone(),
            config.sample_rate,
            config.renderer_block_size,
            config.max_objects,
        )?;
        renderer.prepare_listener(&config.listener)?;
        let scratch = RendererScratch::new(renderer.required_scratch_size()?);
        let geometry_count = renderer.output_channel_count();
        let speaker_gains = vec![SpeakerGain::default(); config.max_objects * geometry_count];
        let silent_position = listener_center(config.listener);
        let render_objects = vec![
            RenderObject {
                position: silent_position,
                gain: 0.0,
            };
            config.max_objects
        ];

        Ok(Self {
            timeline: SceneTimeline::new(),
            renderer,
            scratch,
            room: config.room,
            listener: config.listener,
            output_speakers,
            geometry_speakers,
            geometry_to_output,
            max_objects: config.max_objects,
            object_slots: BTreeMap::new(),
            render_objects,
            speaker_gains,
            property_state: BTreeMap::new(),
        })
    }

    pub fn output_speakers(&self) -> &[Speaker] {
        &self.output_speakers
    }

    pub fn output_channel_count(&self) -> usize {
        self.output_speakers.len()
    }

    pub fn reset(&mut self) {
        self.timeline.reset();
        self.renderer.reset();
        self.object_slots.clear();
        self.property_state.clear();
        self.silence_render_slots();
    }

    pub fn render_frame(
        &mut self,
        frame: &SpatialDecodedFrameV2,
    ) -> Result<AudioBlock, NativeV2SpatialRuntimeError> {
        frame
            .validate()
            .map_err(|error| NativeV2SpatialRuntimeError::InvalidV2(error.to_string()))?;

        if frame.decoded.audio.discontinuity {
            self.reset();
        }
        self.reserve_signal_slots(frame)?;

        // SceneTimeline remains the proven sample-accurate XYZ/gain/ramp
        // planner. Rich V2 properties are consumed in parallel below, so this
        // structural V1 view is internal and never loses metadata externally.
        let structural = structural_v1(frame)?;
        let plan = self.timeline.plan_frame(&structural, self.room)?;

        let mut output = AudioBlock {
            channels: vec![vec![0.0; frame.decoded.audio.frame_count]; self.output_channel_count()],
            frame_count: frame.decoded.audio.frame_count,
            presentation_time_seconds: frame.decoded.audio.presentation_time_seconds,
            discontinuity: frame.decoded.audio.discontinuity,
        };
        self.mix_bed(frame, &plan, &mut output)?;
        self.mix_objects_exact(frame, &plan, &mut output)?;
        Ok(output)
    }

    fn reserve_signal_slots(
        &mut self,
        frame: &SpatialDecodedFrameV2,
    ) -> Result<(), NativeV2SpatialRuntimeError> {
        for signal in &frame.spatial.object_signals {
            if self.object_slots.contains_key(&signal.id) {
                continue;
            }
            if self.object_slots.len() >= self.max_objects {
                return Err(NativeV2SpatialRuntimeError::ObjectCapacityExceeded {
                    maximum: self.max_objects,
                    id: signal.id.clone(),
                });
            }
            let slot = first_free_slot(self.max_objects, self.object_slots.values().copied())
                .ok_or_else(|| NativeV2SpatialRuntimeError::ObjectCapacityExceeded {
                    maximum: self.max_objects,
                    id: signal.id.clone(),
                })?;
            self.object_slots.insert(signal.id.clone(), slot);
        }
        Ok(())
    }

    fn mix_bed(
        &self,
        frame: &SpatialDecodedFrameV2,
        plan: &SpatialRenderPlan,
        output: &mut AudioBlock,
    ) -> Result<(), NativeV2SpatialRuntimeError> {
        for bed in &plan.bed_signals {
            let output_index = self
                .output_speakers
                .iter()
                .position(|speaker| speaker.channel_role == bed.role)
                .ok_or_else(|| NativeV2SpatialRuntimeError::MissingBedOutput {
                    role: bed.role.to_string(),
                })?;
            let input = frame
                .decoded
                .audio
                .channels
                .get(bed.pcm_channel_index)
                .ok_or(NativeV2SpatialRuntimeError::PcmLaneOutOfRange {
                    lane: bed.pcm_channel_index,
                })?;
            for (destination, source) in output.channels[output_index].iter_mut().zip(input.iter()) {
                *destination += *source;
            }
        }
        Ok(())
    }

    fn mix_objects_exact(
        &mut self,
        frame: &SpatialDecodedFrameV2,
        plan: &SpatialRenderPlan,
        output: &mut AudioBlock,
    ) -> Result<(), NativeV2SpatialRuntimeError> {
        let geometry_count = self.geometry_speakers.len();
        let mut updates = frame.spatial.object_updates.iter().collect::<Vec<_>>();
        updates.sort_by_key(|update| update.metadata_sample_offset);
        let mut update_index = 0usize;

        for span in &plan.spans {
            for sample_offset in span.start_sample_offset..span.end_sample_offset {
                while let Some(update) = updates.get(update_index) {
                    if update.metadata_sample_offset != sample_offset {
                        break;
                    }
                    self.apply_property_update(update)?;
                    update_index += 1;
                }

                self.silence_render_slots();
                let absolute_sample = plan.absolute_start_sample + u64::from(sample_offset);

                for curve in &span.objects {
                    let slot = self
                        .object_slots
                        .get(&curve.object_id)
                        .copied()
                        .ok_or_else(|| NativeV2SpatialRuntimeError::UnknownObjectSlot {
                            id: curve.object_id.clone(),
                        })?;
                    let state = curve.state_at_absolute_sample(absolute_sample);
                    let properties = self
                        .property_state
                        .get(&curve.object_id)
                        .copied()
                        .unwrap_or_default();
                    ensure_supported_properties(&curve.object_id, properties)?;
                    let position = apply_position_properties(
                        state.position_meters,
                        self.listener,
                        properties,
                        &curve.object_id,
                    )?;
                    self.render_objects[slot] = RenderObject {
                        position,
                        gain: db_to_linear(state.gain_db),
                    };
                }

                self.renderer.render_gains(
                    &self.listener,
                    &self.render_objects,
                    &mut self.speaker_gains,
                    &mut self.scratch,
                )?;

                let sample_index = sample_offset as usize;
                for curve in &span.objects {
                    let slot = self.object_slots[&curve.object_id];
                    let source = frame
                        .decoded
                        .audio
                        .channels
                        .get(curve.pcm_channel_index)
                        .and_then(|channel| channel.get(sample_index))
                        .copied()
                        .ok_or(NativeV2SpatialRuntimeError::PcmLaneOutOfRange {
                            lane: curve.pcm_channel_index,
                        })?;
                    let properties = self
                        .property_state
                        .get(&curve.object_id)
                        .copied()
                        .unwrap_or_default();
                    let object_gain = self.render_objects[slot].gain;
                    let position = self.render_objects[slot].position;
                    let gains = &mut self.speaker_gains
                        [slot * geometry_count..(slot + 1) * geometry_count];
                    apply_speaker_constraints(
                        gains,
                        &self.geometry_speakers,
                        position,
                        object_gain,
                        properties,
                        &curve.object_id,
                    )?;
                    for (geometry_index, gain) in gains.iter().enumerate() {
                        let output_index = self.geometry_to_output[geometry_index];
                        output.channels[output_index][sample_index] += source * gain.gain;
                    }
                }
            }
        }

        // Metadata at exactly frame_count takes effect for the following frame.
        while let Some(update) = updates.get(update_index) {
            if update.metadata_sample_offset != frame.decoded.audio.frame_count as u32 {
                return Err(NativeV2SpatialRuntimeError::PropertyTimelineMismatch {
                    object_id: update.object_id.clone(),
                });
            }
            self.apply_property_update(update)?;
            update_index += 1;
        }
        Ok(())
    }

    fn apply_property_update(
        &mut self,
        update: &SpatialObjectUpdate,
    ) -> Result<(), NativeV2SpatialRuntimeError> {
        if update.ramp_duration_samples > 0 {
            if let Some(previous) = self.property_state.get(&update.object_id) {
                if previous.distance != update.rendering.distance {
                    return Err(NativeV2SpatialRuntimeError::UnsupportedRichPropertyRamp {
                        object_id: update.object_id.clone(),
                        property: "distance",
                    });
                }
            }
        }
        self.property_state
            .insert(update.object_id.clone(), update.rendering);
        Ok(())
    }

    fn silence_render_slots(&mut self) {
        let position = listener_center(self.listener);
        for object in &mut self.render_objects {
            object.position = position;
            object.gain = 0.0;
        }
    }
}

fn structural_v1(
    frame: &SpatialDecodedFrameV2,
) -> Result<SpatialDecodedFrameV1, NativeV2SpatialRuntimeError> {
    let structural = SpatialDecodedFrameV1 {
        decoded: frame.decoded.clone(),
        spatial: SpatialFrameMetadataV1 {
            domain: frame.spatial.domain,
            bed_signals: frame
                .spatial
                .bed_signals
                .iter()
                .map(|bed| BedSignalBindingV1 {
                    pcm_channel_index: bed.pcm_channel_index,
                    role: bed.role.clone(),
                })
                .collect(),
            object_signals: frame
                .spatial
                .object_signals
                .iter()
                .map(|object| ObjectSignalBindingV1 {
                    id: object.id.clone(),
                    pcm_channel_index: object.pcm_channel_index,
                })
                .collect(),
            object_updates: frame
                .spatial
                .object_updates
                .iter()
                .map(|update| SpatialObjectUpdateV1 {
                    object_id: update.object_id.clone(),
                    active: update.active,
                    coordinate_space: update.coordinate_space,
                    position: update.position,
                    gain_db: update.gain_db,
                    spread: update.spread,
                    metadata_sample_offset: update.metadata_sample_offset,
                    ramp_duration_samples: update.ramp_duration_samples,
                    priority: update.priority,
                })
                .collect(),
        },
    };
    structural.validate().map_err(|error| {
        NativeV2SpatialRuntimeError::InvalidStructuralProjection(error.to_string())
    })?;
    Ok(structural)
}

fn ensure_supported_properties(
    object_id: &str,
    properties: SpatialRenderingProperties,
) -> Result<(), NativeV2SpatialRuntimeError> {
    if properties.extent.width > EPSILON
        || properties.extent.depth > EPSILON
        || properties.extent.height > EPSILON
    {
        return Err(NativeV2SpatialRuntimeError::ExtentRequiresMultiSourceRenderer {
            object_id: object_id.to_owned(),
        });
    }
    if properties.screen_reference.is_some() {
        return Err(NativeV2SpatialRuntimeError::ScreenReferenceNotImplemented {
            object_id: object_id.to_owned(),
        });
    }
    if properties.trim_bypass == Some(true) {
        return Err(NativeV2SpatialRuntimeError::TrimBypassRequiresCalibrationBridge {
            object_id: object_id.to_owned(),
        });
    }
    Ok(())
}

fn apply_position_properties(
    mut position: Vector3,
    listener: Listener,
    properties: SpatialRenderingProperties,
    object_id: &str,
) -> Result<Vector3, NativeV2SpatialRuntimeError> {
    let center = listener_center(listener);
    if !properties.elevation_enabled {
        position.z = center.z;
    }

    match properties.distance {
        ObjectDistance::Unspecified => {}
        ObjectDistance::Meters(distance) => {
            position = project_to_distance(position, center, distance, object_id)?;
        }
        ObjectDistance::Infinity => {
            position = project_to_distance(
                position,
                center,
                INFINITY_RENDER_DISTANCE_METERS,
                object_id,
            )?;
        }
    }
    Ok(position)
}

fn project_to_distance(
    position: Vector3,
    center: Vector3,
    distance: f32,
    object_id: &str,
) -> Result<Vector3, NativeV2SpatialRuntimeError> {
    if distance == 0.0 {
        return Ok(center);
    }
    let direction = position - center;
    let length = direction.length();
    if length <= EPSILON {
        return Err(NativeV2SpatialRuntimeError::DistanceHasNoDirection {
            object_id: object_id.to_owned(),
        });
    }
    let scale = distance / length;
    Ok(Vector3::new(
        center.x + direction.x * scale,
        center.y + direction.y * scale,
        center.z + direction.z * scale,
    ))
}

fn apply_speaker_constraints(
    gains: &mut [SpeakerGain],
    speakers: &[Speaker],
    position: Vector3,
    object_gain: f32,
    properties: SpatialRenderingProperties,
    object_id: &str,
) -> Result<(), NativeV2SpatialRuntimeError> {
    let zone = normalize_zone(properties.zone)?;
    let eligible = speakers
        .iter()
        .map(|speaker| speaker_is_eligible(speaker, zone, properties.elevation_enabled))
        .collect::<Vec<_>>();
    if !eligible.iter().any(|value| *value) {
        return Err(NativeV2SpatialRuntimeError::ZoneHasNoEligibleSpeaker {
            object_id: object_id.to_owned(),
            zone,
        });
    }

    if properties.snap {
        let mut best: Option<(usize, f32)> = None;
        for (index, speaker) in speakers.iter().enumerate() {
            if !eligible[index] {
                continue;
            }
            let distance = speaker.position.distance_to(position);
            if best.map(|(_, current)| distance < current).unwrap_or(true) {
                best = Some((index, distance));
            }
        }
        let selected = best
            .map(|(index, _)| index)
            .ok_or_else(|| NativeV2SpatialRuntimeError::ZoneHasNoEligibleSpeaker {
                object_id: object_id.to_owned(),
                zone,
            })?;
        for (index, gain) in gains.iter_mut().enumerate() {
            gain.gain = if index == selected { object_gain } else { 0.0 };
        }
        return Ok(());
    }

    let original_power = gains
        .iter()
        .map(|gain| gain.gain * gain.gain)
        .sum::<f32>();
    for (index, gain) in gains.iter_mut().enumerate() {
        if !eligible[index] {
            gain.gain = 0.0;
        }
    }
    let remaining_power = gains
        .iter()
        .map(|gain| gain.gain * gain.gain)
        .sum::<f32>();
    if original_power > EPSILON && remaining_power <= EPSILON {
        return Err(NativeV2SpatialRuntimeError::ZoneRemovedAllRenderedEnergy {
            object_id: object_id.to_owned(),
            zone,
        });
    }
    if remaining_power > EPSILON {
        let scale = (original_power / remaining_power).sqrt();
        for gain in gains {
            gain.gain *= scale;
        }
    }
    Ok(())
}

fn normalize_zone(zone: ZoneConstraint) -> Result<ZoneConstraint, NativeV2SpatialRuntimeError> {
    Ok(match zone {
        ZoneConstraint::CodecSpecific(0) => ZoneConstraint::All,
        ZoneConstraint::CodecSpecific(1) => ZoneConstraint::NoBack,
        ZoneConstraint::CodecSpecific(2) => ZoneConstraint::NoSides,
        ZoneConstraint::CodecSpecific(3) => ZoneConstraint::CenterBack,
        ZoneConstraint::CodecSpecific(4) => ZoneConstraint::ScreenOnly,
        ZoneConstraint::CodecSpecific(5) => ZoneConstraint::SurroundOnly,
        ZoneConstraint::CodecSpecific(value) => {
            return Err(NativeV2SpatialRuntimeError::UnknownZoneCode { value })
        }
        semantic => semantic,
    })
}

fn speaker_is_eligible(speaker: &Speaker, zone: ZoneConstraint, elevation_enabled: bool) -> bool {
    if !elevation_enabled && is_height_role(&speaker.channel_role) {
        return false;
    }
    match zone {
        ZoneConstraint::All => true,
        ZoneConstraint::NoBack => !is_back_role(&speaker.channel_role),
        ZoneConstraint::NoSides => !is_side_role(&speaker.channel_role),
        ZoneConstraint::CenterBack => is_back_role(&speaker.channel_role),
        ZoneConstraint::ScreenOnly => is_screen_role(&speaker.channel_role),
        ZoneConstraint::SurroundOnly => is_surround_role(&speaker.channel_role),
        ZoneConstraint::CodecSpecific(_) => false,
    }
}

fn is_height_role(role: &ChannelRole) -> bool {
    matches!(
        role,
        ChannelRole::TopFrontLeft
            | ChannelRole::TopFrontRight
            | ChannelRole::TopRearLeft
            | ChannelRole::TopRearRight
    ) || matches!(role, ChannelRole::Custom(value) if value.contains("top") || value.contains("height"))
}

fn is_back_role(role: &ChannelRole) -> bool {
    matches!(
        role,
        ChannelRole::SurroundBackLeft
            | ChannelRole::SurroundBackRight
            | ChannelRole::TopRearLeft
            | ChannelRole::TopRearRight
    ) || matches!(role, ChannelRole::Custom(value) if value.contains("back") || value.contains("rear"))
}

fn is_side_role(role: &ChannelRole) -> bool {
    matches!(role, ChannelRole::SurroundLeft | ChannelRole::SurroundRight)
        || matches!(role, ChannelRole::Custom(value) if value.contains("side"))
}

fn is_screen_role(role: &ChannelRole) -> bool {
    matches!(
        role,
        ChannelRole::FrontLeft | ChannelRole::FrontRight | ChannelRole::FrontCenter
    ) || matches!(role, ChannelRole::Custom(value) if value.contains("front-wide"))
}

fn is_surround_role(role: &ChannelRole) -> bool {
    matches!(
        role,
        ChannelRole::SurroundLeft
            | ChannelRole::SurroundRight
            | ChannelRole::SurroundBackLeft
            | ChannelRole::SurroundBackRight
    ) || matches!(role, ChannelRole::Custom(value) if value.contains("surround") || value.contains("back"))
}

fn ensure_unique_output_roles(
    speakers: &[Speaker],
) -> Result<(), NativeV2SpatialRuntimeError> {
    for (index, speaker) in speakers.iter().enumerate() {
        if speakers[..index]
            .iter()
            .any(|candidate| candidate.channel_role == speaker.channel_role)
        {
            return Err(NativeV2SpatialRuntimeError::DuplicateOutputRole {
                role: speaker.channel_role.to_string(),
            });
        }
    }
    Ok(())
}

fn first_free_slot(maximum: usize, used: impl Iterator<Item = usize>) -> Option<usize> {
    let mut occupied = vec![false; maximum];
    for slot in used {
        if let Some(entry) = occupied.get_mut(slot) {
            *entry = true;
        }
    }
    occupied.iter().position(|occupied| !*occupied)
}

fn db_to_linear(db: f32) -> f32 {
    if db == f32::NEG_INFINITY {
        0.0
    } else {
        10.0_f32.powf(db / 20.0)
    }
}

fn listener_center(listener: Listener) -> Vector3 {
    Vector3::new(
        listener.position.x,
        listener.position.y,
        listener.position.z + listener.ear_height,
    )
}

#[derive(Debug, Error)]
pub enum NativeV2SpatialRuntimeError {
    #[error("invalid native V2 spatial runtime configuration: {0}")]
    InvalidConfiguration(String),
    #[error("Spatial IR V2 validation failed: {0}")]
    InvalidV2(String),
    #[error("structural V2-to-V1 projection failed validation: {0}")]
    InvalidStructuralProjection(String),
    #[error(transparent)]
    Timeline(#[from] SceneTimelineError),
    #[error(transparent)]
    Renderer(#[from] RendererError),
    #[error("object '{id}' exceeds configured object capacity {maximum}")]
    ObjectCapacityExceeded { maximum: usize, id: String },
    #[error("object '{id}' has no stable runtime slot")]
    UnknownObjectSlot { id: String },
    #[error("decoded PCM lane {lane} is unavailable")]
    PcmLaneOutOfRange { lane: usize },
    #[error("bed role '{role}' has no enabled physical output")]
    MissingBedOutput { role: String },
    #[error("enabled speaker role '{role}' is duplicated")]
    DuplicateOutputRole { role: String },
    #[error("object '{object_id}' distance metadata has no directional position")]
    DistanceHasNoDirection { object_id: String },
    #[error("object '{object_id}' has an extent that requires the multi-source V2 renderer")]
    ExtentRequiresMultiSourceRenderer { object_id: String },
    #[error("object '{object_id}' uses screen-reference metadata that is not mapped yet")]
    ScreenReferenceNotImplemented { object_id: String },
    #[error("object '{object_id}' requests trim bypass; downstream calibration bridge is required")]
    TrimBypassRequiresCalibrationBridge { object_id: String },
    #[error("object '{object_id}' changes {property} during a ramp; exact rich-property interpolation is not admitted yet")]
    UnsupportedRichPropertyRamp {
        object_id: String,
        property: &'static str,
    },
    #[error("codec zone code {value} has no admitted Aurora semantic mapping")]
    UnknownZoneCode { value: u8 },
    #[error("object '{object_id}' zone {zone:?} has no eligible physical speaker")]
    ZoneHasNoEligibleSpeaker {
        object_id: String,
        zone: ZoneConstraint,
    },
    #[error("object '{object_id}' zone {zone:?} removed all VBAP energy")]
    ZoneRemovedAllRenderedEnergy {
        object_id: String,
        zone: ZoneConstraint,
    },
    #[error("rich-property timeline for object '{object_id}' does not align with the decoded frame")]
    PropertyTimelineMismatch { object_id: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::{AudioFormat, SampleType};
    use aurora_decoder_api::DecodedFrame;
    use aurora_spatial_ir_v2::{
        CoordinateSpace, ObjectSignalBinding, SpatialDomain, SpatialFrameMetadata,
        SpatialObjectUpdate, SpatialPosition,
    };

    fn speaker(id: &str, role: ChannelRole, position: Vector3) -> Speaker {
        Speaker {
            id: id.into(),
            label: id.into(),
            channel_role: role,
            position,
            orientation: Vector3::ZERO,
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        }
    }

    fn config() -> SpatialRuntimeConfig {
        SpatialRuntimeConfig {
            sample_rate: 48_000,
            renderer_block_size: 40,
            max_objects: 4,
            room: RoomTransform::from_dimensions(Vector3::new(4.0, 6.0, 3.0)).unwrap(),
            listener: Listener {
                position: Vector3::new(2.0, 3.0, 1.2),
                orientation: Vector3::new(0.0, 1.0, 0.0),
                ear_height: 0.0,
            },
            speakers: vec![
                speaker("fl", ChannelRole::FrontLeft, Vector3::new(0.5, 5.5, 1.2)),
                speaker("fr", ChannelRole::FrontRight, Vector3::new(3.5, 5.5, 1.2)),
                speaker("fc", ChannelRole::FrontCenter, Vector3::new(2.0, 5.8, 1.2)),
                speaker("sl", ChannelRole::SurroundLeft, Vector3::new(0.2, 2.0, 1.2)),
                speaker("sr", ChannelRole::SurroundRight, Vector3::new(3.8, 2.0, 1.2)),
                speaker("sbl", ChannelRole::SurroundBackLeft, Vector3::new(0.8, 0.4, 1.2)),
                speaker("sbr", ChannelRole::SurroundBackRight, Vector3::new(3.2, 0.4, 1.2)),
                speaker("tfl", ChannelRole::TopFrontLeft, Vector3::new(0.8, 5.0, 2.8)),
                speaker("tfr", ChannelRole::TopFrontRight, Vector3::new(3.2, 5.0, 2.8)),
                speaker("trl", ChannelRole::TopRearLeft, Vector3::new(0.8, 1.0, 2.8)),
                speaker("trr", ChannelRole::TopRearRight, Vector3::new(3.2, 1.0, 2.8)),
                speaker("lfe", ChannelRole::LowFrequencyEffects, Vector3::new(2.0, 5.0, 0.3)),
            ],
            quality: aurora_spatial_runtime::SpatialRenderQuality::ExactPerSample,
        }
    }

    fn object_frame(rendering: SpatialRenderingProperties) -> SpatialDecodedFrameV2 {
        SpatialDecodedFrameV2 {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels: vec![vec![1.0; 40]],
                    frame_count: 40,
                    presentation_time_seconds: 0.0,
                    discontinuity: true,
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
                        y: 0.9,
                        z: 0.0,
                    },
                    gain_db: 0.0,
                    spread: 0.0,
                    metadata_sample_offset: 0,
                    ramp_duration_samples: 0,
                    priority: Some(1.0),
                    rendering,
                }],
            },
        }
    }

    #[test]
    fn lfe_is_never_part_of_object_geometry() {
        let runtime = NativeV2SpatialRuntime::new(config()).unwrap();
        assert_eq!(runtime.output_speakers.len(), 12);
        assert_eq!(runtime.geometry_speakers.len(), 11);
        assert!(runtime
            .geometry_speakers
            .iter()
            .all(|speaker| speaker.channel_role != ChannelRole::LowFrequencyEffects));
    }

    #[test]
    fn truehd_zone_codes_map_to_semantic_constraints() {
        assert_eq!(normalize_zone(ZoneConstraint::CodecSpecific(1)).unwrap(), ZoneConstraint::NoBack);
        assert_eq!(normalize_zone(ZoneConstraint::CodecSpecific(5)).unwrap(), ZoneConstraint::SurroundOnly);
        assert!(normalize_zone(ZoneConstraint::CodecSpecific(6)).is_err());
    }

    #[test]
    fn elevation_disabled_removes_height_speakers_from_eligibility() {
        let runtime = NativeV2SpatialRuntime::new(config()).unwrap();
        let properties = SpatialRenderingProperties {
            elevation_enabled: false,
            ..SpatialRenderingProperties::default()
        };
        let eligible = runtime
            .geometry_speakers
            .iter()
            .map(|speaker| speaker_is_eligible(speaker, ZoneConstraint::All, properties.elevation_enabled))
            .collect::<Vec<_>>();
        for (speaker, allowed) in runtime.geometry_speakers.iter().zip(eligible) {
            if is_height_role(&speaker.channel_role) {
                assert!(!allowed);
            }
        }
    }

    #[test]
    fn default_v2_object_renders_without_touching_lfe_output() {
        let mut runtime = NativeV2SpatialRuntime::new(config()).unwrap();
        let output = runtime
            .render_frame(&object_frame(SpatialRenderingProperties::default()))
            .unwrap();
        let lfe_index = runtime
            .output_speakers
            .iter()
            .position(|speaker| speaker.channel_role == ChannelRole::LowFrequencyEffects)
            .unwrap();
        assert!(output.channels[lfe_index].iter().all(|sample| sample.abs() < 1.0e-7));
    }

    #[test]
    fn object_distance_reprojects_position_without_applying_fake_attenuation() {
        let listener = config().listener;
        let center = listener_center(listener);
        let position = Vector3::new(center.x, center.y + 4.0, center.z);
        let projected = project_to_distance(position, center, 2.5, "o0").unwrap();
        assert!((projected.distance_to(center) - 2.5).abs() < 1.0e-5);
    }

    #[test]
    fn screen_reference_still_fails_closed() {
        let mut properties = SpatialRenderingProperties::default();
        properties.screen_reference = Some(aurora_spatial_ir_v2::ScreenReference {
            factor: 0.5,
            depth_factor: 0.5,
        });
        assert!(matches!(
            ensure_supported_properties("o0", properties),
            Err(NativeV2SpatialRuntimeError::ScreenReferenceNotImplemented { .. })
        ));
    }

    #[test]
    fn config_format_assumption_stays_f32_compatible() {
        let format = AudioFormat {
            sample_rate: 48_000,
            channel_count: 12,
            sample_type: SampleType::F32,
            block_size: 40,
        };
        assert_eq!(format.sample_type, SampleType::F32);
    }
}
