//! Deterministic three-dimensional vector-base amplitude panning.
//!
//! This renderer is intentionally separate from the accepted horizontal 2D
//! implementation. It solves loudspeaker triplets in 3D, excludes LFE from
//! spatial panning, power-normalizes accepted triplet gains, and uses a
//! deterministic nearest-direction fallback outside the loudspeaker hull.

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{
    RenderObject, Renderer, RendererError, RendererScratch, RendererScratchSize, SpeakerGain,
};

const GEOMETRY_EPSILON: f64 = 1.0e-8;
const GAIN_EPSILON: f64 = 1.0e-7;
const QUALITY_EPSILON: f64 = 1.0e-10;
const DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND: f64 = 343.0;

/// Allocation-free 3D VBAP loudspeaker renderer.
///
/// The render callback performs a bounded brute-force triplet search over the
/// configured enabled non-LFE speakers. That is deliberately simple and
/// deterministic for the first accepted offline 3D implementation. Triplet
/// precomputation/acceleration may be added later only with identical output
/// evidence and callback-safety guarantees.
#[derive(Debug, Clone)]
pub struct Vbap3dRenderer {
    layout: Vec<Speaker>,
    sample_rate: u32,
    block_size: usize,
    max_objects: usize,
    configured: bool,
}

impl Default for Vbap3dRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Vbap3dRenderer {
    /// Creates an unconfigured 3D renderer.
    pub fn new() -> Self {
        Self {
            layout: Vec::new(),
            sample_rate: 48_000,
            block_size: 256,
            max_objects: 0,
            configured: false,
        }
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
        let required = object_count.saturating_mul(self.layout.len());
        if output_count != required {
            return Err(RendererError::OutputBufferSize {
                required,
                actual: output_count,
            });
        }
        let available = scratch.floats_mut().len();
        if available < self.layout.len() {
            return Err(RendererError::ScratchBufferSize {
                required: self.layout.len(),
                actual: available,
            });
        }
        Ok(())
    }

    fn render_object(
        &self,
        listener: &Listener,
        object: RenderObject,
        output: &mut [SpeakerGain],
        weights: &mut [f32],
    ) {
        let runtime_finite = vector_is_finite(listener.position)
            && vector_is_finite(object.position)
            && object.gain.is_finite();

        if runtime_finite {
            solve_3d_weights(
                &self.layout,
                listener.position,
                object.position,
                weights,
            );
        } else {
            weights.fill(0.0);
        }

        for (speaker_index, ((speaker, weight), result)) in self
            .layout
            .iter()
            .zip(weights.iter().copied())
            .zip(output.iter_mut())
            .enumerate()
        {
            let distance = if runtime_finite {
                distance_f64(object.position, speaker.position)
            } else {
                0.0
            };
            let gain = if runtime_finite {
                finite_f32(weight as f64 * db_to_gain(speaker.gain_db) * object.gain as f64)
            } else {
                0.0
            };
            *result = SpeakerGain {
                speaker_index,
                gain,
                distance_meters: finite_f32(distance),
                delay_samples: if runtime_finite {
                    finite_f32(
                        speaker.delay_samples as f64
                            + distance / DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND
                                * self.sample_rate as f64,
                    )
                } else {
                    0.0
                },
            };
        }
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

        let enabled_count = layout.iter().filter(|speaker| speaker.enabled).count();
        if enabled_count == 0 {
            return Err(RendererError::NoEnabledSpeakers);
        }
        let spatial_count = layout
            .iter()
            .filter(|speaker| speaker.enabled && is_spatial_speaker(speaker))
            .count();
        if spatial_count < 3 {
            return Err(RendererError::InvalidConfiguration(
                "3D VBAP requires at least three enabled non-LFE speakers".to_owned(),
            ));
        }
        if layout
            .iter()
            .filter(|speaker| speaker.enabled)
            .any(|speaker| {
                !vector_is_finite(speaker.position)
                    || !speaker.gain_db.is_finite()
                    || !speaker.delay_samples.is_finite()
            })
        {
            return Err(RendererError::InvalidConfiguration(
                "enabled speaker positions, gains, and delays must be finite".to_owned(),
            ));
        }

        self.layout.clear();
        self.layout.reserve(enabled_count);
        self.layout
            .extend(layout.into_iter().filter(|speaker| speaker.enabled));
        self.sample_rate = sample_rate;
        self.block_size = block_size;
        self.max_objects = max_objects;
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
        self.validate_render_buffers(objects.len(), output_gains.len(), scratch)?;
        let speaker_count = self.layout.len();
        let available = scratch.floats_mut().len();
        let weights = scratch.floats_mut().get_mut(..speaker_count).ok_or(
            RendererError::ScratchBufferSize {
                required: speaker_count,
                actual: available,
            },
        )?;

        for (object, object_output) in objects
            .iter()
            .copied()
            .zip(output_gains.chunks_exact_mut(speaker_count))
        {
            self.render_object(listener, object, object_output, weights);
        }
        Ok(())
    }

    fn reset(&mut self) {}

    fn latency_frames(&self) -> usize {
        let _ = self.block_size;
        0
    }

    fn output_channel_count(&self) -> usize {
        self.layout.len()
    }
}

fn solve_3d_weights(
    speakers: &[Speaker],
    listener: Vector3,
    source: Vector3,
    weights: &mut [f32],
) {
    weights.fill(0.0);
    let source_vector = vector_between(listener, source);
    let Some(source_direction) = normalize(source_vector) else {
        equal_power_spatial(speakers, weights);
        return;
    };

    let mut best = None::<TripletCandidate>;

    for first in 0..speakers.len() {
        if !is_spatial_speaker(&speakers[first]) {
            continue;
        }
        let Some(a) = normalize(vector_between(listener, speakers[first].position)) else {
            continue;
        };
        for second in (first + 1)..speakers.len() {
            if !is_spatial_speaker(&speakers[second]) {
                continue;
            }
            let Some(b) = normalize(vector_between(listener, speakers[second].position)) else {
                continue;
            };
            for third in (second + 1)..speakers.len() {
                if !is_spatial_speaker(&speakers[third]) {
                    continue;
                }
                let Some(c) = normalize(vector_between(listener, speakers[third].position)) else {
                    continue;
                };

                let det = dot(a, cross(b, c));
                if !det.is_finite() || det.abs() <= GEOMETRY_EPSILON {
                    continue;
                }

                let mut gains = [
                    dot(source_direction, cross(b, c)) / det,
                    dot(a, cross(source_direction, c)) / det,
                    dot(a, cross(b, source_direction)) / det,
                ];
                if gains
                    .iter()
                    .any(|gain| !gain.is_finite() || *gain < -GAIN_EPSILON)
                {
                    continue;
                }
                for gain in &mut gains {
                    *gain = gain.max(0.0);
                }
                let norm = (gains[0] * gains[0]
                    + gains[1] * gains[1]
                    + gains[2] * gains[2])
                    .sqrt();
                if !norm.is_finite() || norm <= GEOMETRY_EPSILON {
                    continue;
                }
                for gain in &mut gains {
                    *gain /= norm;
                }

                let quality = gains[0].min(gains[1]).min(gains[2]);
                let candidate = TripletCandidate {
                    indices: [first, second, third],
                    gains,
                    quality,
                };
                if candidate_precedes(candidate, best) {
                    best = Some(candidate);
                }
            }
        }
    }

    if let Some(candidate) = best {
        for (index, gain) in candidate.indices.into_iter().zip(candidate.gains) {
            if let Some(weight) = weights.get_mut(index) {
                *weight = finite_f32(gain);
            }
        }
        return;
    }

    nearest_spatial_direction(speakers, listener, source_direction, weights);
}

#[derive(Debug, Clone, Copy)]
struct TripletCandidate {
    indices: [usize; 3],
    gains: [f64; 3],
    quality: f64,
}

fn candidate_precedes(candidate: TripletCandidate, best: Option<TripletCandidate>) -> bool {
    match best {
        None => true,
        Some(best) => {
            candidate.quality > best.quality + QUALITY_EPSILON
                || ((candidate.quality - best.quality).abs() <= QUALITY_EPSILON
                    && candidate.indices < best.indices)
        }
    }
}

fn nearest_spatial_direction(
    speakers: &[Speaker],
    listener: Vector3,
    source_direction: [f64; 3],
    weights: &mut [f32],
) {
    let mut best = None::<(usize, f64)>;
    for (index, speaker) in speakers.iter().enumerate() {
        if !is_spatial_speaker(speaker) {
            continue;
        }
        let Some(direction) = normalize(vector_between(listener, speaker.position)) else {
            continue;
        };
        let score = dot(direction, source_direction);
        let replace = match best {
            None => true,
            Some((best_index, best_score)) => {
                score > best_score + QUALITY_EPSILON
                    || ((score - best_score).abs() <= QUALITY_EPSILON && index < best_index)
            }
        };
        if replace {
            best = Some((index, score));
        }
    }
    if let Some((index, _)) = best {
        if let Some(weight) = weights.get_mut(index) {
            *weight = 1.0;
        }
    }
}

fn equal_power_spatial(speakers: &[Speaker], weights: &mut [f32]) {
    let count = speakers
        .iter()
        .filter(|speaker| is_spatial_speaker(speaker))
        .count();
    if count == 0 {
        return;
    }
    let gain = (1.0 / count as f64).sqrt() as f32;
    for (speaker, weight) in speakers.iter().zip(weights.iter_mut()) {
        if is_spatial_speaker(speaker) {
            *weight = gain;
        }
    }
}

fn is_spatial_speaker(speaker: &Speaker) -> bool {
    !matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects)
}

fn vector_between(from: Vector3, to: Vector3) -> [f64; 3] {
    [
        to.x as f64 - from.x as f64,
        to.y as f64 - from.y as f64,
        to.z as f64 - from.z as f64,
    ]
}

fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(vector, vector).sqrt();
    if !length.is_finite() || length <= GEOMETRY_EPSILON {
        return None;
    }
    Some([
        vector[0] / length,
        vector[1] / length,
        vector[2] / length,
    ])
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn vector_is_finite(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

fn distance_f64(left: Vector3, right: Vector3) -> f64 {
    let dx = left.x as f64 - right.x as f64;
    let dy = left.y as f64 - right.y as f64;
    let dz = left.z as f64 - right.z as f64;
    dx.mul_add(dx, dy.mul_add(dy, dz * dz)).sqrt()
}

fn db_to_gain(db: f32) -> f64 {
    10.0_f64.powf(db as f64 / 20.0)
}

fn finite_f32(value: f64) -> f32 {
    if value.is_finite() {
        value.clamp(f32::MIN as f64, f32::MAX as f64) as f32
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speaker(id: &str, role: ChannelRole, x: f32, y: f32, z: f32) -> Speaker {
        Speaker {
            id: id.to_owned(),
            label: id.to_owned(),
            channel_role: role,
            position: Vector3::new(x, y, z),
            orientation: Vector3::new(0.0, -1.0, 0.0),
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

    fn five_one_two_layout() -> Vec<Speaker> {
        vec![
            speaker("FL", ChannelRole::FrontLeft, -1.0, 1.0, 0.0),
            speaker("FR", ChannelRole::FrontRight, 1.0, 1.0, 0.0),
            speaker("FC", ChannelRole::FrontCenter, 0.0, 1.0, 0.0),
            speaker(
                "LFE",
                ChannelRole::LowFrequencyEffects,
                0.0,
                0.5,
                0.0,
            ),
            speaker("SL", ChannelRole::SurroundLeft, -1.0, -1.0, 0.0),
            speaker("SR", ChannelRole::SurroundRight, 1.0, -1.0, 0.0),
            speaker("TFL", ChannelRole::TopFrontLeft, -0.8, 0.8, 1.2),
            speaker("TFR", ChannelRole::TopFrontRight, 0.8, 0.8, 1.2),
        ]
    }

    fn configured_renderer() -> (Vbap3dRenderer, RendererScratch) {
        let mut renderer = Vbap3dRenderer::new();
        renderer
            .configure(five_one_two_layout(), 48_000, 256, 4)
            .unwrap();
        let scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        (renderer, scratch)
    }

    #[test]
    fn overhead_probe_routes_energy_to_height_speakers() {
        let (mut renderer, mut scratch) = configured_renderer();
        let object = RenderObject {
            position: Vector3::new(0.0, 0.8, 1.5),
            gain: 1.0,
        };
        let mut output = vec![SpeakerGain::default(); renderer.output_channel_count()];
        renderer
            .render_gains(&listener(), &[object], &mut output, &mut scratch)
            .unwrap();

        let height_power = output[6].gain.powi(2) + output[7].gain.powi(2);
        let total_power: f32 = output.iter().map(|gain| gain.gain.powi(2)).sum();
        assert!(height_power > 0.05, "height energy must be non-zero: {output:?}");
        assert!((total_power - 1.0).abs() < 1.0e-4, "power={total_power}");
    }

    #[test]
    fn lfe_never_receives_spatial_vbap_energy() {
        let (mut renderer, mut scratch) = configured_renderer();
        let object = RenderObject {
            position: Vector3::new(0.2, 0.9, 0.7),
            gain: 1.0,
        };
        let mut output = vec![SpeakerGain::default(); renderer.output_channel_count()];
        renderer
            .render_gains(&listener(), &[object], &mut output, &mut scratch)
            .unwrap();
        assert_eq!(output[3].gain, 0.0);
    }

    #[test]
    fn source_at_listener_is_equal_power_across_spatial_outputs() {
        let (mut renderer, mut scratch) = configured_renderer();
        let object = RenderObject {
            position: Vector3::ZERO,
            gain: 1.0,
        };
        let mut output = vec![SpeakerGain::default(); renderer.output_channel_count()];
        renderer
            .render_gains(&listener(), &[object], &mut output, &mut scratch)
            .unwrap();
        let spatial: Vec<_> = output
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != 3)
            .map(|(_, gain)| gain.gain)
            .collect();
        for pair in spatial.windows(2) {
            assert!((pair[0] - pair[1]).abs() < 1.0e-6);
        }
        assert_eq!(output[3].gain, 0.0);
    }

    #[test]
    fn malformed_nonfinite_runtime_state_fails_to_finite_silence() {
        let (mut renderer, mut scratch) = configured_renderer();
        let object = RenderObject {
            position: Vector3::new(f32::NAN, 0.0, 1.0),
            gain: 1.0,
        };
        let mut output = vec![SpeakerGain::default(); renderer.output_channel_count()];
        renderer
            .render_gains(&listener(), &[object], &mut output, &mut scratch)
            .unwrap();
        assert!(output.iter().all(|gain| gain.gain == 0.0));
        assert!(output.iter().all(|gain| gain.gain.is_finite()));
    }

    #[test]
    fn render_path_reuses_fixed_scratch_capacity() {
        let (mut renderer, mut scratch) = configured_renderer();
        let capacity = scratch.float_capacity();
        let mut output = vec![SpeakerGain::default(); renderer.output_channel_count()];
        for step in 0..128 {
            let angle = step as f32 * 0.049;
            let object = RenderObject {
                position: Vector3::new(angle.sin(), angle.cos(), 0.8 + 0.4 * angle.sin()),
                gain: 1.0,
            };
            renderer
                .render_gains(&listener(), &[object], &mut output, &mut scratch)
                .unwrap();
            assert_eq!(scratch.float_capacity(), capacity);
            assert!(output.iter().all(|gain| gain.gain.is_finite()));
        }
    }

    #[test]
    fn fewer_than_three_spatial_speakers_are_rejected() {
        let mut renderer = Vbap3dRenderer::new();
        let layout = vec![
            speaker("FL", ChannelRole::FrontLeft, -1.0, 1.0, 0.0),
            speaker("FR", ChannelRole::FrontRight, 1.0, 1.0, 0.0),
            speaker(
                "LFE",
                ChannelRole::LowFrequencyEffects,
                0.0,
                0.5,
                0.0,
            ),
        ];
        assert!(matches!(
            renderer.configure(layout, 48_000, 256, 1),
            Err(RendererError::InvalidConfiguration(_))
        ));
    }
}
