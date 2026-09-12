//! Layer-aware immersive loudspeaker renderer for cinema-style layouts.
//!
//! [`ImmersiveLayeredRenderer`] keeps Aurora's generic convex-hull 3D VBAP for
//! interior directions, while treating the highest coplanar loudspeaker ring
//! as a closed azimuthal layer. Sources at or above that layer are panned only
//! within the upper ring. This avoids pulling a source authored on the height
//! boundary down into a base-layer loudspeaker merely because the physical
//! upper-ring polygon does not cover the same unit-sphere direction.
//!
//! The policy is layout-generic: no 7.1.4 channel names or angles are
//! hard-coded. It activates only when at least two non-LFE loudspeakers share
//! the highest positive listener-relative elevation.

use std::cmp::Ordering;
use std::f64::consts::{PI, TAU};

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{
    RenderObject, Renderer, RendererCapabilities, RendererError, RendererScratch,
    RendererScratchSize, SpeakerGain,
};

use crate::{ValidatedTriplet, Vbap3dRenderer};

const GEOMETRY_EPSILON: f64 = 1.0e-8;
const LAYER_EPSILON: f64 = 1.0e-6;
const ANGLE_EPSILON: f64 = 1.0e-8;

/// Hybrid 3D renderer optimized for standard immersive loudspeaker layouts.
#[derive(Debug, Clone)]
pub struct ImmersiveLayeredRenderer {
    inner: Vbap3dRenderer,
    layout: Vec<Speaker>,
    smoothing_alpha: f32,
    previous_gains: Vec<f32>,
    max_objects: usize,
    configured: bool,
}

impl Default for ImmersiveLayeredRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl ImmersiveLayeredRenderer {
    /// Creates an unconfigured renderer with smoothing disabled.
    pub fn new() -> Self {
        Self {
            inner: Vbap3dRenderer::new().with_smoothing(1.0),
            layout: Vec::new(),
            smoothing_alpha: 1.0,
            previous_gains: Vec::new(),
            max_objects: 0,
            configured: false,
        }
    }

    /// Sets block-to-block smoothing alpha, clamped to `0.0..=1.0`.
    pub fn with_smoothing(mut self, smoothing_alpha: f32) -> Self {
        self.smoothing_alpha = if smoothing_alpha.is_finite() {
            smoothing_alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self
    }

    /// Prepares the underlying listener-relative convex hull.
    pub fn prepare_listener(&mut self, listener: &Listener) -> Result<(), RendererError> {
        self.inner.prepare_listener(listener)
    }

    /// Returns the active listener-relative hull triplets from the generic 3D path.
    pub fn validated_triplets(&self) -> &[ValidatedTriplet] {
        self.inner.validated_triplets()
    }

    /// Reports whether the listener is inside the generic 3D hull.
    pub const fn listener_inside_hull(&self) -> bool {
        self.inner.listener_inside_hull()
    }

    fn apply_upper_layer_policy(
        &self,
        listener: &Listener,
        object: RenderObject,
        output: &mut [SpeakerGain],
    ) -> bool {
        let Some(listener_center) = acoustic_listener_center(listener) else {
            return false;
        };
        let Some(source_direction) = normalize(subtract(object.position, listener_center)) else {
            return false;
        };

        let mut maximum_z = f64::NEG_INFINITY;
        let mut upper_count = 0usize;
        for speaker in &self.layout {
            if matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects) {
                continue;
            }
            let Some(direction) = normalize(subtract(speaker.position, listener_center)) else {
                continue;
            };
            if direction[2] > maximum_z + LAYER_EPSILON {
                maximum_z = direction[2];
                upper_count = 1;
            } else if (direction[2] - maximum_z).abs() <= LAYER_EPSILON {
                upper_count += 1;
            }
        }

        if upper_count < 2
            || !maximum_z.is_finite()
            || maximum_z <= LAYER_EPSILON
            || source_direction[2] < maximum_z - LAYER_EPSILON
        {
            return false;
        }

        let source_horizontal = [source_direction[0], source_direction[1]];
        let horizontal_length = source_horizontal[0].hypot(source_horizontal[1]);
        if horizontal_length <= GEOMETRY_EPSILON {
            let gain = 1.0 / (upper_count as f64).sqrt();
            self.replace_with_upper_equal_power(listener_center, maximum_z, object, output, gain);
            return true;
        }
        let source_angle = source_horizontal[1].atan2(source_horizontal[0]);

        let mut best_pair = None::<(usize, usize, f64, f64, f64)>;
        for first in 0..self.layout.len() {
            let Some(first_direction) = self.upper_direction(first, listener_center, maximum_z) else {
                continue;
            };
            let first_angle = first_direction[1].atan2(first_direction[0]);
            for second in first + 1..self.layout.len() {
                let Some(second_direction) = self.upper_direction(second, listener_center, maximum_z)
                else {
                    continue;
                };
                let second_angle = second_direction[1].atan2(second_direction[0]);
                let delta = positive_angle(second_angle - first_angle);
                let (left, right, left_angle, span) = if delta <= PI {
                    (first, second, first_angle, delta)
                } else {
                    (second, first, second_angle, TAU - delta)
                };
                if span <= ANGLE_EPSILON || span > PI + ANGLE_EPSILON {
                    continue;
                }
                let offset = positive_angle(source_angle - left_angle);
                if offset > span + ANGLE_EPSILON {
                    continue;
                }
                let replace = best_pair.map_or(true, |(best_left, best_right, best_span, _, _)| {
                    span < best_span - ANGLE_EPSILON
                        || ((span - best_span).abs() <= ANGLE_EPSILON
                            && pair_precedes(&self.layout, left, right, best_left, best_right))
                });
                if replace {
                    best_pair = Some((left, right, span, left_angle, offset.clamp(0.0, span)));
                }
            }
        }

        let Some((left, right, span, _, offset)) = best_pair else {
            return false;
        };
        let denominator = span.sin();
        let (mut left_gain, mut right_gain) = if denominator.abs() <= GEOMETRY_EPSILON {
            (1.0, 0.0)
        } else {
            ((span - offset).sin() / denominator, offset.sin() / denominator)
        };
        left_gain = left_gain.max(0.0);
        right_gain = right_gain.max(0.0);
        let norm = left_gain.hypot(right_gain);
        if norm <= GEOMETRY_EPSILON || !norm.is_finite() {
            return false;
        }
        left_gain /= norm;
        right_gain /= norm;

        for result in output.iter_mut() {
            result.gain = 0.0;
        }
        set_output_gain(
            output,
            left,
            finite_f32(
                left_gain
                    * db_to_gain(self.layout[left].gain_db)
                    * object.gain as f64,
            ),
        );
        set_output_gain(
            output,
            right,
            finite_f32(
                right_gain
                    * db_to_gain(self.layout[right].gain_db)
                    * object.gain as f64,
            ),
        );
        true
    }

    fn upper_direction(
        &self,
        index: usize,
        listener_center: Vector3,
        maximum_z: f64,
    ) -> Option<[f64; 3]> {
        let speaker = self.layout.get(index)?;
        if matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects) {
            return None;
        }
        let direction = normalize(subtract(speaker.position, listener_center))?;
        ((direction[2] - maximum_z).abs() <= LAYER_EPSILON).then_some(direction)
    }

    fn replace_with_upper_equal_power(
        &self,
        listener_center: Vector3,
        maximum_z: f64,
        object: RenderObject,
        output: &mut [SpeakerGain],
        gain: f64,
    ) {
        for result in output.iter_mut() {
            result.gain = 0.0;
        }
        for index in 0..self.layout.len() {
            if self
                .upper_direction(index, listener_center, maximum_z)
                .is_none()
            {
                continue;
            }
            set_output_gain(
                output,
                index,
                finite_f32(gain * db_to_gain(self.layout[index].gain_db) * object.gain as f64),
            );
        }
    }

    fn apply_smoothing(&mut self, object_index: usize, output: &mut [SpeakerGain]) {
        let speaker_count = self.layout.len();
        for result in output.iter_mut() {
            let history_index = object_index * speaker_count + result.speaker_index;
            let Some(previous) = self.previous_gains.get_mut(history_index) else {
                result.gain = 0.0;
                continue;
            };
            let target = result.gain;
            let smoothed = *previous + (target - *previous) * self.smoothing_alpha;
            *previous = if smoothed.is_finite() { smoothed } else { 0.0 };
            result.gain = *previous;
        }
    }
}

impl Renderer for ImmersiveLayeredRenderer {
    fn capabilities(&self) -> RendererCapabilities {
        self.inner.capabilities()
    }

    fn configure(
        &mut self,
        layout: Vec<Speaker>,
        sample_rate: u32,
        block_size: usize,
        max_objects: usize,
    ) -> Result<(), RendererError> {
        let enabled_layout = layout
            .into_iter()
            .filter(|speaker| speaker.enabled)
            .collect::<Vec<_>>();
        self.inner.configure(
            enabled_layout.clone(),
            sample_rate,
            block_size,
            max_objects,
        )?;
        let history_len = enabled_layout
            .len()
            .checked_mul(max_objects)
            .ok_or_else(|| {
                RendererError::InvalidConfiguration(
                    "immersive layered renderer history capacity overflow".to_owned(),
                )
            })?;
        self.layout = enabled_layout;
        self.previous_gains = vec![0.0; history_len];
        self.max_objects = max_objects;
        self.configured = true;
        Ok(())
    }

    fn required_scratch_size(&self) -> Result<RendererScratchSize, RendererError> {
        self.inner.required_scratch_size()
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
        self.inner
            .render_gains(listener, objects, output_gains, scratch)?;
        let speaker_count = self.layout.len();
        for (object_index, (object, output)) in objects
            .iter()
            .copied()
            .zip(output_gains.chunks_exact_mut(speaker_count))
            .enumerate()
        {
            self.apply_upper_layer_policy(listener, object, output);
            self.apply_smoothing(object_index, output);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.previous_gains.fill(0.0);
    }

    fn latency_frames(&self) -> usize {
        self.inner.latency_frames()
    }

    fn output_channel_count(&self) -> usize {
        self.inner.output_channel_count()
    }
}

fn set_output_gain(output: &mut [SpeakerGain], speaker_index: usize, gain: f32) {
    if let Some(result) = output
        .iter_mut()
        .find(|result| result.speaker_index == speaker_index)
    {
        result.gain = gain;
    }
}

fn pair_precedes(
    layout: &[Speaker],
    left: usize,
    right: usize,
    best_left: usize,
    best_right: usize,
) -> bool {
    layout[left]
        .id
        .cmp(&layout[best_left].id)
        .then_with(|| layout[right].id.cmp(&layout[best_right].id))
        .then_with(|| left.cmp(&best_left))
        .then_with(|| right.cmp(&best_right))
        == Ordering::Less
}

fn positive_angle(angle: f64) -> f64 {
    angle.rem_euclid(TAU)
}

fn acoustic_listener_center(listener: &Listener) -> Option<Vector3> {
    let center = Vector3::new(
        listener.position.x,
        listener.position.y,
        listener.position.z + listener.ear_height,
    );
    (vector_is_finite(center) && listener.ear_height.is_finite()).then_some(center)
}

fn subtract(first: Vector3, second: Vector3) -> [f64; 3] {
    [
        first.x as f64 - second.x as f64,
        first.y as f64 - second.y as f64,
        first.z as f64 - second.z as f64,
    ]
}

fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt();
    (length > GEOMETRY_EPSILON && length.is_finite())
        .then(|| [vector[0] / length, vector[1] / length, vector[2] / length])
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
    use super::*;

    fn polar(azimuth_degrees: f32, elevation_degrees: f32) -> Vector3 {
        let azimuth = azimuth_degrees.to_radians();
        let elevation = elevation_degrees.to_radians();
        let horizontal = elevation.cos();
        Vector3::new(
            -horizontal * azimuth.sin(),
            horizontal * azimuth.cos(),
            elevation.sin(),
        )
    }

    fn speaker(id: &str, role: ChannelRole, azimuth: f32, elevation: f32) -> Speaker {
        Speaker {
            id: id.to_owned(),
            label: id.to_owned(),
            channel_role: role,
            position: polar(azimuth, elevation),
            orientation: Vector3::new(0.0, -1.0, 0.0),
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        }
    }

    fn seven_one_four() -> Vec<Speaker> {
        vec![
            speaker("fl", ChannelRole::FrontLeft, 30.0, 0.0),
            speaker("fr", ChannelRole::FrontRight, -30.0, 0.0),
            speaker("fc", ChannelRole::FrontCenter, 0.0, 0.0),
            speaker("lfe", ChannelRole::LowFrequencyEffects, 45.0, -30.0),
            speaker("sl", ChannelRole::SurroundLeft, 90.0, 0.0),
            speaker("sr", ChannelRole::SurroundRight, -90.0, 0.0),
            speaker("sbl", ChannelRole::SurroundBackLeft, 135.0, 0.0),
            speaker("sbr", ChannelRole::SurroundBackRight, -135.0, 0.0),
            speaker("tfl", ChannelRole::TopFrontLeft, 45.0, 30.0),
            speaker("tfr", ChannelRole::TopFrontRight, -45.0, 30.0),
            speaker("trl", ChannelRole::TopRearLeft, 135.0, 30.0),
            speaker("trr", ChannelRole::TopRearRight, -135.0, 30.0),
        ]
    }

    fn listener() -> Listener {
        Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 0.0,
        }
    }

    #[test]
    fn top_boundary_stays_in_upper_ring() {
        let mut renderer = ImmersiveLayeredRenderer::new();
        renderer.configure(seven_one_four(), 48_000, 256, 1).unwrap();
        let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        let object = RenderObject {
            position: polar(0.0, 30.0),
            gain: 1.0,
        };
        let mut output = [SpeakerGain::default(); 12];
        renderer
            .render_gains(&listener(), &[object], &mut output, &mut scratch)
            .unwrap();
        let base_energy: f32 = output[..8].iter().map(|result| result.gain * result.gain).sum();
        let top_energy: f32 = output[8..].iter().map(|result| result.gain * result.gain).sum();
        assert!(base_energy < 1.0e-6, "base energy leaked: {base_energy}");
        assert!((top_energy - 1.0).abs() < 1.0e-5, "top energy: {top_energy}");
        assert!((output[8].gain - std::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-5);
        assert!((output[9].gain - std::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-5);
    }

    #[test]
    fn interior_height_keeps_generic_3d_blend() {
        let mut renderer = ImmersiveLayeredRenderer::new();
        renderer.configure(seven_one_four(), 48_000, 256, 1).unwrap();
        let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        let object = RenderObject {
            position: polar(0.0, 15.0),
            gain: 1.0,
        };
        let mut output = [SpeakerGain::default(); 12];
        renderer
            .render_gains(&listener(), &[object], &mut output, &mut scratch)
            .unwrap();
        let base_energy: f32 = output[..8].iter().map(|result| result.gain * result.gain).sum();
        let top_energy: f32 = output[8..].iter().map(|result| result.gain * result.gain).sum();
        assert!(base_energy > 0.0);
        assert!(top_energy > 0.0);
        assert!((base_energy + top_energy - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn lfe_never_receives_upper_layer_object_energy() {
        let mut renderer = ImmersiveLayeredRenderer::new();
        renderer.configure(seven_one_four(), 48_000, 256, 1).unwrap();
        let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        let object = RenderObject {
            position: polar(60.0, 45.0),
            gain: 1.0,
        };
        let mut output = [SpeakerGain::default(); 12];
        renderer
            .render_gains(&listener(), &[object], &mut output, &mut scratch)
            .unwrap();
        assert_eq!(output[3].gain, 0.0);
    }
}
