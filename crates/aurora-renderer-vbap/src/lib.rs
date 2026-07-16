//! Deterministic two-dimensional vector-base amplitude panning.

use std::f32::consts::{PI, TAU};

use aurora_core::{Listener, Speaker, Vector3};
use aurora_renderer_api::{
    RenderObject, Renderer, RendererError, RendererScratch, RendererScratchSize, SpeakerGain,
};

const GEOMETRY_EPSILON: f32 = 1.0e-6;
const ANGLE_EPSILON: f32 = 1.0e-5;
const DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND: f32 = 343.0;

/// Allocation-free horizontal-plane VBAP renderer.
#[derive(Debug, Clone)]
pub struct VbapRenderer {
    layout: Vec<Speaker>,
    sample_rate: u32,
    block_size: usize,
    max_objects: usize,
    smoothing_alpha: f32,
    previous_gains: Vec<f32>,
    configured: bool,
}

impl Default for VbapRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl VbapRenderer {
    /// Creates a renderer with smoothing disabled.
    pub fn new() -> Self {
        Self {
            layout: Vec::new(),
            sample_rate: 48_000,
            block_size: 256,
            max_objects: 0,
            smoothing_alpha: 1.0,
            previous_gains: Vec::new(),
            configured: false,
        }
    }

    /// Sets block-to-block smoothing alpha, clamped to `0.0..=1.0`.
    pub fn with_smoothing(mut self, smoothing_alpha: f32) -> Self {
        self.smoothing_alpha = smoothing_alpha.clamp(0.0, 1.0);
        self
    }

    fn render_object(
        &mut self,
        listener: &Listener,
        object_index: usize,
        object: RenderObject,
        output: &mut [SpeakerGain],
        weights: &mut [f32],
    ) -> Result<(), RendererError> {
        vbap_weights(&self.layout, listener.position, object.position, weights);

        let speaker_count = self.layout.len();
        let history_len = self.previous_gains.len();
        for (speaker_index, ((speaker, weight), result)) in self
            .layout
            .iter()
            .zip(weights.iter())
            .zip(output.iter_mut())
            .enumerate()
        {
            let target = *weight * db_to_gain(speaker.gain_db) * object.gain;
            let history_index = object_index * speaker_count + speaker_index;
            let previous_gain = self.previous_gains.get_mut(history_index).ok_or(
                RendererError::OutputBufferSize {
                    required: self.max_objects * speaker_count,
                    actual: history_len,
                },
            )?;
            let smoothed = *previous_gain + (target - *previous_gain) * self.smoothing_alpha;
            *previous_gain = smoothed;
            let distance = object.position.distance_to(speaker.position);
            *result = SpeakerGain {
                speaker_index,
                gain: smoothed,
                distance_meters: distance,
                delay_samples: speaker.delay_samples
                    + distance / DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND * self.sample_rate as f32,
            };
        }
        Ok(())
    }
}

impl Renderer for VbapRenderer {
    fn configure(
        &mut self,
        layout: Vec<Speaker>,
        sample_rate: u32,
        block_size: usize,
        max_objects: usize,
    ) -> Result<(), RendererError> {
        if sample_rate == 0 || block_size == 0 || max_objects == 0 {
            return Err(RendererError::InvalidConfiguration(
                "sample rate, block size, and max objects must be greater than zero".to_owned(),
            ));
        }
        let enabled_count = layout.iter().filter(|speaker| speaker.enabled).count();
        if enabled_count == 0 {
            return Err(RendererError::NoEnabledSpeakers);
        }

        self.layout.clear();
        self.layout.reserve(enabled_count);
        self.layout
            .extend(layout.into_iter().filter(|speaker| speaker.enabled));
        self.sample_rate = sample_rate;
        self.block_size = block_size;
        self.max_objects = max_objects;
        self.previous_gains = vec![0.0; enabled_count * max_objects];
        self.configured = true;
        Ok(())
    }

    fn required_scratch_size(&self) -> Result<RendererScratchSize, RendererError> {
        if !self.configured {
            return Err(RendererError::NotConfigured);
        }
        Ok(RendererScratchSize {
            float_count: self.layout.len(),
        })
    }

    fn render_gains(
        &mut self,
        listener: &Listener,
        objects: &[RenderObject],
        output_gains: &mut [SpeakerGain],
        scratch: &mut RendererScratch,
    ) -> Result<(), RendererError> {
        if !self.configured {
            return Err(RendererError::NotConfigured);
        }
        if objects.len() > self.max_objects {
            return Err(RendererError::TooManyObjects {
                maximum: self.max_objects,
                actual: objects.len(),
            });
        }
        let required = objects.len() * self.layout.len();
        if output_gains.len() != required {
            return Err(RendererError::OutputBufferSize {
                required,
                actual: output_gains.len(),
            });
        }
        let available = scratch.floats_mut().len();
        if available < self.layout.len() {
            return Err(RendererError::ScratchBufferSize {
                required: self.layout.len(),
                actual: available,
            });
        }

        let speaker_count = self.layout.len();
        let weights = scratch.floats_mut().get_mut(..speaker_count).ok_or(
            RendererError::ScratchBufferSize {
                required: speaker_count,
                actual: available,
            },
        )?;
        for (object_index, (object, object_output)) in objects
            .iter()
            .copied()
            .zip(output_gains.chunks_exact_mut(speaker_count))
            .enumerate()
        {
            self.render_object(listener, object_index, object, object_output, weights)?;
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.previous_gains.fill(0.0);
    }

    fn latency_frames(&self) -> usize {
        let _ = self.block_size;
        0
    }

    fn output_channel_count(&self) -> usize {
        self.layout.len()
    }
}

fn vbap_weights(speakers: &[Speaker], listener: Vector3, source: Vector3, weights: &mut [f32]) {
    weights.fill(0.0);
    let source_x = source.x - listener.x;
    let source_y = source.y - listener.y;
    let source_length = source_x.hypot(source_y);
    if source_length <= GEOMETRY_EPSILON {
        equal_power(weights);
        return;
    }
    let source_angle = source_y.atan2(source_x);

    let mut best_pair = None::<(usize, usize, f32)>;
    for (first, first_speaker) in speakers.iter().enumerate() {
        let first_angle = speaker_angle(first_speaker, listener);
        let Some(first_angle) = first_angle else {
            continue;
        };
        for (second, second_speaker) in speakers.iter().enumerate().skip(first + 1) {
            let second_angle = speaker_angle(second_speaker, listener);
            let Some(second_angle) = second_angle else {
                continue;
            };
            let delta = positive_angle(second_angle - first_angle);
            let (left, right, left_angle, span) = if delta <= PI {
                (first, second, first_angle, delta)
            } else {
                (second, first, second_angle, TAU - delta)
            };
            if !(ANGLE_EPSILON..=(PI - ANGLE_EPSILON)).contains(&span) {
                continue;
            }
            let offset = positive_angle(source_angle - left_angle);
            if offset > span + ANGLE_EPSILON {
                continue;
            }

            let replace = match best_pair {
                None => true,
                Some((best_left, best_right, best_span)) => {
                    span < best_span - ANGLE_EPSILON
                        || ((span - best_span).abs() <= ANGLE_EPSILON
                            && (left.min(right), left.max(right))
                                < (best_left.min(best_right), best_left.max(best_right)))
                }
            };
            if replace {
                best_pair = Some((left, right, span));
            }
        }
    }

    if let Some((left, right, _)) = best_pair {
        if solve_pair(
            &speakers[left],
            &speakers[right],
            listener,
            source_x / source_length,
            source_y / source_length,
            left,
            right,
            weights,
        ) {
            return;
        }
    }

    nearest_direction(speakers, listener, source_x, source_y, weights);
}

#[allow(clippy::too_many_arguments)]
fn solve_pair(
    left_speaker: &Speaker,
    right_speaker: &Speaker,
    listener: Vector3,
    source_x: f32,
    source_y: f32,
    left: usize,
    right: usize,
    weights: &mut [f32],
) -> bool {
    let left_x = left_speaker.position.x - listener.x;
    let left_y = left_speaker.position.y - listener.y;
    let right_x = right_speaker.position.x - listener.x;
    let right_y = right_speaker.position.y - listener.y;
    let left_length = left_x.hypot(left_y);
    let right_length = right_x.hypot(right_y);
    if left_length <= GEOMETRY_EPSILON || right_length <= GEOMETRY_EPSILON {
        return false;
    }
    let lx = left_x / left_length;
    let ly = left_y / left_length;
    let rx = right_x / right_length;
    let ry = right_y / right_length;
    let determinant = lx * ry - rx * ly;
    if determinant.abs() <= GEOMETRY_EPSILON {
        return false;
    }
    let left_gain = ((source_x * ry - rx * source_y) / determinant).max(0.0);
    let right_gain = ((lx * source_y - source_x * ly) / determinant).max(0.0);
    let norm = left_gain.hypot(right_gain);
    if norm <= GEOMETRY_EPSILON || !norm.is_finite() {
        return false;
    }
    if let Some(weight) = weights.get_mut(left) {
        *weight = left_gain / norm;
    }
    if let Some(weight) = weights.get_mut(right) {
        *weight = right_gain / norm;
    }
    true
}

fn nearest_direction(
    speakers: &[Speaker],
    listener: Vector3,
    source_x: f32,
    source_y: f32,
    weights: &mut [f32],
) {
    let source_length = source_x.hypot(source_y);
    let mut nearest = None::<(usize, f32)>;
    for (index, speaker) in speakers.iter().enumerate() {
        let speaker_x = speaker.position.x - listener.x;
        let speaker_y = speaker.position.y - listener.y;
        let length = speaker_x.hypot(speaker_y);
        if length <= GEOMETRY_EPSILON {
            continue;
        }
        let dot = (speaker_x * source_x + speaker_y * source_y) / (length * source_length);
        if nearest.map_or(true, |(_, best_dot)| dot > best_dot) {
            nearest = Some((index, dot));
        }
    }
    if let Some((index, _)) = nearest {
        if let Some(weight) = weights.get_mut(index) {
            *weight = 1.0;
        }
    } else {
        equal_power(weights);
    }
}

fn equal_power(weights: &mut [f32]) {
    let gain = 1.0 / (weights.len() as f32).sqrt();
    weights.fill(gain);
}

fn speaker_angle(speaker: &Speaker, listener: Vector3) -> Option<f32> {
    let x = speaker.position.x - listener.x;
    let y = speaker.position.y - listener.y;
    (x.hypot(y) > GEOMETRY_EPSILON).then(|| y.atan2(x))
}

fn positive_angle(angle: f32) -> f32 {
    angle.rem_euclid(TAU)
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use aurora_core::ChannelRole;

    use super::*;

    fn speaker(id: &str, role: ChannelRole, x: f32, y: f32) -> Speaker {
        Speaker {
            id: id.to_owned(),
            label: id.to_owned(),
            channel_role: role,
            position: Vector3::new(x, y, 0.0),
            orientation: Vector3::ZERO,
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        }
    }

    fn listener() -> Listener {
        Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 1.2,
        }
    }

    fn renderer() -> (VbapRenderer, RendererScratch, Vec<SpeakerGain>) {
        let mut renderer = VbapRenderer::new();
        renderer
            .configure(
                vec![
                    speaker("left", ChannelRole::FrontLeft, -1.0, 1.0),
                    speaker("right", ChannelRole::FrontRight, 1.0, 1.0),
                ],
                48_000,
                256,
                1,
            )
            .unwrap();
        let scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        (renderer, scratch, vec![SpeakerGain::default(); 2])
    }

    fn render_at(x: f32, y: f32) -> Vec<SpeakerGain> {
        let (mut renderer, mut scratch, mut gains) = renderer();
        renderer
            .render_gains(
                &listener(),
                &[RenderObject {
                    position: Vector3::new(x, y, 0.0),
                    gain: 1.0,
                }],
                &mut gains,
                &mut scratch,
            )
            .unwrap();
        gains
    }

    #[test]
    fn center_direction_has_equal_power_stereo_gains() {
        let gains = render_at(0.0, 1.0);
        assert!((gains[0].gain - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
        assert!((gains[1].gain - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
    }

    #[test]
    fn exact_speaker_direction_routes_to_that_speaker() {
        let gains = render_at(-1.0, 1.0);
        assert!((gains[0].gain - 1.0).abs() < 0.0001);
        assert!(gains[1].gain.abs() < 0.0001);
    }

    #[test]
    fn source_at_listener_is_finite_and_power_normalized() {
        let gains = render_at(0.0, 0.0);
        let power = gains.iter().map(|gain| gain.gain.powi(2)).sum::<f32>();
        assert!(gains.iter().all(|gain| gain.gain.is_finite()));
        assert!((power - 1.0).abs() < 0.0001);
    }

    #[test]
    fn silent_object_produces_zero_gains() {
        let (mut renderer, mut scratch, mut gains) = renderer();
        renderer
            .render_gains(
                &listener(),
                &[RenderObject {
                    position: Vector3::new(0.0, 1.0, 0.0),
                    gain: 0.0,
                }],
                &mut gains,
                &mut scratch,
            )
            .unwrap();
        assert!(gains.iter().all(|gain| gain.gain == 0.0));
    }

    #[test]
    fn render_preserves_caller_owned_capacities() {
        let (mut renderer, mut scratch, mut gains) = renderer();
        let gain_capacity = gains.capacity();
        let scratch_capacity = scratch.float_capacity();
        for step in 0..1_000 {
            let angle = step as f32 * 0.01;
            renderer
                .render_gains(
                    &listener(),
                    &[RenderObject {
                        position: Vector3::new(angle.cos(), angle.sin(), 0.0),
                        gain: 1.0,
                    }],
                    &mut gains,
                    &mut scratch,
                )
                .unwrap();
        }
        assert_eq!(gains.capacity(), gain_capacity);
        assert_eq!(scratch.float_capacity(), scratch_capacity);
    }

    #[test]
    fn zero_enabled_speakers_returns_structured_error() {
        let mut disabled = speaker("left", ChannelRole::FrontLeft, -1.0, 1.0);
        disabled.enabled = false;
        let error = VbapRenderer::new()
            .configure(vec![disabled], 48_000, 256, 1)
            .unwrap_err();
        assert_eq!(error, RendererError::NoEnabledSpeakers);
    }

    #[test]
    fn output_is_deterministic() {
        assert_eq!(render_at(0.25, 0.75), render_at(0.25, 0.75));
    }
}
