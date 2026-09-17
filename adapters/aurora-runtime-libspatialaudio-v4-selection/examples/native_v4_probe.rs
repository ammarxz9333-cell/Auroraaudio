use std::env;

use aurora_config::{
    AuroraConfigurationV4, CompatibleMinorRange, ComponentContractKind, ComponentReference,
    ValidatedConfigurationV4,
};
use aurora_realtime_engine::{ProcessStatus, RealTimeEngineConfig, TestSignal};
use aurora_runtime_assembly::PreparedRendererKind;
use aurora_runtime_libspatialaudio_selector::{
    LIBSPATIALAUDIO_BLOCK_FRAMES, LIBSPATIALAUDIO_MEDIA_RATE_HZ,
    LIBSPATIALAUDIO_RENDERER_COMPONENT_ID, LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION,
    OBJECT_PCM_RENDERER_CONTRACT_MAJOR, OBJECT_PCM_RENDERER_CONTRACT_MINOR,
};
use aurora_runtime_libspatialaudio_v4_selection::{
    materialize_libspatialaudio_engine_from_prepared_plan,
    prepare_runtime_plan_from_configuration_v4,
};
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

    let runtime_plan = prepare_runtime_plan_from_configuration_v4(&configuration)?;
    let prepared_renderer = runtime_plan.execution().renderer();
    let prepared_identity = prepared_renderer.component_identity();
    if prepared_renderer.kind() != PreparedRendererKind::ExternalObjectPcm {
        return Err("native v4 runtime plan did not preserve external Object-PCM execution".into());
    }
    if prepared_identity.implementation_id() != LIBSPATIALAUDIO_RENDERER_COMPONENT_ID
        || prepared_identity.implementation_version()
            != LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION
        || prepared_identity.contract_major() != OBJECT_PCM_RENDERER_CONTRACT_MAJOR
        || prepared_identity.contract_minor() != OBJECT_PCM_RENDERER_CONTRACT_MINOR
    {
        return Err(
            "native v4 runtime plan did not preserve exact libspatialaudio identity".into(),
        );
    }
    if runtime_plan
        .execution()
        .audio_format()
        .output_channel_count()
        != 12
        || runtime_plan.execution().audio_format().sample_rate() != LIBSPATIALAUDIO_MEDIA_RATE_HZ
        || runtime_plan.execution().audio_format().callback_frames() as usize
            != LIBSPATIALAUDIO_BLOCK_FRAMES
    {
        return Err("native v4 runtime plan changed the proven media contract".into());
    }

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
    let mut engine = materialize_libspatialaudio_engine_from_prepared_plan(
        &runtime_plan,
        shim.as_str(),
        scene.clone(),
        engine_config.clone(),
        0,
    )?;

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
        "aurora-libspatialaudio-v4-selection: PASS component={} rate={} block={} layout=7.1.4 latency=255 callbacks={} peak={:.8} plan=external_object_pcm driver=prepared_plan",
        LIBSPATIALAUDIO_RENDERER_COMPONENT_ID,
        LIBSPATIALAUDIO_MEDIA_RATE_HZ,
        LIBSPATIALAUDIO_BLOCK_FRAMES,
        engine.metrics().callback_count,
        peak,
    );
    // Host-provided PCM uses the same prepared-plan path. The current engine
    // contract is one mono source with scene metadata, not decoded object beds.
    let mut input_config = engine_config;
    input_config.input_channels = 1;
    input_config.test_signal = TestSignal::None;
    let mut batched = materialize_libspatialaudio_engine_from_prepared_plan(
        &runtime_plan,
        shim.as_str(),
        scene.clone(),
        input_config.clone(),
        0,
    )?;
    let mut separate = materialize_libspatialaudio_engine_from_prepared_plan(
        &runtime_plan,
        shim.as_str(),
        scene,
        input_config,
        0,
    )?;
    let frames = LIBSPATIALAUDIO_BLOCK_FRAMES * 4;
    let input: Vec<f32> = (0..frames)
        .map(|frame| ((frame * 37 % 257) as f32 - 128.0) / 1024.0)
        .collect();
    let mut actual = vec![0.0; frames * 12];
    let mut expected = vec![0.0; frames * 12];
    if batched.process_interleaved(Some(&input), &mut actual) != ProcessStatus::Ok {
        return Err("native external PCM batched callback failed".into());
    }
    for (source, target) in input
        .chunks_exact(LIBSPATIALAUDIO_BLOCK_FRAMES)
        .zip(expected.chunks_exact_mut(LIBSPATIALAUDIO_BLOCK_FRAMES * 12))
    {
        if separate.process_interleaved(Some(source), target) != ProcessStatus::Ok {
            return Err("native external PCM single callback failed".into());
        }
    }
    if actual
        .iter()
        .chain(expected.iter())
        .any(|sample| !sample.is_finite())
    {
        return Err("native external PCM output is non-finite".into());
    }
    let max_delta = actual
        .iter()
        .zip(&expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    let input_peak = actual
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0_f32, f32::max);
    if max_delta > 1.0e-6 || input_peak <= f32::EPSILON {
        return Err(format!(
            "native external PCM continuity failed: delta={max_delta} peak={input_peak}"
        )
        .into());
    }
    println!("aurora-libspatialaudio-input-continuity: PASS source=host_mono_pcm frames={frames} channels=12 batched_callbacks=1 separate_callbacks=4 max_delta={max_delta:.8} peak={input_peak:.8}");
    Ok(())
}
