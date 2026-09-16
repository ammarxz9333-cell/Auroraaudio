use std::env;

use aurora_config::{
    AuroraConfigurationV4, CompatibleMinorRange, ComponentContractKind, ComponentReference,
    ValidatedConfigurationV4,
};
use aurora_realtime_engine::{ProcessStatus, RealTimeEngineConfig, TestSignal};
use aurora_runtime_libspatialaudio_selector::{
    materialize_selected_libspatialaudio_engine, LIBSPATIALAUDIO_BLOCK_FRAMES,
    LIBSPATIALAUDIO_MEDIA_RATE_HZ, LIBSPATIALAUDIO_RENDERER_COMPONENT_ID,
    LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION, OBJECT_PCM_RENDERER_CONTRACT_MAJOR,
    OBJECT_PCM_RENDERER_CONTRACT_MINOR,
};
use aurora_runtime_libspatialaudio_v4_selection::selection_from_configuration_v4;
use aurora_scene::RenderScene;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let shim = env::var("AURORA_LIBSPATIALAUDIO_SHIM")?;

    let mut raw: AuroraConfigurationV4 = serde_json::from_slice(include_bytes!(
        "../../../fixtures/config/surround-7-1-4-v4.json"
    ))?;
    raw.renderer = ComponentReference {
        component_id: LIBSPATIALAUDIO_RENDERER_COMPONENT_ID.to_owned(),
        contract_kind: ComponentContractKind::Renderer,
        contract_major: OBJECT_PCM_RENDERER_CONTRACT_MAJOR,
        compatible_minor: CompatibleMinorRange {
            minimum: OBJECT_PCM_RENDERER_CONTRACT_MINOR,
            maximum: OBJECT_PCM_RENDERER_CONTRACT_MINOR,
        },
        implementation_version_pin: Some(
            LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION.to_owned(),
        ),
        configuration_schema: 1,
        configuration: serde_json::json!({}),
    };
    let configuration = ValidatedConfigurationV4::new(raw)?;
    let selection = selection_from_configuration_v4(&configuration, shim)?;

    let scene: RenderScene = serde_json::from_str(include_str!(
        "../../../fixtures/scenes/7_1_4_reference.json"
    ))?;
    let engine_config = RealTimeEngineConfig {
        sample_rate: LIBSPATIALAUDIO_MEDIA_RATE_HZ,
        block_size: LIBSPATIALAUDIO_BLOCK_FRAMES,
        input_channels: 0,
        apply_geometric_delay: false,
        speed_of_sound: 343.0,
        test_signal: TestSignal::Sine,
    };
    let mut engine =
        materialize_selected_libspatialaudio_engine(&selection, scene, engine_config, 0)?;

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
            return Err("native v4 realtime callback did not complete successfully".into());
        }
        if output.iter().any(|sample| !sample.is_finite()) {
            return Err("native v4 realtime output contains non-finite samples".into());
        }
        peak = output
            .iter()
            .map(|sample| sample.abs())
            .fold(peak, f32::max);
    }
    if peak <= f32::EPSILON {
        return Err("native v4 libspatialaudio path produced silent output".into());
    }

    println!(
        "aurora-libspatialaudio-v4-selection: PASS component={} rate={} block={} layout=7.1.4 latency=255 callbacks={} peak={:.8}",
        LIBSPATIALAUDIO_RENDERER_COMPONENT_ID,
        LIBSPATIALAUDIO_MEDIA_RATE_HZ,
        LIBSPATIALAUDIO_BLOCK_FRAMES,
        engine.metrics().callback_count,
        peak,
    );
    Ok(())
}
