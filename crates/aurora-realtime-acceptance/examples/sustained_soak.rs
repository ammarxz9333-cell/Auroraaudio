use std::path::PathBuf;
use std::time::Instant;

use aurora_realtime_acceptance::{
    evaluate_realtime_acceptance, RealTimeAcceptancePolicy, RealTimeHealthReportV1,
};
use aurora_realtime_engine::{
    BasicRendererMode, ProcessStatus, RealTimeEngine, RealTimeEngineConfig, RealTimeFault,
    TestSignal,
};
use aurora_runtime_materialization::materialize_default_realtime_engine;
use aurora_scene::RenderScene;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_SIZE: usize = 256;
const DEFAULT_MEDIA_SECONDS: u64 = 600;
const VALIDATION_STRIDE_BLOCKS: u64 = 1_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let media_seconds = args
        .next()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(DEFAULT_MEDIA_SECONDS);
    let report_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("output/realtime-health-soak.json"));
    if args.next().is_some() {
        return Err("usage: sustained_soak [media_seconds] [report_path]".into());
    }
    if media_seconds == 0 {
        return Err("media_seconds must be greater than zero".into());
    }

    let scene: RenderScene = serde_json::from_str(include_str!(
        "../../../fixtures/scenes/7_1_4_reference.json"
    ))?;
    let mut engine = materialize_default_realtime_engine(
        scene,
        RealTimeEngineConfig {
            sample_rate: SAMPLE_RATE,
            block_size: BLOCK_SIZE,
            input_channels: 0,
            apply_geometric_delay: false,
            speed_of_sound: 343.0,
            test_signal: TestSignal::RotatingSine,
            renderer_mode: BasicRendererMode::InverseDistance,
        },
        0,
    )?;

    let channels = engine.output_roles().len();
    if channels != 12 {
        return Err(format!("expected 12 output channels, got {channels}").into());
    }

    let total_frames = u64::from(SAMPLE_RATE)
        .checked_mul(media_seconds)
        .ok_or("media duration overflow")?;
    if total_frames % BLOCK_SIZE as u64 != 0 {
        return Err("media duration must resolve to a whole callback count".into());
    }
    let callbacks = total_frames / BLOCK_SIZE as u64;
    let mut output = vec![0.0_f32; BLOCK_SIZE * channels];
    let started = Instant::now();
    let mut observed_peak = 0.0_f32;

    for callback_index in 0..callbacks {
        let status = engine.process_interleaved(None, &mut output);
        if status != ProcessStatus::Ok {
            return Err(format!(
                "callback {callback_index} failed with {status:?}; fault={:?}",
                engine.metrics().fault
            )
            .into());
        }
        if callback_index % VALIDATION_STRIDE_BLOCKS == 0 || callback_index + 1 == callbacks {
            for sample in &output {
                if !sample.is_finite() {
                    return Err(format!("non-finite output at callback {callback_index}").into());
                }
                observed_peak = observed_peak.max(sample.abs());
            }
        }
    }

    if observed_peak <= f32::EPSILON {
        return Err("sustained soak output remained silent".into());
    }

    let policy = RealTimeAcceptancePolicy {
        max_estimated_end_to_end_latency_frames: Some(BLOCK_SIZE * 4),
        ..RealTimeAcceptancePolicy::default()
    };
    let health = evaluate_realtime_acceptance(engine.metrics(), policy);
    if !health.accepted {
        return Err(format!("health acceptance failed: {:?}", health.violations).into());
    }
    if engine.metrics().callback_count != callbacks
        || engine.metrics().processed_blocks != callbacks
        || engine.metrics().input_underruns != 0
        || engine.metrics().output_underruns != 0
        || engine.metrics().dropped_blocks != 0
        || engine.metrics().fault != RealTimeFault::None
    {
        return Err(format!("unexpected final metrics: {:?}", engine.metrics()).into());
    }

    if let Some(parent) = report_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let document = RealTimeHealthReportV1::from_evaluation(engine.metrics(), policy, &health);
    std::fs::write(&report_path, document.to_pretty_json()?)?;

    let elapsed = started.elapsed().as_secs_f64();
    let speedup = media_seconds as f64 / elapsed.max(f64::EPSILON);
    println!(
        "AURORA REALTIME HEALTH SOAK PASS media_seconds={} callbacks={} channels={} wall_seconds={:.3} speedup={:.2}x p95_budget_usage_percent={:.3} peak={:.6} report={}",
        media_seconds,
        callbacks,
        channels,
        elapsed,
        speedup,
        health.p95_budget_usage_percent.unwrap_or_default(),
        observed_peak,
        report_path.display()
    );
    Ok(())
}
