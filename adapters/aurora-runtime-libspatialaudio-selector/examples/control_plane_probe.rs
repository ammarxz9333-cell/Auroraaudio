use std::env;

use aurora_realtime_engine::{ProcessStatus, RealTimeEngineConfig, TestSignal};
use aurora_runtime_libspatialaudio_selector::{
    materialize_selected_libspatialaudio_engine, LibspatialaudioSelectionIntent,
    LIBSPATIALAUDIO_BLOCK_FRAMES, LIBSPATIALAUDIO_MEDIA_RATE_HZ,
    LIBSPATIALAUDIO_RENDERER_COMPONENT_ID,
};
use aurora_scene::RenderScene;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let shim = env::var("AURORA_LIBSPATIALAUDIO_SHIM")?;
    let selection = LibspatialaudioSelectionIntent::v1(shim);
    let serialized = serde_json::to_vec(&selection)?;
    let parsed = LibspatialaudioSelectionIntent::from_json(&serialized)?;

    let scene: RenderScene = serde_json::from_str(include_str!(
        "../../../fixtures/scenes/7_1_4_reference.json"
    ))?;
    let config = RealTimeEngineConfig {
        sample_rate: LIBSPATIALAUDIO_MEDIA_RATE_HZ,
        block_size: LIBSPATIALAUDIO_BLOCK_FRAMES,
        input_channels: 0,
        apply_geometric_delay: false,
        speed_of_sound: 343.0,
        test_signal: TestSignal::Sine,
    };
    let mut engine = materialize_selected_libspatialaudio_engine(&parsed, scene, config, 0)?;
    if engine.metrics().renderer_latency_frames != 255 {
        return Err(format!(
            "unexpected renderer latency {}",
            engine.metrics().renderer_latency_frames
        )
        .into());
    }

    let mut output = vec![0.0_f32; 12 * LIBSPATIALAUDIO_BLOCK_FRAMES];
    let mut peak = 0.0_f32;
    for _ in 0..4 {
        if engine.process_interleaved(None, &mut output) != ProcessStatus::Ok {
            return Err("realtime callback did not complete successfully".into());
        }
        if output.iter().any(|sample| !sample.is_finite()) {
            return Err("realtime output contains non-finite samples".into());
        }
        peak = output
            .iter()
            .map(|sample| sample.abs())
            .fold(peak, f32::max);
    }
    if peak <= f32::EPSILON {
        return Err("selected libspatialaudio path produced silent output".into());
    }

    println!(
        "aurora-libspatialaudio-control-plane: PASS component={} rate={} block={} layout=7.1.4 latency=255 callbacks={} peak={:.8}",
        LIBSPATIALAUDIO_RENDERER_COMPONENT_ID,
        LIBSPATIALAUDIO_MEDIA_RATE_HZ,
        LIBSPATIALAUDIO_BLOCK_FRAMES,
        engine.metrics().callback_count,
        peak,
    );
    Ok(())
}
