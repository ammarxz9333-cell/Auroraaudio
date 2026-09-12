//! Control-thread materialization of concrete Aurora runtime components.
//!
//! `aurora-runtime-assembly` stays passive and deterministic; this crate owns
//! concrete implementation selection and construction before callback activation.

#![forbid(unsafe_code)]

use aurora_dsp_basic::DelayProcessor;
use aurora_realtime_engine::{RealTimeEngine, RealTimeEngineConfig, RealTimeEngineError};
use aurora_renderer_api::Renderer;
use aurora_renderer_basic::BasicRenderer;
pub use aurora_renderer_basic::BasicRendererMode;
use aurora_renderer_vbap::VbapRenderer;
use aurora_scene::RenderScene;

/// Stable identity for Aurora's current default realtime delay implementation.
pub const BASIC_DELAY_IMPLEMENTATION_ID: &str = "org.aurora.dsp.basic-delay";
/// Stable identity for Aurora's basic geometric renderer implementation.
pub const BASIC_RENDERER_IMPLEMENTATION_ID: &str = "org.aurora.renderer.basic";
/// Stable identity for Aurora's horizontal VBAP renderer implementation.
pub const VBAP_RENDERER_IMPLEMENTATION_ID: &str = "org.aurora.renderer.vbap";

/// Concrete renderer choices owned by control-thread materialization.
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

/// Materializes a realtime engine with an explicitly selected renderer implementation.
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

#[cfg(test)]
mod tests {
    use aurora_realtime_engine::{ProcessStatus, RealTimeEngineConfig, TestSignal};

    use super::*;

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
