//! Object-semantic wrapper for the horizontal VBAP renderer.
//!
//! Object audio must not use an LFE channel as a directional panning target.
//! This adapter renders only against enabled non-LFE speakers, then maps the
//! results back into the configured enabled-speaker order while preserving an
//! explicit zero-gain LFE slot. All storage is allocated during configuration;
//! [`Renderer::render_gains`] performs no allocation.

use aurora_core::{ChannelRole, Listener, Speaker};
use aurora_renderer_api::{
    RenderObject, Renderer, RendererCapabilities, RendererError, RendererScratch,
    RendererScratchSize, SpeakerGain,
};

use crate::VbapRenderer;

/// Horizontal VBAP renderer with object-audio LFE semantics.
///
/// The wrapped [`VbapRenderer`] receives only enabled non-LFE speakers. Results
/// are remapped into the complete enabled output order, with every LFE output
/// retained as an explicit finite zero-gain slot. This matches the semantic
/// expectation that LFE is a separately authored/managed effects channel, not a
/// point-object panning destination.
#[derive(Debug, Clone)]
pub struct ObjectVbapRenderer {
    inner: VbapRenderer,
    directional_to_full: Vec<usize>,
    directional_gains: Vec<SpeakerGain>,
    full_channel_count: usize,
    max_objects: usize,
    configured: bool,
}

impl Default for ObjectVbapRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectVbapRenderer {
    /// Creates an unconfigured object-semantic VBAP renderer.
    pub fn new() -> Self {
        Self {
            inner: VbapRenderer::new(),
            directional_to_full: Vec::new(),
            directional_gains: Vec::new(),
            full_channel_count: 0,
            max_objects: 0,
            configured: false,
        }
    }

    /// Creates an unconfigured renderer with the wrapped VBAP smoothing value.
    pub fn with_smoothing(mut self, smoothing_alpha: f32) -> Self {
        self.inner = VbapRenderer::new().with_smoothing(smoothing_alpha);
        self
    }

    fn validate_output(
        &self,
        object_count: usize,
        output_count: usize,
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
        let required = object_count.saturating_mul(self.full_channel_count);
        if output_count != required {
            return Err(RendererError::OutputBufferSize {
                required,
                actual: output_count,
            });
        }
        Ok(())
    }
}

impl Renderer for ObjectVbapRenderer {
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
        let enabled: Vec<Speaker> = layout
            .into_iter()
            .filter(|speaker| speaker.enabled)
            .collect();
        if enabled.is_empty() {
            return Err(RendererError::NoEnabledSpeakers);
        }

        let mut directional = Vec::with_capacity(enabled.len());
        let mut directional_to_full = Vec::with_capacity(enabled.len());
        for (full_index, speaker) in enabled.iter().enumerate() {
            if speaker.channel_role != ChannelRole::LowFrequencyEffects {
                directional_to_full.push(full_index);
                directional.push(speaker.clone());
            }
        }
        if directional.is_empty() {
            return Err(RendererError::InvalidConfiguration(
                "object VBAP requires at least one enabled non-LFE speaker".to_owned(),
            ));
        }

        self.inner
            .configure(directional, sample_rate, block_size, max_objects)?;

        let directional_capacity = directional_to_full
            .len()
            .checked_mul(max_objects)
            .ok_or_else(|| {
                RendererError::InvalidConfiguration(
                    "directional speaker and object capacity product is too large".to_owned(),
                )
            })?;
        self.directional_to_full = directional_to_full;
        self.directional_gains = vec![SpeakerGain::default(); directional_capacity];
        self.full_channel_count = enabled.len();
        self.max_objects = max_objects;
        self.configured = true;
        Ok(())
    }

    fn required_scratch_size(&self) -> Result<RendererScratchSize, RendererError> {
        if !self.configured {
            return Err(RendererError::NotConfigured);
        }
        self.inner.required_scratch_size()
    }

    fn render_gains(
        &mut self,
        listener: &Listener,
        objects: &[RenderObject],
        output_gains: &mut [SpeakerGain],
        scratch: &mut RendererScratch,
    ) -> Result<(), RendererError> {
        self.validate_output(objects.len(), output_gains.len())?;

        let directional_count = self.directional_to_full.len();
        let directional_required = objects.len().saturating_mul(directional_count);
        let directional_available = self.directional_gains.len();
        let directional_output = self
            .directional_gains
            .get_mut(..directional_required)
            .ok_or(RendererError::OutputBufferSize {
                required: directional_required,
                actual: directional_available,
            })?;
        self.inner
            .render_gains(listener, objects, directional_output, scratch)?;

        for (object_index, full_output) in output_gains
            .chunks_exact_mut(self.full_channel_count)
            .enumerate()
        {
            for (speaker_index, slot) in full_output.iter_mut().enumerate() {
                *slot = SpeakerGain {
                    speaker_index,
                    gain: 0.0,
                    distance_meters: 0.0,
                    delay_samples: 0.0,
                };
            }

            let directional_start = object_index * directional_count;
            let directional_end = directional_start + directional_count;
            let directional_object = &directional_output[directional_start..directional_end];
            for (directional_index, &full_index) in self.directional_to_full.iter().enumerate() {
                let gain = directional_object[directional_index];
                full_output[full_index] = SpeakerGain {
                    speaker_index: full_index,
                    gain: gain.gain,
                    distance_meters: gain.distance_meters,
                    delay_samples: gain.delay_samples,
                };
            }
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.inner.reset();
        self.directional_gains.fill(SpeakerGain::default());
    }

    fn latency_frames(&self) -> usize {
        self.inner.latency_frames()
    }

    fn output_channel_count(&self) -> usize {
        self.full_channel_count
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::PI;

    use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
    use aurora_renderer_api::{
        RenderObject, Renderer, RendererError, RendererScratch, SpeakerGain,
    };

    use super::ObjectVbapRenderer;

    fn vector_from_azimuth(degrees: f32) -> Vector3 {
        let radians = degrees * PI / 180.0;
        Vector3::new(radians.cos(), radians.sin(), 0.0)
    }

    fn speaker(id: &str, role: ChannelRole, azimuth_degrees: f32) -> Speaker {
        Speaker {
            id: id.to_owned(),
            label: id.to_owned(),
            channel_role: role,
            position: vector_from_azimuth(azimuth_degrees),
            orientation: Vector3::ZERO,
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        }
    }

    fn five_one() -> Vec<Speaker> {
        vec![
            speaker("FL", ChannelRole::FrontLeft, -30.0),
            speaker("FR", ChannelRole::FrontRight, 30.0),
            speaker("FC", ChannelRole::FrontCenter, 0.0),
            speaker("LFE", ChannelRole::LowFrequencyEffects, 0.0),
            speaker("SL", ChannelRole::SurroundLeft, -110.0),
            speaker("SR", ChannelRole::SurroundRight, 110.0),
        ]
    }

    fn listener() -> Listener {
        Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(1.0, 0.0, 0.0),
            ear_height: 0.0,
        }
    }

    #[test]
    fn preserves_five_one_order_and_keeps_lfe_silent() {
        let mut renderer = ObjectVbapRenderer::new();
        renderer.configure(five_one(), 48_000, 256, 1).unwrap();
        assert_eq!(renderer.output_channel_count(), 6);

        let mut output = vec![SpeakerGain::default(); 6];
        let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        renderer
            .render_gains(
                &listener(),
                &[RenderObject {
                    position: vector_from_azimuth(-30.0),
                    gain: 1.0,
                }],
                &mut output,
                &mut scratch,
            )
            .unwrap();

        for (index, gain) in output.iter().enumerate() {
            assert_eq!(gain.speaker_index, index);
            assert!(gain.gain.is_finite());
        }
        assert!(output[0].gain > 0.999);
        assert_eq!(output[3].gain, 0.0);
    }

    #[test]
    fn routes_surround_object_without_lfe_leakage() {
        let mut renderer = ObjectVbapRenderer::new();
        renderer.configure(five_one(), 48_000, 256, 1).unwrap();
        let mut output = vec![SpeakerGain::default(); 6];
        let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        renderer
            .render_gains(
                &listener(),
                &[RenderObject {
                    position: vector_from_azimuth(110.0),
                    gain: 1.0,
                }],
                &mut output,
                &mut scratch,
            )
            .unwrap();

        assert!(output[5].gain > 0.999);
        assert_eq!(output[3].gain, 0.0);
    }

    #[test]
    fn warmed_up_object_render_allocates_zero_times() {
        let mut renderer = ObjectVbapRenderer::new();
        renderer.configure(five_one(), 48_000, 256, 1).unwrap();
        let listener = listener();
        let object = RenderObject {
            position: vector_from_azimuth(-30.0),
            gain: 1.0,
        };
        let mut output = vec![SpeakerGain::default(); 6];
        let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        renderer
            .render_gains(
                &listener,
                std::slice::from_ref(&object),
                &mut output,
                &mut scratch,
            )
            .unwrap();

        let output_capacity = output.capacity();
        let scratch_capacity = scratch.float_capacity();
        let allocations = crate::allocation_audit::count_allocations(|| {
            for _ in 0..1_000 {
                renderer
                    .render_gains(
                        &listener,
                        std::slice::from_ref(&object),
                        &mut output,
                        &mut scratch,
                    )
                    .unwrap();
            }
        });

        assert_eq!(allocations, 0);
        assert_eq!(output.capacity(), output_capacity);
        assert_eq!(scratch.float_capacity(), scratch_capacity);
        assert_eq!(output[3].gain, 0.0);
    }

    #[test]
    fn rejects_lfe_only_layout() {
        let mut renderer = ObjectVbapRenderer::new();
        let error = renderer
            .configure(
                vec![speaker("LFE", ChannelRole::LowFrequencyEffects, 0.0)],
                48_000,
                256,
                1,
            )
            .unwrap_err();
        assert!(matches!(error, RendererError::InvalidConfiguration(_)));
    }
}
