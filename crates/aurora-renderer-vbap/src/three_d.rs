//! Experimental three-dimensional vector-base amplitude panning.
//!
//! This module is the first offline height-capable loudspeaker-rendering slice
//! for issue #38. It keeps LFE out of spatial panning, validates non-degenerate
//! loudspeaker triplets during configuration, normalizes spatial gain power,
//! and uses a deterministic nearest-direction fallback when no configured
//! triplet contains the requested source direction.

use std::cmp::Ordering;

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{
    RenderObject, Renderer, RendererError, RendererScratch, RendererScratchSize, SpeakerGain,
};

const GEOMETRY_EPSILON: f64 = 1.0e-8;
const GAIN_EPSILON: f64 = 1.0e-7;
const DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND: f64 = 343.0;
const MAX_SPATIAL_SPEAKERS: usize = 32;

/// One non-degenerate speaker triplet accepted during renderer configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedTriplet {
    speaker_indices: [usize; 3],
    speaker_ids: [String; 3],
    determinant_abs: f64,
    span_radians: f64,
}

impl ValidatedTriplet {
    /// Returns configured output indices participating in this triplet.
    pub const fn speaker_indices(&self) -> [usize; 3] {
        self.speaker_indices
    }

    /// Returns a stable, lexicographically sorted speaker-ID key.
    pub fn speaker_ids(&self) -> &[String; 3] {
        &self.speaker_ids
    }

    /// Returns the absolute configured-direction determinant.
    pub const fn determinant_abs(&self) -> f64 {
        self.determinant_abs
    }

    /// Returns the largest pairwise angular separation in this triplet.
    pub const fn span_radians(&self) -> f64 {
        self.span_radians
    }
}

/// Allocation-free steady-state 3D VBAP renderer.
///
/// Configuration builds and deterministically orders all non-degenerate
/// non-LFE triplets. At render time the current listener position is applied to
/// speaker directions, so a triplet that becomes degenerate for a moved
/// listener is skipped rather than producing non-finite gains.
#[derive(Debug, Clone)]
pub struct Vbap3dRenderer {
    layout: Vec<Speaker>,
    spatial_indices: Vec<usize>,
    triplets: Vec<ValidatedTriplet>,
    sample_rate: u32,
    block_size: usize,
    max_objects: usize,
    smoothing_alpha: f32,
    previous_gains: Vec<f32>,
    configured: bool,
}

impl Default for Vbap3dRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Vbap3dRenderer {
    /// Creates an unconfigured renderer with block smoothing disabled.
    pub fn new() -> Self {
        Self {
            layout: Vec::new(),
            spatial_indices: Vec::new(),
            triplets: Vec::new(),
            sample_rate: 48_000,
            block_size: 256,
            max_objects: 0,
            smoothing_alpha: 1.0,
            previous_gains: Vec::new(),
            configured: false,
        }
    }

    /// Sets block-to-block smoothing alpha, clamped to `0.0..=1.0`.
    ///
    /// A non-finite value selects the unsmoothed default `1.0`.
    pub fn with_smoothing(mut self, smoothing_alpha: f32) -> Self {
        self.smoothing_alpha = if smoothing_alpha.is_finite() {
            smoothing_alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self
    }

    /// Returns the validated triplets in deterministic preference order.
    pub fn validated_triplets(&self) -> &[ValidatedTriplet] {
        &self.triplets
    }

    fn render_object(
        &mut self,
        listener: &Listener,
        object_index: usize,
        object: RenderObject,
        directions: &[f32],
        weights: &mut [f32],
        output: &mut [SpeakerGain],
    ) -> Result<(), RendererError> {
        weights.fill(0.0);
        let runtime_input_is_finite = vector_is_finite(listener.position)
            && vector_is_finite(object.position)
            && object.gain.is_finite();

        if runtime_input_is_finite {
            let source_delta = subtract(object.position, listener.position);
            let source_length = length(source_delta);
            if source_length <= GEOMETRY_EPSILON {
                equal_power_spatial(weights, &self.spatial_indices);
            } else {
                let source_direction = scale(source_delta, 1.0 / source_length);
                if !self.select_triplet(source_direction, directions, weights) {
                    nearest_direction(
                        &self.layout,
                        &self.spatial_indices,
                        source_direction,
                        directions,
                        weights,
                    );
                }
            }
        }

        let speaker_count = self.layout.len();
        let history_len = self.previous_gains.len();
        for (speaker_index, ((speaker, weight), result)) in self
            .layout
            .iter()
            .zip(weights.iter())
            .zip(output.iter_mut())
            .enumerate()
        {
            let target = if runtime_input_is_finite {
                finite_f32(*weight as f64 * db_to_gain(speaker.gain_db) * object.gain as f64)
            } else {
                0.0
            };
            let history_index = object_index * speaker_count + speaker_index;
            let previous_gain = self.previous_gains.get_mut(history_index).ok_or(
                RendererError::OutputBufferSize {
                    required: self.max_objects * speaker_count,
                    actual: history_len,
                },
            )?;
            let smoothed = if runtime_input_is_finite {
                finite_f32(
                    *previous_gain as f64
                        + (target as f64 - *previous_gain as f64) * self.smoothing_alpha as f64,
                )
            } else {
                0.0
            };
            *previous_gain = smoothed;
            let distance = if runtime_input_is_finite {
                distance(object.position, speaker.position)
            } else {
                0.0
            };
            *result = SpeakerGain {
                speaker_index,
                gain: smoothed,
                distance_meters: finite_f32(distance),
                delay_samples: finite_f32(
                    speaker.delay_samples as f64
                        + distance / DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND
                            * self.sample_rate as f64,
                ),
            };
        }
        Ok(())
    }

    fn select_triplet(
        &self,
        source_direction: [f64; 3],
        directions: &[f32],
        weights: &mut [f32],
    ) -> bool {
        for triplet in &self.triplets {
            let [first, second, third] = triplet.speaker_indices;
            let Some(first_direction) = scratch_direction(directions, first) else {
                continue;
            };
            let Some(second_direction) = scratch_direction(directions, second) else {
                continue;
            };
            let Some(third_direction) = scratch_direction(directions, third) else {
                continue;
            };
            let Some(mut gains) = solve_triplet(
                first_direction,
                second_direction,
                third_direction,
                source_direction,
            ) else {
                continue;
            };
            if gains.iter().any(|gain| *gain < -GAIN_EPSILON) {
                continue;
            }
            for gain in &mut gains {
                *gain = gain.max(0.0);
            }
            let norm = (gains[0] * gains[0] + gains[1] * gains[1] + gains[2] * gains[2]).sqrt();
            if norm <= GEOMETRY_EPSILON || !norm.is_finite() {
                continue;
            }
            weights[first] = finite_f32(gains[0] / norm);
            weights[second] = finite_f32(gains[1] / norm);
            weights[third] = finite_f32(gains[2] / norm);
            return true;
        }
        false
    }

    fn validate_render_buffers(
        &self,
        object_count: usize,
        output_count: usize,
        scratch: &mut RendererScratch,
    ) -> Result<(), RendererError> {
        if !self.configured {
            return Err(RendererError::NotConfigured);
        }
        if object_count > self.max_objects {
            return Err(RendererError::TooManyObjects {
                maximum: self.max_objects,
                actual: object_count,
            });
        }
        let required_output = object_count.saturating_mul(self.layout.len());
        if output_count != required_output {
            return Err(RendererError::OutputBufferSize {
                required: required_output,
                actual: output_count,
            });
        }
        let required_scratch = self.layout.len().checked_mul(4).ok_or_else(|| {
            RendererError::InvalidConfiguration("3D VBAP scratch size overflow".to_owned())
        })?;
        let actual_scratch = scratch.floats_mut().len();
        if actual_scratch < required_scratch {
            return Err(RendererError::ScratchBufferSize {
                required: required_scratch,
                actual: actual_scratch,
            });
        }
        Ok(())
    }
}

impl Renderer for Vbap3dRenderer {
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
        let enabled_layout = layout
            .into_iter()
            .filter(|speaker| speaker.enabled)
            .collect::<Vec<_>>();
        if enabled_layout.is_empty() {
            return Err(RendererError::NoEnabledSpeakers);
        }
        if enabled_layout.iter().any(|speaker| {
            !vector_is_finite(speaker.position)
                || !speaker.gain_db.is_finite()
                || !speaker.delay_samples.is_finite()
        }) {
            return Err(RendererError::InvalidConfiguration(
                "enabled speaker positions, gains, and delays must be finite".to_owned(),
            ));
        }

        let spatial_indices = enabled_layout
            .iter()
            .enumerate()
            .filter_map(|(index, speaker)| {
                (!matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects)).then_some(index)
            })
            .collect::<Vec<_>>();
        if spatial_indices.len() < 3 {
            return Err(RendererError::InvalidConfiguration(
                "3D VBAP requires at least three enabled non-LFE speakers".to_owned(),
            ));
        }
        if spatial_indices.len() > MAX_SPATIAL_SPEAKERS {
            return Err(RendererError::InvalidConfiguration(format!(
                "3D VBAP supports at most {MAX_SPATIAL_SPEAKERS} spatial speakers"
            )));
        }
        if spatial_indices.iter().any(|index| {
            length(vector_to_array(enabled_layout[*index].position)) <= GEOMETRY_EPSILON
        }) {
            return Err(RendererError::InvalidConfiguration(
                "3D VBAP spatial speakers must not be located at the reference origin".to_owned(),
            ));
        }

        let mut triplets = build_triplets(&enabled_layout, &spatial_indices);
        if triplets.is_empty() {
            return Err(RendererError::InvalidConfiguration(
                "3D VBAP layout has no non-degenerate loudspeaker triplet".to_owned(),
            ));
        }
        triplets.sort_by(compare_triplets);

        let history_len = enabled_layout
            .len()
            .checked_mul(max_objects)
            .ok_or_else(|| {
                RendererError::InvalidConfiguration(
                    "enabled speaker and object capacity product is too large".to_owned(),
                )
            })?;

        self.layout = enabled_layout;
        self.spatial_indices = spatial_indices;
        self.triplets = triplets;
        self.sample_rate = sample_rate;
        self.block_size = block_size;
        self.max_objects = max_objects;
        self.previous_gains = vec![0.0; history_len];
        self.configured = true;
        Ok(())
    }

    fn required_scratch_size(&self) -> Result<RendererScratchSize, RendererError> {
        if !self.configured {
            return Err(RendererError::NotConfigured);
        }
        let float_count = self.layout.len().checked_mul(4).ok_or_else(|| {
            RendererError::InvalidConfiguration("3D VBAP scratch size overflow".to_owned())
        })?;
        Ok(RendererScratchSize { float_count })
    }

    fn render_gains(
        &mut self,
        listener: &Listener,
        objects: &[RenderObject],
        output_gains: &mut [SpeakerGain],
        scratch: &mut RendererScratch,
    ) -> Result<(), RendererError> {
        self.validate_render_buffers(objects.len(), output_gains.len(), scratch)?;
        let speaker_count = self.layout.len();
        let direction_count = speaker_count * 3;
        let scratch_floats = scratch.floats_mut();
        let (directions, remaining) = scratch_floats.split_at_mut(direction_count);
        let weights = &mut remaining[..speaker_count];

        for (speaker, direction_output) in self
            .layout
            .iter()
            .zip(directions.chunks_exact_mut(3))
        {
            let direction = if vector_is_finite(listener.position) {
                normalize(subtract(speaker.position, listener.position))
            } else {
                None
            };
            if let Some(direction) = direction {
                direction_output[0] = finite_f32(direction[0]);
                direction_output[1] = finite_f32(direction[1]);
                direction_output[2] = finite_f32(direction[2]);
            } else {
                direction_output.fill(0.0);
            }
        }

        for (object_index, (object, object_output)) in objects
            .iter()
            .copied()
            .zip(output_gains.chunks_exact_mut(speaker_count))
            .enumerate()
        {
            self.render_object(
                listener,
                object_index,
                object,
                directions,
                weights,
                object_output,
            )?;
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

fn build_triplets(layout: &[Speaker], spatial_indices: &[usize]) -> Vec<ValidatedTriplet> {
    let mut triplets = Vec::new();
    for first_position in 0..spatial_indices.len() - 2 {
        for second_position in first_position + 1..spatial_indices.len() - 1 {
            for third_position in second_position + 1..spatial_indices.len() {
                let indices = [
                    spatial_indices[first_position],
                    spatial_indices[second_position],
                    spatial_indices[third_position],
                ];
                let Some(first) = normalize(vector_to_array(layout[indices[0]].position)) else {
                    continue;
                };
                let Some(second) = normalize(vector_to_array(layout[indices[1]].position)) else {
                    continue;
                };
                let Some(third) = normalize(vector_to_array(layout[indices[2]].position)) else {
                    continue;
                };
                let determinant_abs = determinant(first, second, third).abs();
                if determinant_abs <= GEOMETRY_EPSILON {
                    continue;
                }
                let mut speaker_ids = [
                    layout[indices[0]].id.clone(),
                    layout[indices[1]].id.clone(),
                    layout[indices[2]].id.clone(),
                ];
                speaker_ids.sort();
                triplets.push(ValidatedTriplet {
                    speaker_indices: indices,
                    speaker_ids,
                    determinant_abs,
                    span_radians: maximum_pair_angle(first, second, third),
                });
            }
        }
    }
    triplets
}

fn compare_triplets(first: &ValidatedTriplet, second: &ValidatedTriplet) -> Ordering {
    first
        .span_radians
        .total_cmp(&second.span_radians)
        .then_with(|| first.speaker_ids.cmp(&second.speaker_ids))
        .then_with(|| first.speaker_indices.cmp(&second.speaker_indices))
}

fn solve_triplet(
    first: [f64; 3],
    second: [f64; 3],
    third: [f64; 3],
    source: [f64; 3],
) -> Option<[f64; 3]> {
    let determinant = determinant(first, second, third);
    if determinant.abs() <= GEOMETRY_EPSILON || !determinant.is_finite() {
        return None;
    }
    let gains = [
        dot(source, cross(second, third)) / determinant,
        dot(first, cross(source, third)) / determinant,
        dot(first, cross(second, source)) / determinant,
    ];
    gains.iter().all(|gain| gain.is_finite()).then_some(gains)
}

fn nearest_direction(
    layout: &[Speaker],
    spatial_indices: &[usize],
    source_direction: [f64; 3],
    directions: &[f32],
    weights: &mut [f32],
) {
    let mut best = None::<(usize, f64)>;
    for index in spatial_indices.iter().copied() {
        let Some(direction) = scratch_direction(directions, index) else {
            continue;
        };
        let candidate_dot = dot(direction, source_direction);
        let replace = best.map_or(true, |(best_index, best_dot)| {
            candidate_dot > best_dot + GAIN_EPSILON
                || ((candidate_dot - best_dot).abs() <= GAIN_EPSILON
                    && (layout[index].id < layout[best_index].id
                        || (layout[index].id == layout[best_index].id && index < best_index)))
        });
        if replace {
            best = Some((index, candidate_dot));
        }
    }
    if let Some((index, _)) = best {
        weights[index] = 1.0;
    } else {
        equal_power_spatial(weights, spatial_indices);
    }
}

fn equal_power_spatial(weights: &mut [f32], spatial_indices: &[usize]) {
    if spatial_indices.is_empty() {
        return;
    }
    let gain = 1.0 / (spatial_indices.len() as f32).sqrt();
    for index in spatial_indices.iter().copied() {
        weights[index] = gain;
    }
}

fn scratch_direction(directions: &[f32], speaker_index: usize) -> Option<[f64; 3]> {
    let start = speaker_index.checked_mul(3)?;
    let values = directions.get(start..start + 3)?;
    let direction = [values[0] as f64, values[1] as f64, values[2] as f64];
    (length(direction) > GEOMETRY_EPSILON).then_some(direction)
}

fn maximum_pair_angle(first: [f64; 3], second: [f64; 3], third: [f64; 3]) -> f64 {
    angle(first, second)
        .max(angle(first, third))
        .max(angle(second, third))
}

fn angle(first: [f64; 3], second: [f64; 3]) -> f64 {
    dot(first, second).clamp(-1.0, 1.0).acos()
}

fn determinant(first: [f64; 3], second: [f64; 3], third: [f64; 3]) -> f64 {
    dot(first, cross(second, third))
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    let vector_length = length(vector);
    (vector_length > GEOMETRY_EPSILON && vector_length.is_finite())
        .then(|| scale(vector, 1.0 / vector_length))
}

fn scale(vector: [f64; 3], scalar: f64) -> [f64; 3] {
    [vector[0] * scalar, vector[1] * scalar, vector[2] * scalar]
}

fn subtract(first: Vector3, second: Vector3) -> [f64; 3] {
    [
        first.x as f64 - second.x as f64,
        first.y as f64 - second.y as f64,
        first.z as f64 - second.z as f64,
    ]
}

fn vector_to_array(vector: Vector3) -> [f64; 3] {
    [vector.x as f64, vector.y as f64, vector.z as f64]
}

fn length(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn distance(first: Vector3, second: Vector3) -> f64 {
    length(subtract(first, second))
}

fn vector_is_finite(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

fn db_to_gain(db: f32) -> f64 {
    10.0_f64.powf(db as f64 / 20.0)
}

fn finite_f32(value: f64) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use aurora_scene::load_render_scene;

    use super::*;

    fn speaker(id: &str, role: ChannelRole, x: f32, y: f32, z: f32) -> Speaker {
        Speaker {
            id: id.to_owned(),
            label: id.to_owned(),
            channel_role: role,
            position: Vector3::new(x, y, z),
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

    fn axis_layout() -> Vec<Speaker> {
        vec![
            speaker(
                "axis-x",
                ChannelRole::Custom("axis-x".to_owned()),
                1.0,
                0.0,
                0.0,
            ),
            speaker(
                "axis-y",
                ChannelRole::Custom("axis-y".to_owned()),
                0.0,
                1.0,
                0.0,
            ),
            speaker(
                "axis-z",
                ChannelRole::Custom("axis-z".to_owned()),
                0.0,
                0.0,
                1.0,
            ),
        ]
    }

    fn configured(layout: Vec<Speaker>) -> (Vbap3dRenderer, RendererScratch) {
        let mut renderer = Vbap3dRenderer::new();
        renderer.configure(layout, 48_000, 256, 1).unwrap();
        let scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        (renderer, scratch)
    }

    fn render(layout: Vec<Speaker>, position: Vector3) -> Vec<SpeakerGain> {
        let speaker_count = layout.iter().filter(|speaker| speaker.enabled).count();
        let (mut renderer, mut scratch) = configured(layout);
        let mut output = vec![SpeakerGain::default(); speaker_count];
        renderer
            .render_gains(
                &listener(),
                &[RenderObject {
                    position,
                    gain: 1.0,
                }],
                &mut output,
                &mut scratch,
            )
            .unwrap();
        output
    }

    fn power(gains: &[SpeakerGain]) -> f32 {
        gains.iter().map(|gain| gain.gain * gain.gain).sum()
    }

    #[test]
    fn configuration_builds_non_degenerate_triplets() {
        let (renderer, _) = configured(axis_layout());
        assert_eq!(renderer.validated_triplets().len(), 1);
        let triplet = &renderer.validated_triplets()[0];
        assert!(triplet.determinant_abs() > 0.99);
        assert_eq!(
            triplet.speaker_ids(),
            &["axis-x".to_owned(), "axis-y".to_owned(), "axis-z".to_owned()]
        );
    }

    #[test]
    fn coplanar_direction_layout_without_valid_triplet_is_rejected() {
        let mut renderer = Vbap3dRenderer::new();
        let error = renderer
            .configure(
                vec![
                    speaker("east", ChannelRole::FrontRight, 1.0, 0.0, 0.0),
                    speaker("north", ChannelRole::FrontCenter, 0.0, 1.0, 0.0),
                    speaker("west", ChannelRole::FrontLeft, -1.0, 0.0, 0.0),
                ],
                48_000,
                256,
                1,
            )
            .unwrap_err();
        assert!(matches!(error, RendererError::InvalidConfiguration(_)));
    }

    #[test]
    fn exact_axis_direction_routes_to_matching_speaker() {
        let gains = render(axis_layout(), Vector3::new(1.0, 0.0, 0.0));
        assert!((gains[0].gain - 1.0).abs() < 1.0e-5);
        assert!(gains[1].gain.abs() < 1.0e-5);
        assert!(gains[2].gain.abs() < 1.0e-5);
    }

    #[test]
    fn diagonal_direction_preserves_unit_spatial_power() {
        let gains = render(axis_layout(), Vector3::new(1.0, 1.0, 1.0));
        let expected = 1.0 / 3.0_f32.sqrt();
        for gain in &gains {
            assert!((gain.gain - expected).abs() < 1.0e-5);
        }
        assert!((power(&gains) - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn canonical_front_horizon_uses_equal_front_pair_without_height() {
        let layout = vec![
            speaker("front-left", ChannelRole::FrontLeft, -1.0, 1.0, 0.0),
            speaker("front-right", ChannelRole::FrontRight, 1.0, 1.0, 0.0),
            speaker(
                "top-front",
                ChannelRole::TopFrontLeft,
                0.0,
                1.0,
                1.0,
            ),
        ];
        let gains = render(layout, Vector3::new(0.0, 1.0, 0.0));
        assert!((gains[0].gain - std::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-5);
        assert!((gains[1].gain - std::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-5);
        assert!(gains[2].gain.abs() < 1.0e-5);
    }

    #[test]
    fn elevated_front_direction_adds_height_energy() {
        let layout = vec![
            speaker("front-left", ChannelRole::FrontLeft, -1.0, 1.0, 0.0),
            speaker("front-right", ChannelRole::FrontRight, 1.0, 1.0, 0.0),
            speaker(
                "top-front",
                ChannelRole::TopFrontLeft,
                0.0,
                1.0,
                1.0,
            ),
        ];
        let gains = render(layout, Vector3::new(0.0, 1.0, 0.7));
        assert!(gains[2].gain > 0.0);
        assert!((power(&gains) - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn outside_triplet_hull_uses_deterministic_nearest_direction_fallback() {
        let first = render(axis_layout(), Vector3::new(-1.0, 0.0, 0.0));
        let second = render(axis_layout(), Vector3::new(-1.0, 0.0, 0.0));
        assert_eq!(first, second);
        assert_eq!(first.iter().filter(|gain| gain.gain > 0.5).count(), 1);
        assert!((power(&first) - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn lfe_is_never_used_as_spatial_vbap_vertex() {
        let mut layout = axis_layout();
        layout.insert(
            0,
            speaker(
                "lfe",
                ChannelRole::LowFrequencyEffects,
                0.0,
                0.0,
                2.0,
            ),
        );
        let gains = render(layout, Vector3::new(0.2, 0.2, 1.0));
        assert_eq!(gains[0].gain, 0.0);
        assert!(gains[1..].iter().any(|gain| gain.gain > 0.0));
    }

    #[test]
    fn non_finite_runtime_source_produces_finite_silence() {
        let gains = render(axis_layout(), Vector3::new(f32::NAN, 1.0, 1.0));
        assert!(gains.iter().all(|gain| gain.gain == 0.0));
        assert!(gains.iter().all(|gain| gain.distance_meters.is_finite()));
        assert!(gains.iter().all(|gain| gain.delay_samples.is_finite()));
    }

    #[test]
    fn existing_five_one_two_fixture_configures_and_routes_height() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/scenes/5_1_2_upfiring.json");
        let scene = load_render_scene(&fixture).unwrap();
        let mut renderer = Vbap3dRenderer::new();
        renderer
            .configure(scene.speakers.clone(), 48_000, scene.block_size, 1)
            .unwrap();
        assert!(!renderer.validated_triplets().is_empty());

        let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        let mut gains = vec![SpeakerGain::default(); renderer.output_channel_count()];
        renderer
            .render_gains(
                &scene.listener,
                &[RenderObject {
                    position: Vector3::new(0.0, 1.1, 2.2),
                    gain: 1.0,
                }],
                &mut gains,
                &mut scratch,
            )
            .unwrap();

        assert!(gains.iter().all(|gain| gain.gain.is_finite()));
        let lfe_index = scene
            .speakers
            .iter()
            .filter(|speaker| speaker.enabled)
            .position(|speaker| matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects))
            .unwrap();
        assert_eq!(gains[lfe_index].gain, 0.0);
        let top_energy = gains
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                matches!(
                    scene.speakers[*index].channel_role,
                    ChannelRole::TopFrontLeft | ChannelRole::TopFrontRight
                )
            })
            .map(|(_, gain)| gain.gain * gain.gain)
            .sum::<f32>();
        assert!(top_energy > 0.0);
    }
}
