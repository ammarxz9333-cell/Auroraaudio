//! Object-preserving spatial render runtime for Aurora.
//!
//! The runtime is deliberately codec-neutral. Decoder backends produce
//! `SpatialDecodedFrame`; `SceneTimeline` turns metadata into sample-addressed
//! trajectories; this crate renders those trajectories into a physical speaker
//! layout. Speaker calibration remains downstream DSP so bed and object signals
//! receive trims/delays/EQ exactly once.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use aurora_core::{AudioBlock, Listener, Speaker, Vector3};
use aurora_decoder_engine::scene_timeline::{
    RoomTransform, SceneTimeline, SceneTimelineError, SpatialRenderPlan,
};
use aurora_decoder_engine::spatial_ir::SpatialDecodedFrame;
use aurora_renderer_api::{
    RenderObject, Renderer, RendererError, RendererScratch, SpeakerGain,
};
use aurora_renderer_vbap::Vbap3dRenderer;
use thiserror::Error;

/// Correctness-first render policy.
///
/// `ExactPerSample` evaluates the spatial renderer at every PCM sample during
/// object motion. This is the conformance/reference path. A measured adaptive
/// sub-block policy can be added later without weakening this baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpatialRenderQuality {
    #[default]
    ExactPerSample,
}

#[derive(Debug, Clone)]
pub struct SpatialRuntimeConfig {
    pub sample_rate: u32,
    pub renderer_block_size: usize,
    pub max_objects: usize,
    pub room: RoomTransform,
    pub listener: Listener,
    pub speakers: Vec<Speaker>,
    pub quality: SpatialRenderQuality,
}

/// Codec-neutral Aurora spatial runtime using the validated 3D VBAP backend.
pub struct VbapSpatialRuntime {
    timeline: SceneTimeline,
    renderer: Vbap3dRenderer,
    scratch: RendererScratch,
    room: RoomTransform,
    listener: Listener,
    output_speakers: Vec<Speaker>,
    max_objects: usize,
    object_slots: BTreeMap<String, usize>,
    render_objects: Vec<RenderObject>,
    speaker_gains: Vec<SpeakerGain>,
    quality: SpatialRenderQuality,
}

impl VbapSpatialRuntime {
    pub fn new(config: SpatialRuntimeConfig) -> Result<Self, SpatialRuntimeError> {
        if config.sample_rate == 0 || config.renderer_block_size == 0 || config.max_objects == 0 {
            return Err(SpatialRuntimeError::InvalidConfiguration(
                "sample rate, renderer block size, and max objects must be non-zero".into(),
            ));
        }

        let output_speakers = config
            .speakers
            .iter()
            .filter(|speaker| speaker.enabled)
            .cloned()
            .collect::<Vec<_>>();
        if output_speakers.is_empty() {
            return Err(SpatialRuntimeError::InvalidConfiguration(
                "speaker layout has no enabled outputs".into(),
            ));
        }
        ensure_unique_output_roles(&output_speakers)?;

        // The renderer is geometry-only in this runtime. Speaker trims and
        // delays are intentionally cleared here because Aurora DSP applies
        // physical-output calibration after bed+object summation.
        let mut geometry_layout = output_speakers.clone();
        for speaker in &mut geometry_layout {
            speaker.gain_db = 0.0;
            speaker.delay_samples = 0.0;
        }

        let mut renderer = Vbap3dRenderer::new().with_smoothing(1.0);
        renderer.configure(
            geometry_layout,
            config.sample_rate,
            config.renderer_block_size,
            config.max_objects,
        )?;
        renderer.prepare_listener(&config.listener)?;
        let scratch = RendererScratch::new(renderer.required_scratch_size()?);
        let output_count = renderer.output_channel_count();
        let speaker_gains = vec![SpeakerGain::default(); config.max_objects * output_count];
        let silent_position = acoustic_listener_center(config.listener);
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
            max_objects: config.max_objects,
            object_slots: BTreeMap::new(),
            render_objects,
            speaker_gains,
            quality: config.quality,
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
        self.silence_render_slots();
    }

    /// Render one object-preserving decoded frame into planar speaker PCM.
    ///
    /// Output remains pre-calibration. Downstream DSP owns speaker EQ, trim,
    /// static delay, bass management and limiting.
    pub fn render_frame(
        &mut self,
        frame: &SpatialDecodedFrame,
    ) -> Result<AudioBlock, SpatialRuntimeError> {
        if frame.decoded.audio.discontinuity {
            self.renderer.reset();
            self.object_slots.clear();
            self.silence_render_slots();
        }
        self.reserve_signal_slots(frame)?;
        let plan = self.timeline.plan_frame(frame, self.room)?;

        let mut output = AudioBlock {
            channels: vec![vec![0.0; frame.decoded.audio.frame_count]; self.output_channel_count()],
            frame_count: frame.decoded.audio.frame_count,
            presentation_time_seconds: frame.decoded.audio.presentation_time_seconds,
            discontinuity: frame.decoded.audio.discontinuity,
        };
        self.mix_bed(frame, &plan, &mut output)?;
        match self.quality {
            SpatialRenderQuality::ExactPerSample => {
                self.mix_objects_exact(frame, &plan, &mut output)?;
            }
        }
        Ok(output)
    }

    fn reserve_signal_slots(&mut self, frame: &SpatialDecodedFrame) -> Result<(), SpatialRuntimeError> {
        for signal in &frame.spatial.object_signals {
            if self.object_slots.contains_key(&signal.id) {
                continue;
            }
            if self.object_slots.len() >= self.max_objects {
                return Err(SpatialRuntimeError::ObjectCapacityExceeded {
                    maximum: self.max_objects,
                    id: signal.id.clone(),
                });
            }
            let slot = first_free_slot(self.max_objects, self.object_slots.values().copied())
                .ok_or_else(|| SpatialRuntimeError::ObjectCapacityExceeded {
                    maximum: self.max_objects,
                    id: signal.id.clone(),
                })?;
            self.object_slots.insert(signal.id.clone(), slot);
        }
        Ok(())
    }

    fn mix_bed(
        &self,
        frame: &SpatialDecodedFrame,
        plan: &SpatialRenderPlan,
        output: &mut AudioBlock,
    ) -> Result<(), SpatialRuntimeError> {
        for bed in &plan.bed_signals {
            let output_index = self
                .output_speakers
                .iter()
                .position(|speaker| speaker.channel_role == bed.role)
                .ok_or_else(|| SpatialRuntimeError::MissingBedOutput {
                    role: bed.role.to_string(),
                })?;
            let input = frame
                .decoded
                .audio
                .channels
                .get(bed.pcm_channel_index)
                .ok_or(SpatialRuntimeError::PcmLaneOutOfRange {
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
        frame: &SpatialDecodedFrame,
        plan: &SpatialRenderPlan,
        output: &mut AudioBlock,
    ) -> Result<(), SpatialRuntimeError> {
        let speaker_count = self.output_channel_count();
        for span in &plan.spans {
            for sample_offset in span.start_sample_offset..span.end_sample_offset {
                self.silence_render_slots();
                let absolute_sample = plan.absolute_start_sample + u64::from(sample_offset);

                for curve in &span.objects {
                    let slot = self
                        .object_slots
                        .get(&curve.object_id)
                        .copied()
                        .ok_or_else(|| SpatialRuntimeError::UnknownObjectSlot {
                            id: curve.object_id.clone(),
                        })?;
                    let state = curve.state_at_absolute_sample(absolute_sample);
                    if state.spread > 1.0e-6 {
                        return Err(SpatialRuntimeError::UnsupportedSpread {
                            id: curve.object_id.clone(),
                            spread: state.spread,
                        });
                    }
                    self.render_objects[slot] = RenderObject {
                        position: state.position_meters,
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
                        .ok_or(SpatialRuntimeError::PcmLaneOutOfRange {
                            lane: curve.pcm_channel_index,
                        })?;
                    let gains = &self.speaker_gains
                        [slot * speaker_count..(slot + 1) * speaker_count];
                    for (destination, gain) in output.channels.iter_mut().zip(gains.iter()) {
                        destination[sample_index] += source * gain.gain;
                    }
                }
            }
        }
        Ok(())
    }

    fn silence_render_slots(&mut self) {
        let position = acoustic_listener_center(self.listener);
        for object in &mut self.render_objects {
            object.position = position;
            object.gain = 0.0;
        }
    }
}

#[derive(Debug, Error)]
pub enum SpatialRuntimeError {
    #[error("invalid spatial runtime configuration: {0}")]
    InvalidConfiguration(String),
    #[error(transparent)]
    Timeline(#[from] SceneTimelineError),
    #[error(transparent)]
    Renderer(#[from] RendererError),
    #[error("object '{id}' exceeds configured spatial object capacity {maximum}")]
    ObjectCapacityExceeded { maximum: usize, id: String },
    #[error("render plan references object '{id}' without a stable runtime slot")]
    UnknownObjectSlot { id: String },
    #[error("decoded PCM lane {lane} is unavailable")]
    PcmLaneOutOfRange { lane: usize },
    #[error("bed role '{role}' has no enabled physical output")]
    MissingBedOutput { role: String },
    #[error("enabled speaker role '{role}' is duplicated; direct bed routing would be ambiguous")]
    DuplicateOutputRole { role: String },
    #[error("object '{id}' requests spread {spread}; exact spread rendering is not implemented")]
    UnsupportedSpread { id: String, spread: f32 },
}

fn ensure_unique_output_roles(speakers: &[Speaker]) -> Result<(), SpatialRuntimeError> {
    for (index, speaker) in speakers.iter().enumerate() {
        if speakers[..index]
            .iter()
            .any(|candidate| candidate.channel_role == speaker.channel_role)
        {
            return Err(SpatialRuntimeError::DuplicateOutputRole {
                role: speaker.channel_role.to_string(),
            });
        }
    }
    Ok(())
}

fn first_free_slot(
    maximum: usize,
    used: impl Iterator<Item = usize>,
) -> Option<usize> {
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

fn acoustic_listener_center(listener: Listener) -> Vector3 {
    Vector3::new(
        listener.position.x,
        listener.position.y,
        listener.position.z + listener.ear_height,
    )
}

#[cfg(test)]
mod tests {
    use aurora_core::{ChannelRole, SampleType};
    use aurora_decoder_api::DecodedFrame;
    use aurora_decoder_engine::spatial_ir::{
        CoordinateSpace, ObjectSignalBinding, SpatialDomain, SpatialFrameMetadata,
        SpatialObjectUpdate, SpatialPosition,
    };

    use super::*;

    fn speaker(id: &str, role: ChannelRole, position: Vector3) -> Speaker {
        Speaker {
            id: id.into(),
            label: id.into(),
            channel_role: role,
            position,
            orientation: Vector3::ZERO,
            gain_db: -9.0,
            delay_samples: 17.0,
            enabled: true,
        }
    }

    fn config() -> SpatialRuntimeConfig {
        let room = RoomTransform::from_dimensions(Vector3::new(4.0, 4.0, 3.0)).unwrap();
        SpatialRuntimeConfig {
            sample_rate: 48_000,
            renderer_block_size: 40,
            max_objects: 4,
            room,
            listener: Listener {
                position: Vector3::ZERO,
                orientation: Vector3::new(0.0, 1.0, 0.0),
                ear_height: 0.0,
            },
            speakers: vec![
                speaker("x", ChannelRole::FrontRight, Vector3::new(1.0, 0.0, 0.0)),
                speaker("y", ChannelRole::FrontCenter, Vector3::new(0.0, 1.0, 0.0)),
                speaker("z", ChannelRole::TopFrontLeft, Vector3::new(0.0, 0.0, 1.0)),
                speaker("lfe", ChannelRole::LowFrequencyEffects, Vector3::new(0.0, -1.0, 0.0)),
            ],
            quality: SpatialRenderQuality::ExactPerSample,
        }
    }

    fn object_frame(position: Vector3, frames: usize) -> SpatialDecodedFrame {
        SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels: vec![vec![1.0; frames]],
                    frame_count: frames,
                    presentation_time_seconds: 0.0,
                    discontinuity: true,
                },
                objects: Vec::new(),
            },
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                object_signals: vec![ObjectSignalBinding {
                    id: "object-0".into(),
                    pcm_channel_index: 0,
                }],
                object_updates: vec![SpatialObjectUpdate {
                    object_id: "object-0".into(),
                    active: true,
                    coordinate_space: CoordinateSpace::AuroraMeters,
                    position: SpatialPosition::Cartesian {
                        x: position.x,
                        y: position.y,
                        z: position.z,
                    },
                    gain_db: 0.0,
                    spread: 0.0,
                    metadata_sample_offset: 0,
                    ramp_duration_samples: 0,
                    priority: Some(1.0),
                }],
            },
        }
    }

    #[test]
    fn exact_object_on_axis_routes_to_matching_speaker_without_calibration_trim() {
        let mut runtime = VbapSpatialRuntime::new(config()).unwrap();
        let output = runtime
            .render_frame(&object_frame(Vector3::new(1.0, 0.0, 0.0), 4))
            .unwrap();
        assert!(output.channels[0].iter().all(|sample| (*sample - 1.0).abs() < 1.0e-5));
        assert!(output.channels[1].iter().all(|sample| sample.abs() < 1.0e-5));
        assert!(output.channels[2].iter().all(|sample| sample.abs() < 1.0e-5));
        assert!(output.channels[3].iter().all(|sample| sample.abs() < 1.0e-5));
    }

    #[test]
    fn runtime_keeps_pcm_format_planar_f32_contract() {
        let format = aurora_core::AudioFormat {
            sample_rate: 48_000,
            channel_count: 4,
            sample_type: SampleType::F32,
            block_size: 40,
        };
        let runtime = VbapSpatialRuntime::new(config()).unwrap();
        assert_eq!(runtime.output_channel_count(), format.channel_count);
    }
}
