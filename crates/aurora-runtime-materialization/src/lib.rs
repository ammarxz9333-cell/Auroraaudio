//! Control-thread materialization of concrete Aurora runtime components.
//!
//! `aurora-runtime-assembly` stays passive and deterministic; this crate owns
//! concrete implementation selection and construction before callback activation.

#![forbid(unsafe_code)]

use aurora_dsp_basic::DelayProcessor;
use aurora_realtime_engine::{RealTimeEngine, RealTimeEngineConfig, RealTimeEngineError};
use aurora_renderer_api::{ObjectPcmRenderer, Renderer, RendererCapabilities};
use aurora_renderer_basic::BasicRenderer;
pub use aurora_renderer_basic::BasicRendererMode;
use aurora_renderer_vbap::VbapRenderer;
pub use aurora_runtime_assembly::{
    BASIC_DELAY_IMPLEMENTATION_ID, BASIC_RENDERER_IMPLEMENTATION_ID,
    REALTIME_DELAY_CONTRACT_VERSION, REALTIME_RENDERER_CONTRACT_VERSION,
    VBAP_RENDERER_IMPLEMENTATION_ID,
};
use aurora_scene::RenderScene;

/// Concrete gain-renderer choices owned by control-thread materialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimeRendererSelection {
    /// Current basic geometric renderer with an explicit behavior mode.
    Basic(BasicRendererMode),
    /// Aurora's horizontal VBAP renderer.
    Vbap,
}

/// Materializes Aurora's current default realtime engine on the control thread.
///
/// The compatibility default remains the existing inverse-distance BasicRenderer path.
pub fn materialize_default_realtime_engine(
    scene: RenderScene,
    config: RealTimeEngineConfig,
    estimated_device_latency_frames: usize,
) -> Result<RealTimeEngine, RealTimeEngineError> {
    materialize_realtime_engine(
        scene,
        config,
        RealtimeRendererSelection::Basic(BasicRendererMode::InverseDistance),
        estimated_device_latency_frames,
    )
}

/// Materializes a realtime engine with an explicitly selected gain renderer.
pub fn materialize_realtime_engine(
    scene: RenderScene,
    config: RealTimeEngineConfig,
    renderer_selection: RealtimeRendererSelection,
    estimated_device_latency_frames: usize,
) -> Result<RealTimeEngine, RealTimeEngineError> {
    let speakers = scene.ordered_speakers()?;
    let mut renderer: Box<dyn Renderer> = match renderer_selection {
        RealtimeRendererSelection::Basic(mode) => {
            Box::new(BasicRenderer::new(mode).with_smoothing(0.35))
        }
        RealtimeRendererSelection::Vbap => Box::new(VbapRenderer::new().with_smoothing(0.35)),
    };
    renderer.configure(speakers, config.sample_rate, config.block_size, 1)?;
    let renderer_capabilities = renderer.capabilities();
    let requirements = RealTimeEngine::delay_requirements(&scene, &config, renderer_capabilities)?;
    let delay = DelayProcessor::new(
        requirements.channel_count(),
        requirements.max_delay_samples(),
    );
    RealTimeEngine::new_with_prepared_components(
        scene,
        config,
        estimated_device_latency_frames,
        renderer,
        Box::new(delay),
    )
}

/// Materializes a realtime engine around one object-PCM renderer.
///
/// The renderer is configured on the control thread and then activated through
/// the mutually exclusive PCM path in [`RealTimeEngine`]. The existing default
/// Basic/VBAP selection is unchanged.
pub fn materialize_realtime_engine_with_pcm_renderer(
    scene: RenderScene,
    config: RealTimeEngineConfig,
    mut renderer: Box<dyn ObjectPcmRenderer>,
    estimated_device_latency_frames: usize,
) -> Result<RealTimeEngine, RealTimeEngineError> {
    let speakers = scene.ordered_speakers()?;
    renderer.configure(speakers, config.sample_rate, config.block_size, 1)?;
    let requirements =
        RealTimeEngine::delay_requirements(&scene, &config, RendererCapabilities::default())?;
    let delay = DelayProcessor::new(
        requirements.channel_count(),
        requirements.max_delay_samples(),
    );
    RealTimeEngine::new_with_prepared_pcm_components(
        scene,
        config,
        estimated_device_latency_frames,
        renderer,
        Box::new(delay),
    )
}

#[cfg(test)]
mod tests {
    use aurora_dsp_api::{RealtimeDelayProcessor, RealtimeDspFault};
    use aurora_realtime_engine::{ProcessStatus, RealTimeEngineConfig, TestSignal};
    use aurora_renderer_api::{
        ObjectPcmBlock, PcmListener as Listener, PcmRendererError, PcmSpeaker as Speaker,
    };

    use super::*;

    #[derive(Debug)]
    struct AlternatePassThroughDelay {
        channels: usize,
        max_delay_samples: f32,
    }

    impl RealtimeDelayProcessor for AlternatePassThroughDelay {
        fn channel_count(&self) -> usize {
            self.channels
        }

        fn max_delay_samples(&self) -> f32 {
            self.max_delay_samples
        }

        fn set_delays(&mut self, delays_samples: &[f32]) -> Result<(), RealtimeDspFault> {
            if delays_samples.len() != self.channels {
                return Err(RealtimeDspFault::DelayShape);
            }
            if delays_samples
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0 || *value > self.max_delay_samples)
            {
                return Err(RealtimeDspFault::DelayValue);
            }
            Ok(())
        }

        fn process_planar(
            &mut self,
            input: &[Vec<f32>],
            output: &mut [Vec<f32>],
            frame_count: usize,
        ) -> Result<(), RealtimeDspFault> {
            if input.len() != self.channels
                || output.len() != self.channels
                || input.iter().any(|channel| channel.len() < frame_count)
                || output.iter().any(|channel| channel.len() < frame_count)
            {
                return Err(RealtimeDspFault::BufferShape);
            }
            for (source, destination) in input.iter().zip(output.iter_mut()) {
                destination[..frame_count].copy_from_slice(&source[..frame_count]);
            }
            Ok(())
        }

        fn reset(&mut self) {}

        fn latency_frames(&self) -> usize {
            0
        }
    }

    #[derive(Debug, Default)]
    struct TestPcmRenderer {
        channels: usize,
        block_size: usize,
    }

    impl ObjectPcmRenderer for TestPcmRenderer {
        fn configure(
            &mut self,
            layout: Vec<Speaker>,
            _sample_rate: u32,
            block_size: usize,
            _max_objects: usize,
        ) -> Result<(), PcmRendererError> {
            self.channels = layout.iter().filter(|speaker| speaker.enabled).count();
            self.block_size = block_size;
            Ok(())
        }

        fn render_pcm(
            &mut self,
            _listener: &Listener,
            objects: &[ObjectPcmBlock<'_>],
            output: &mut [Vec<f32>],
        ) -> Result<(), PcmRendererError> {
            if output.len() != self.channels {
                return Err(PcmRendererError::OutputChannelCount {
                    required: self.channels,
                    actual: output.len(),
                });
            }
            let Some(object) = objects.first() else {
                return Ok(());
            };
            if object.samples.len() != self.block_size {
                return Err(PcmRendererError::InputBlockSize {
                    object_index: 0,
                    required: self.block_size,
                    actual: object.samples.len(),
                });
            }
            for (index, channel) in output.iter_mut().enumerate() {
                if channel.len() != self.block_size {
                    return Err(PcmRendererError::OutputBlockSize {
                        channel: index,
                        required: self.block_size,
                        actual: channel.len(),
                    });
                }
                channel.fill(0.0);
            }
            if let Some(first) = output.first_mut() {
                first.copy_from_slice(object.samples);
            }
            Ok(())
        }

        fn reset(&mut self) {}

        fn latency_frames(&self) -> usize {
            23
        }

        fn output_channel_count(&self) -> usize {
            self.channels
        }
    }

    fn scene() -> RenderScene {
        serde_json::from_str(include_str!("../../../fixtures/scenes/stereo_circle.json"))
            .expect("fixture scene")
    }

    fn config() -> RealTimeEngineConfig {
        RealTimeEngineConfig {
            sample_rate: 48_000,
            block_size: 256,
            input_channels: 0,
            apply_geometric_delay: false,
            speed_of_sound: 343.0,
            test_signal: TestSignal::Sine,
        }
    }

    #[test]
    fn default_materialization_matches_explicit_basic_selection() {
        let mut default_engine =
            materialize_default_realtime_engine(scene(), config(), 256).unwrap();
        let mut explicit_engine = materialize_realtime_engine(
            scene(),
            config(),
            RealtimeRendererSelection::Basic(BasicRendererMode::InverseDistance),
            256,
        )
        .unwrap();
        let mut default_output = vec![0.0_f32; 512];
        let mut explicit_output = vec![0.0_f32; 512];
        for _ in 0..8 {
            assert_eq!(
                default_engine.process_interleaved(None, &mut default_output),
                ProcessStatus::Ok
            );
            assert_eq!(
                explicit_engine.process_interleaved(None, &mut explicit_output),
                ProcessStatus::Ok
            );
            assert_eq!(default_output, explicit_output);
        }
    }

    #[test]
    fn pcm_renderer_materializes_through_alternative_engine_path() {
        let mut engine = materialize_realtime_engine_with_pcm_renderer(
            scene(),
            config(),
            Box::new(TestPcmRenderer::default()),
            256,
        )
        .unwrap();
        assert_eq!(engine.metrics().renderer_latency_frames, 23);
        let mut output = vec![0.0_f32; 512];
        assert_eq!(
            engine.process_interleaved(None, &mut output),
            ProcessStatus::Ok
        );
        assert!(output.iter().any(|sample| sample.abs() > f32::EPSILON));
    }

    #[test]
    fn alternate_delay_adapter_uses_the_same_prepared_component_boundary() {
        let scene = scene();
        let config = config();
        let speakers = scene.ordered_speakers().unwrap();
        let mut renderer =
            BasicRenderer::new(BasicRendererMode::InverseDistance).with_smoothing(0.35);
        renderer
            .configure(speakers, config.sample_rate, config.block_size, 1)
            .unwrap();
        let requirements =
            RealTimeEngine::delay_requirements(&scene, &config, renderer.capabilities()).unwrap();
        let alternate = AlternatePassThroughDelay {
            channels: requirements.channel_count(),
            max_delay_samples: requirements.max_delay_samples(),
        };
        let mut engine = RealTimeEngine::new_with_prepared_components(
            scene,
            config,
            256,
            Box::new(renderer),
            Box::new(alternate),
        )
        .unwrap();
        let mut output = vec![0.0_f32; 512];
        assert_eq!(
            engine.process_interleaved(None, &mut output),
            ProcessStatus::Ok
        );
        assert!(output.iter().any(|sample| sample.abs() > f32::EPSILON));
    }

    #[test]
    fn failed_replacement_preparation_does_not_mutate_existing_engine() {
        let mut active = materialize_default_realtime_engine(scene(), config(), 256).unwrap();
        let mut before = vec![0.0_f32; 512];
        assert_eq!(
            active.process_interleaved(None, &mut before),
            ProcessStatus::Ok
        );
        let callbacks_before = active.metrics().callback_count;

        let replacement_scene = scene();
        let replacement_config = config();
        let speakers = replacement_scene.ordered_speakers().unwrap();
        let mut renderer =
            BasicRenderer::new(BasicRendererMode::InverseDistance).with_smoothing(0.35);
        renderer
            .configure(
                speakers,
                replacement_config.sample_rate,
                replacement_config.block_size,
                1,
            )
            .unwrap();
        let invalid_delay = AlternatePassThroughDelay {
            channels: 1,
            max_delay_samples: 0.0,
        };
        assert!(RealTimeEngine::new_with_prepared_components(
            replacement_scene,
            replacement_config,
            256,
            Box::new(renderer),
            Box::new(invalid_delay),
        )
        .is_err());

        let mut after = vec![0.0_f32; 512];
        assert_eq!(
            active.process_interleaved(None, &mut after),
            ProcessStatus::Ok
        );
        assert_eq!(active.metrics().callback_count, callbacks_before + 1);
        assert!(after.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn component_identity_constants_come_from_runtime_assembly() {
        assert_eq!(
            BASIC_RENDERER_IMPLEMENTATION_ID,
            "org.aurora.renderer.basic"
        );
        assert_eq!(VBAP_RENDERER_IMPLEMENTATION_ID, "org.aurora.renderer.vbap");
        assert_eq!(BASIC_DELAY_IMPLEMENTATION_ID, "org.aurora.dsp.basic-delay");
        assert_eq!(REALTIME_RENDERER_CONTRACT_VERSION, 1);
        assert_eq!(REALTIME_DELAY_CONTRACT_VERSION, 1);
    }

    #[test]
    fn basic_and_vbap_materialize_through_the_same_engine_boundary() {
        for selection in [
            RealtimeRendererSelection::Basic(BasicRendererMode::InverseDistance),
            RealtimeRendererSelection::Vbap,
        ] {
            let mut engine =
                materialize_realtime_engine(scene(), config(), selection, 256).unwrap();
            let mut output = vec![0.0_f32; 512];
            assert_eq!(
                engine.process_interleaved(None, &mut output),
                ProcessStatus::Ok
            );
            assert!(output.iter().all(|sample| sample.is_finite()));
        }
    }
}
