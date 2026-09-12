//! Control-thread materialization of concrete Aurora runtime components.
//!
//! `aurora-runtime-assembly` stays passive and deterministic; this crate owns
//! concrete implementation selection and construction before callback activation.

#![forbid(unsafe_code)]

use aurora_dsp_basic::DelayProcessor;
use aurora_realtime_engine::{RealTimeEngine, RealTimeEngineConfig, RealTimeEngineError};
use aurora_scene::RenderScene;

/// Stable identity for Aurora's current default realtime delay implementation.
pub const BASIC_DELAY_IMPLEMENTATION_ID: &str = "org.aurora.dsp.basic-delay";

/// Materializes Aurora's current default realtime engine on the control thread.
pub fn materialize_default_realtime_engine(
    scene: RenderScene,
    config: RealTimeEngineConfig,
    estimated_device_latency_frames: usize,
) -> Result<RealTimeEngine, RealTimeEngineError> {
    let requirements = RealTimeEngine::delay_requirements(&scene, &config)?;
    let delay = DelayProcessor::new(
        requirements.channel_count(),
        requirements.max_delay_samples(),
    );
    RealTimeEngine::new_with_prepared_delay_processor(
        scene,
        config,
        estimated_device_latency_frames,
        Box::new(delay),
    )
}
