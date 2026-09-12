//! Deterministic geometric renderer implementations.

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{
    RenderObject, Renderer, RendererCapabilities, RendererError, RendererScratch,
    RendererScratchSize, SpeakerGain,
};

const DISTANCE_EPSILON: f32 = 0.001;
const COINCIDENT_EPSILON: f32 = 0.0001;
const DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND: f32 = 343.0;

/// Basic deterministic geometric renderer modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasicRendererMode {
    /// Route the object entirely to the closest enabled speaker.
    NearestSpeaker,
    /// Weight all enabled speakers by inverse squared distance.
    InverseDistance,
    /// Weight the two closest enabled speakers with equal-power normalization.
    EqualPowerAdjacent,
    /// Lightweight geometric stereo rendering for headphones.
    ///
    /// This mode applies geometric interaural time difference (ITD), geometric
    /// interaural level difference (ILD), and per-ear geometric distance
    /// weighting followed by power normalization. It is not an HRTF renderer
    /// and uses no HRIR data, convolution, pinna cues, or elevation cues.
    GeometricBinaural,
}

/// Minimal deterministic renderer using geometric distances.
///
/// [`BasicRendererMode::GeometricBinaural`] adds geometric ITD, geometric ILD,
/// and per-ear geometric distance weighting followed by power normalization
/// for two-channel headphone output. It is not an HRTF renderer and provides
/// no HRIR data, convolution, pinna cues, or elevation cues.
#[derive(Debug, Clone)]
pub struct BasicRenderer {
    mode: BasicRendererMode,
    layout: Vec<Speaker>,
    sample_rate: u32,
    block_size: usize,
    max_objects: usize,
    smoothing_alpha: f32,
    previous_gains: Vec<f32>,
    configured: bool,
}

/// Geometric delay alignment calculated for one speaker.
#[derive(Debug, Clone, PartialEq)]
pub struct GeometricDelay {
    /// Speaker identifier.
    pub speaker_id: String,
    /// Speaker channel role.
    pub channel_role: ChannelRole,
    /// Distance from listener to speaker in meters.
    pub distance_meters: f32,
    /// Added delay in samples.
    pub delay_samples: f32,
    /// Added delay in milliseconds.
    pub delay_milliseconds: f32,
}

impl BasicRenderer {
    /// Creates a basic renderer using the selected mode.
    pub fn new(mode: BasicRendererMode) -> Self {
        Self {
            mode,
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
        object_index: usize,
        listener: &Listener,
        object: RenderObject,
        output: &mut [SpeakerGain],
        weights: &mut [f32],
    ) -> Result<(), RendererError> {
        if self.mode == BasicRendererMode::GeometricBinaural {
            let forward = listener.orientation;
            let right = Vector3::new(forward.y, -forward.x, 0.0);
            let right_len = right.length();
            let right_normalized = if right_len > 0.0001 {
                Vector3::new(right.x / right_len, right.y / right_len, 0.0)
            } else {
                Vector3::new(1.0, 0.0, 0.0)
            };
            let head_radius = 0.0875;
            let head_center = Vector3::new(
                listener.position.x,
                listener.position.y,
                listener.position.z + listener.ear_height,
            );
            let ear_offset = Vector3::new(
                right_normalized.x * head_radius,
                right_normalized.y * head_radius,
                0.0,
            );
            let left_ear = head_center - ear_offset;
            let right_ear = Vector3::new(
                head_center.x + ear_offset.x,
                head_center.y + ear_offset.y,
                head_center.z + ear_offset.z,
            );

            let dist_l = object.position.distance_to(left_ear);
            let dist_r = object.position.distance_to(right_ear);

            let weight_l = 1.0 / (dist_l + DISTANCE_EPSILON).powi(2);
            let weight_r = 1.0 / (dist_r + DISTANCE_EPSILON).powi(2);

            let to_source = object.position - head_center;
            let to_source_2d = Vector3::new(to_source.x, to_source.y, 0.0);
            let to_source_len = to_source_2d.length();
            let sin_theta = if to_source_len > 0.0001 {
                let to_source_dir = Vector3::new(
                    to_source_2d.x / to_source_len,
                    to_source_2d.y / to_source_len,
                    0.0,
                );
                let forward_2d = Vector3::new(forward.x, forward.y, 0.0);
                let forward_len = forward_2d.length();
                if forward_len > 0.0001 {
                    let forward_dir =
                        Vector3::new(forward_2d.x / forward_len, forward_2d.y / forward_len, 0.0);
                    forward_dir.x * to_source_dir.y - forward_dir.y * to_source_dir.x
                } else {
                    0.0
                }
            } else {
                0.0
            };

            let head_shadow_factor = 0.4;
            let ild_l = 1.0 - ((-sin_theta).max(0.0) * (1.0 - head_shadow_factor));
            let ild_r = 1.0 - (sin_theta.max(0.0) * (1.0 - head_shadow_factor));

            weights[0] = weight_l * ild_l;
            weights[1] = weight_r * ild_r;
            normalize_power(weights);

            let target_l = weights[0] * db_to_gain(self.layout[0].gain_db) * object.gain;
            let target_r = weights[1] * db_to_gain(self.layout[1].gain_db) * object.gain;

            let history_index_l = object_index * 2;
            let history_index_r = object_index * 2 + 1;

            let previous_l = &mut self.previous_gains[history_index_l];
            let smoothed_l = *previous_l + (target_l - *previous_l) * self.smoothing_alpha;
            *previous_l = smoothed_l;

            let previous_r = &mut self.previous_gains[history_index_r];
            let smoothed_r = *previous_r + (target_r - *previous_r) * self.smoothing_alpha;
            *previous_r = smoothed_r;

            output[0] = SpeakerGain {
                speaker_index: 0,
                gain: smoothed_l,
                distance_meters: dist_l,
                delay_samples: dist_l / DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND
                    * self.sample_rate as f32,
            };
            output[1] = SpeakerGain {
                speaker_index: 1,
                gain: smoothed_r,
                distance_meters: dist_r,
                delay_samples: dist_r / DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND
                    * self.sample_rate as f32,
            };
            return Ok(());
        }

        match self.mode {
            BasicRendererMode::NearestSpeaker => {
                nearest_speaker_gains(&self.layout, object.position, weights)
            }
            BasicRendererMode::InverseDistance => {
                inverse_distance_gains(&self.layout, object.position, weights)
            }
            BasicRendererMode::EqualPowerAdjacent => {
                adjacent_speaker_gains(&self.layout, object.position, weights)
            }
            BasicRendererMode::GeometricBinaural => unreachable!(),
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
            let target = *weight * db_to_gain(speaker.gain_db) * object.gain;
            let history_index = object_index * speaker_count + speaker_index;
            let previous_gain = self.previous_gains.get_mut(history_index).ok_or(
                RendererError::OutputBufferSize {
                    required: self.max_objects * speaker_count,
                    actual: history_len,
                },
            )?;
            let previous = *previous_gain;
            let smoothed = previous + (target - previous) * self.smoothing_alpha;
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

/// Calculates speaker alignment delays relative to the farthest enabled speaker.
pub fn calculate_geometric_delays(
    speakers: &[Speaker],
    listener: Listener,
    sample_rate: u32,
    speed_of_sound_meters_per_second: f32,
) -> Vec<GeometricDelay> {
    let max_distance = speakers
        .iter()
        .filter(|speaker| speaker.enabled)
        .map(|speaker| speaker.position.distance_to(listener.position))
        .fold(0.0_f32, f32::max);

    speakers
        .iter()
        .map(|speaker| {
            let distance_meters = speaker.position.distance_to(listener.position);
            let delay_seconds = if speaker.enabled && speed_of_sound_meters_per_second > 0.0 {
                (max_distance - distance_meters).max(0.0) / speed_of_sound_meters_per_second
            } else {
                0.0
            };
            GeometricDelay {
                speaker_id: speaker.id.clone(),
                channel_role: speaker.channel_role.clone(),
                distance_meters,
                delay_samples: delay_seconds * sample_rate as f32,
                delay_milliseconds: delay_seconds * 1000.0,
            }
        })
        .collect()
}

impl Renderer for BasicRenderer {
    fn capabilities(&self) -> RendererCapabilities {
        RendererCapabilities::new(self.mode == BasicRendererMode::GeometricBinaural)
    }

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
        if self.mode == BasicRendererMode::GeometricBinaural {
            let mut enabled_roles = layout
                .iter()
                .filter(|speaker| speaker.enabled)
                .map(|speaker| &speaker.channel_role);
            let has_canonical_stereo_order = matches!(
                (
                    enabled_roles.next(),
                    enabled_roles.next(),
                    enabled_roles.next()
                ),
                (
                    Some(ChannelRole::FrontLeft),
                    Some(ChannelRole::FrontRight),
                    None
                )
            );
            if !has_canonical_stereo_order {
                return Err(RendererError::InvalidConfiguration(
                    "geometric binaural mode requires enabled output channels in canonical order [FrontLeft, FrontRight]"
                        .to_owned(),
                ));
            }
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
            self.render_object(object_index, listener, object, object_output, weights)?;
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

fn nearest_speaker_gains(speakers: &[Speaker], source: Vector3, weights: &mut [f32]) {
    weights.fill(0.0);
    let mut nearest_index = 0;
    let mut nearest_distance = f32::INFINITY;
    for (index, speaker) in speakers.iter().enumerate() {
        let distance = source.distance_to(speaker.position);
        if distance < nearest_distance {
            nearest_distance = distance;
            nearest_index = index;
        }
    }
    if let Some(weight) = weights.get_mut(nearest_index) {
        *weight = 1.0;
    }
}

fn inverse_distance_gains(speakers: &[Speaker], source: Vector3, weights: &mut [f32]) {
    weights.fill(0.0);
    for (index, speaker) in speakers.iter().enumerate() {
        let distance = source.distance_to(speaker.position);
        if distance < COINCIDENT_EPSILON {
            weights.fill(0.0);
            if let Some(weight) = weights.get_mut(index) {
                *weight = 1.0;
            }
            return;
        }
        if let Some(weight) = weights.get_mut(index) {
            *weight = 1.0 / (distance + DISTANCE_EPSILON).powi(2);
        }
    }
    normalize_power(weights);
}

fn adjacent_speaker_gains(speakers: &[Speaker], source: Vector3, weights: &mut [f32]) {
    if speakers.len() <= 2 {
        inverse_distance_gains(speakers, source, weights);
        return;
    }
    weights.fill(0.0);
    let mut first = (usize::MAX, f32::INFINITY);
    let mut second = (usize::MAX, f32::INFINITY);
    for (index, speaker) in speakers.iter().enumerate() {
        let distance = source.distance_to(speaker.position);
        if distance < first.1 {
            second = first;
            first = (index, distance);
        } else if distance < second.1 {
            second = (index, distance);
        }
    }
    if let Some(weight) = weights.get_mut(first.0) {
        *weight = 1.0 / (first.1 + DISTANCE_EPSILON).powi(2);
    }
    if let Some(weight) = weights.get_mut(second.0) {
        *weight = 1.0 / (second.1 + DISTANCE_EPSILON).powi(2);
    }
    normalize_power(weights);
}

fn normalize_power(weights: &mut [f32]) {
    let mut sum = 0.0_f32;
    for weight in weights.iter() {
        sum += *weight;
    }
    if sum <= f32::EPSILON || !sum.is_finite() {
        weights.fill(0.0);
        return;
    }
    for weight in weights {
        *weight = (*weight / sum).sqrt();
    }
}

fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
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

    fn renderer() -> (BasicRenderer, RendererScratch, Vec<SpeakerGain>) {
        let mut renderer = BasicRenderer::new(BasicRendererMode::InverseDistance);
        renderer
            .configure(
                vec![
                    speaker("left", ChannelRole::FrontLeft, -1.0, 0.0),
                    speaker("right", ChannelRole::FrontRight, 1.0, 0.0),
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
    fn symmetric_stereo_center_position_has_equal_normalized_gain() {
        let gains = render_at(0.0, 0.0);
        assert!((gains[0].gain - gains[1].gain).abs() < 0.0001);
        assert!((gains[0].gain - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.0001);
    }

    #[test]
    fn source_exactly_at_speaker_position_maps_to_that_speaker() {
        let gains = render_at(-1.0, 0.0);
        assert_eq!(gains[0].gain, 1.0);
        assert_eq!(gains[1].gain, 0.0);
    }

    #[test]
    fn source_at_listener_is_finite_and_normalized() {
        let gains = render_at(0.0, 0.0);
        let power = gains.iter().map(|gain| gain.gain.powi(2)).sum::<f32>();
        assert!(gains.iter().all(|gain| gain.gain.is_finite()));
        assert!((power - 1.0).abs() < 0.0001);
    }

    #[test]
    fn render_reuses_caller_owned_storage() {
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
        let mut disabled = speaker("left", ChannelRole::FrontLeft, -1.0, 0.0);
        disabled.enabled = false;
        let error = BasicRenderer::new(BasicRendererMode::InverseDistance)
            .configure(vec![disabled], 48_000, 256, 1)
            .unwrap_err();
        assert_eq!(error, RendererError::NoEnabledSpeakers);
    }

    #[test]
    fn geometric_delay_farthest_is_zero_and_nearer_is_fractional() {
        let delays = calculate_geometric_delays(
            &[
                speaker("near", ChannelRole::FrontLeft, 1.0, 0.0),
                speaker("far", ChannelRole::FrontRight, 2.0, 0.0),
            ],
            listener(),
            48_000,
            343.0,
        );
        assert_eq!(delays[1].delay_samples, 0.0);
        assert!((delays[0].delay_samples - 48_000.0 / 343.0).abs() < 0.001);
    }

    #[test]
    fn output_is_deterministic() {
        assert_eq!(render_at(0.25, -0.5), render_at(0.25, -0.5));
    }

    fn geometric_binaural_renderer() -> (BasicRenderer, RendererScratch, Vec<SpeakerGain>) {
        let mut renderer = BasicRenderer::new(BasicRendererMode::GeometricBinaural);
        renderer
            .configure(
                vec![
                    speaker("left", ChannelRole::FrontLeft, -0.0875, 0.0),
                    speaker("right", ChannelRole::FrontRight, 0.0875, 0.0),
                ],
                48_000,
                256,
                1,
            )
            .unwrap();
        let scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        (renderer, scratch, vec![SpeakerGain::default(); 2])
    }

    fn render_geometric_binaural(position: Vector3) -> Vec<SpeakerGain> {
        let (mut renderer, mut scratch, mut gains) = geometric_binaural_renderer();
        let object = RenderObject {
            position,
            gain: 1.0,
        };
        renderer
            .render_gains(&listener(), &[object], &mut gains, &mut scratch)
            .unwrap();
        gains
    }

    #[test]
    fn right_and_left_sources_have_opposite_itd_polarity() {
        let right = render_geometric_binaural(Vector3::new(5.0, 0.0, 1.2));
        let left = render_geometric_binaural(Vector3::new(-5.0, 0.0, 1.2));

        assert!(right[1].delay_samples < right[0].delay_samples);
        assert!(left[0].delay_samples < left[1].delay_samples);
        let right_difference = right[0].delay_samples - right[1].delay_samples;
        let left_difference = left[1].delay_samples - left[0].delay_samples;
        assert!((right_difference - 24.489).abs() < 0.1);
        assert!((left_difference - 24.489).abs() < 0.1);
    }

    #[test]
    fn lateral_sources_have_correct_mirrored_ild_polarity() {
        let right = render_geometric_binaural(Vector3::new(5.0, 0.0, 1.2));
        let left = render_geometric_binaural(Vector3::new(-5.0, 0.0, 1.2));

        assert!(right[1].gain > right[0].gain);
        assert!(left[0].gain > left[1].gain);
        assert!((right[0].gain - left[1].gain).abs() < 0.0001);
        assert!((right[1].gain - left[0].gain).abs() < 0.0001);
    }

    #[test]
    fn geometric_distance_weighting_is_relative_without_absolute_rolloff() {
        let near_center = render_geometric_binaural(Vector3::new(0.0, 1.0, 1.2));
        let far_center = render_geometric_binaural(Vector3::new(0.0, 10.0, 1.2));
        let lateral = render_geometric_binaural(Vector3::new(5.0, 0.0, 1.2));

        for gains in [&near_center, &far_center, &lateral] {
            let power = gains.iter().map(|gain| gain.gain.powi(2)).sum::<f32>();
            assert!(gains.iter().all(|gain| gain.gain.is_finite()));
            assert!((power - 1.0).abs() < 0.0001);
        }
        assert!((near_center[0].gain - far_center[0].gain).abs() < 0.0001);
        assert!((near_center[1].gain - far_center[1].gain).abs() < 0.0001);
        assert!(lateral[1].gain > lateral[0].gain);
    }

    #[test]
    fn geometric_binaural_output_is_finite_and_power_normalized() {
        for position in [
            Vector3::new(-5.0, 0.0, 1.2),
            Vector3::new(0.0, 5.0, 1.2),
            Vector3::new(5.0, 0.0, 1.2),
        ] {
            let gains = render_geometric_binaural(position);
            let power = gains.iter().map(|gain| gain.gain.powi(2)).sum::<f32>();
            assert!(gains.iter().all(|gain| {
                gain.gain.is_finite()
                    && gain.distance_meters.is_finite()
                    && gain.delay_samples.is_finite()
            }));
            assert!((power - 1.0).abs() < 0.0001);
        }
    }

    #[test]
    fn source_at_head_center_is_finite_symmetric_and_normalized() {
        let gains = render_geometric_binaural(Vector3::new(0.0, 0.0, 1.2));
        let power = gains.iter().map(|gain| gain.gain.powi(2)).sum::<f32>();

        assert!(gains.iter().all(|gain| gain.gain.is_finite()));
        assert!((gains[0].gain - gains[1].gain).abs() < 0.0001);
        assert!((gains[0].delay_samples - gains[1].delay_samples).abs() < 0.0001);
        assert!((power - 1.0).abs() < 0.0001);
    }

    #[test]
    fn source_at_ear_position_remains_finite_and_normalized() {
        let gains = render_geometric_binaural(Vector3::new(0.0875, 0.0, 1.2));
        let power = gains.iter().map(|gain| gain.gain.powi(2)).sum::<f32>();

        assert!(gains.iter().all(|gain| {
            gain.gain.is_finite()
                && gain.distance_meters.is_finite()
                && gain.delay_samples.is_finite()
        }));
        assert_eq!(gains[1].distance_meters, 0.0);
        assert_eq!(gains[1].delay_samples, 0.0);
        assert!((power - 1.0).abs() < 0.0001);
    }

    fn assert_invalid_geometric_binaural_layout(layout: Vec<Speaker>) {
        let error = BasicRenderer::new(BasicRendererMode::GeometricBinaural)
            .configure(layout, 48_000, 256, 1)
            .unwrap_err();
        assert_eq!(
            error,
            RendererError::InvalidConfiguration(
                "geometric binaural mode requires enabled output channels in canonical order [FrontLeft, FrontRight]"
                    .to_owned()
            )
        );
    }

    #[test]
    fn geometric_binaural_accepts_canonical_stereo_layout() {
        let mut renderer = BasicRenderer::new(BasicRendererMode::GeometricBinaural);
        renderer
            .configure(
                vec![
                    speaker("left", ChannelRole::FrontLeft, -0.0875, 0.0),
                    speaker("right", ChannelRole::FrontRight, 0.0875, 0.0),
                ],
                48_000,
                256,
                1,
            )
            .unwrap();
    }

    #[test]
    fn geometric_binaural_rejects_mono_layout() {
        assert_invalid_geometric_binaural_layout(vec![speaker(
            "left",
            ChannelRole::FrontLeft,
            -0.0875,
            0.0,
        )]);
    }

    #[test]
    fn geometric_binaural_rejects_more_than_two_enabled_channels() {
        assert_invalid_geometric_binaural_layout(vec![
            speaker("left", ChannelRole::FrontLeft, -0.0875, 0.0),
            speaker("right", ChannelRole::FrontRight, 0.0875, 0.0),
            speaker("center", ChannelRole::FrontCenter, 0.0, 0.1),
        ]);
    }

    #[test]
    fn geometric_binaural_rejects_non_stereo_roles() {
        assert_invalid_geometric_binaural_layout(vec![
            speaker("center", ChannelRole::FrontCenter, -0.0875, 0.0),
            speaker("lfe", ChannelRole::LowFrequencyEffects, 0.0875, 0.0),
        ]);
    }

    #[test]
    fn geometric_binaural_rejects_reversed_channel_order() {
        assert_invalid_geometric_binaural_layout(vec![
            speaker("right", ChannelRole::FrontRight, 0.0875, 0.0),
            speaker("left", ChannelRole::FrontLeft, -0.0875, 0.0),
        ]);
    }

    #[test]
    fn geometric_binaural_rejects_duplicate_channel_roles() {
        assert_invalid_geometric_binaural_layout(vec![
            speaker("left-a", ChannelRole::FrontLeft, -0.0875, 0.0),
            speaker("left-b", ChannelRole::FrontLeft, 0.0875, 0.0),
        ]);
    }

    #[test]
    fn geometric_binaural_rejects_disabled_required_channel() {
        let mut left = speaker("left", ChannelRole::FrontLeft, -0.0875, 0.0);
        left.enabled = false;
        assert_invalid_geometric_binaural_layout(vec![
            left,
            speaker("right", ChannelRole::FrontRight, 0.0875, 0.0),
        ]);
    }

    #[test]
    fn moving_source_geometric_delays_change_continuously() {
        let (mut renderer, mut scratch, mut gains) = geometric_binaural_renderer();
        let mut previous: Option<[f32; 2]> = None;
        for step in 0..=360 {
            let angle = step as f32 * std::f32::consts::TAU / 360.0;
            renderer
                .render_gains(
                    &listener(),
                    &[RenderObject {
                        position: Vector3::new(angle.cos() * 2.0, angle.sin() * 2.0, 1.2),
                        gain: 1.0,
                    }],
                    &mut gains,
                    &mut scratch,
                )
                .unwrap();
            let current = [gains[0].delay_samples, gains[1].delay_samples];
            if let Some(previous) = previous {
                for channel in 0..2 {
                    assert!((current[channel] - previous[channel]).abs() < 0.5);
                }
            }
            previous = Some(current);
        }
    }
}
