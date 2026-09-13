use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use aurora_realtime_engine::{ProcessStatus, RealTimeEngineConfig, RealTimeFault, TestSignal};
use aurora_runtime_materialization::materialize_default_realtime_engine;
use aurora_scene::RenderScene;
use serde::Serialize;

const SAMPLE_RATE: u32 = 48_000;
const BLOCK_SIZE: usize = 256;
const DEFAULT_WALL_SECONDS: u64 = 30;
const MAX_RSS_GROWTH_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Serialize)]
struct WallClockSoakReport {
    schema_version: u32,
    accepted: bool,
    wall_seconds_requested: u64,
    wall_seconds_observed: f64,
    callback_period_microseconds: f64,
    callbacks: u64,
    channels: usize,
    processing_deadline_misses: u64,
    scheduler_late_callbacks: u64,
    maximum_processing_microseconds: f64,
    callback_busy_ratio: f64,
    observed_peak: f32,
    rss_start_bytes: Option<u64>,
    rss_max_bytes: Option<u64>,
    rss_final_bytes: Option<u64>,
    rss_growth_bytes: Option<i64>,
    engine_input_underruns: u64,
    engine_output_underruns: u64,
    engine_dropped_blocks: u64,
    engine_fault: String,
    violations: Vec<String>,
    truth_boundary: &'static str,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let wall_seconds = args
        .next()
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(DEFAULT_WALL_SECONDS);
    let report_path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("output/validation/realtime-wall-clock-soak-v1.json"));
    if args.next().is_some() {
        return Err("usage: wall_clock_soak [wall_seconds] [report_path]".into());
    }
    if wall_seconds == 0 {
        return Err("wall_seconds must be greater than zero".into());
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
        },
        0,
    )?;

    let channels = engine.output_roles().len();
    if channels != 12 {
        return Err(format!("expected 12 output channels, got {channels}").into());
    }

    let callback_period = Duration::from_secs_f64(BLOCK_SIZE as f64 / f64::from(SAMPLE_RATE));
    let requested_duration = Duration::from_secs(wall_seconds);
    let mut output = vec![0.0_f32; BLOCK_SIZE * channels];
    let started = Instant::now();
    let mut next_deadline = started;
    let mut callbacks = 0_u64;
    let mut processing_deadline_misses = 0_u64;
    let mut scheduler_late_callbacks = 0_u64;
    let mut maximum_processing = Duration::ZERO;
    let mut total_processing = Duration::ZERO;
    let mut observed_peak = 0.0_f32;
    let rss_start = resident_set_bytes();
    let mut rss_max = rss_start;
    let mut next_memory_sample = started;

    while started.elapsed() < requested_duration {
        let now = Instant::now();
        if now < next_deadline {
            thread::sleep(next_deadline - now);
        } else if callbacks > 0 && now.duration_since(next_deadline) >= callback_period {
            scheduler_late_callbacks = scheduler_late_callbacks.saturating_add(1);
        }

        let callback_started = Instant::now();
        let status = engine.process_interleaved(None, &mut output);
        let processing = callback_started.elapsed();
        total_processing += processing;
        maximum_processing = maximum_processing.max(processing);
        if processing > callback_period {
            processing_deadline_misses = processing_deadline_misses.saturating_add(1);
        }
        if status != ProcessStatus::Ok {
            return Err(format!(
                "wall-clock callback {callbacks} failed with {status:?}; fault={:?}",
                engine.metrics().fault
            )
            .into());
        }
        if output.iter().any(|sample| !sample.is_finite()) {
            return Err(format!("non-finite output at callback {callbacks}").into());
        }
        for sample in &output {
            observed_peak = observed_peak.max(sample.abs());
        }
        callbacks = callbacks.saturating_add(1);
        next_deadline += callback_period;

        if Instant::now() >= next_memory_sample {
            if let Some(rss) = resident_set_bytes() {
                rss_max = Some(rss_max.unwrap_or(rss).max(rss));
            }
            next_memory_sample += Duration::from_secs(1);
        }
    }

    let elapsed = started.elapsed();
    let rss_final = resident_set_bytes();
    if let Some(rss) = rss_final {
        rss_max = Some(rss_max.unwrap_or(rss).max(rss));
    }
    let rss_growth = match (rss_start, rss_final) {
        (Some(start), Some(final_rss)) => Some(final_rss as i64 - start as i64),
        _ => None,
    };

    let metrics = engine.metrics();
    let mut violations = Vec::new();
    if callbacks == 0 {
        violations.push("no callbacks executed".to_owned());
    }
    if observed_peak <= f32::EPSILON {
        violations.push("output remained silent".to_owned());
    }
    if processing_deadline_misses > 0 {
        violations.push(format!(
            "{processing_deadline_misses} callback processing durations exceeded the block deadline"
        ));
    }
    if metrics.input_underruns != 0 || metrics.output_underruns != 0 {
        violations.push(format!(
            "unexpected engine underruns: input={} output={}",
            metrics.input_underruns, metrics.output_underruns
        ));
    }
    if metrics.dropped_blocks != 0 {
        violations.push(format!(
            "unexpected dropped blocks: {}",
            metrics.dropped_blocks
        ));
    }
    if metrics.fault != RealTimeFault::None {
        violations.push(format!("engine fault: {:?}", metrics.fault));
    }
    if let (Some(start), Some(maximum)) = (rss_start, rss_max) {
        if maximum.saturating_sub(start) > MAX_RSS_GROWTH_BYTES {
            violations.push(format!(
                "RSS grew more than {} bytes: start={} max={}",
                MAX_RSS_GROWTH_BYTES, start, maximum
            ));
        }
    }

    let elapsed_seconds = elapsed.as_secs_f64();
    let report = WallClockSoakReport {
        schema_version: 1,
        accepted: violations.is_empty(),
        wall_seconds_requested: wall_seconds,
        wall_seconds_observed: elapsed_seconds,
        callback_period_microseconds: callback_period.as_secs_f64() * 1_000_000.0,
        callbacks,
        channels,
        processing_deadline_misses,
        scheduler_late_callbacks,
        maximum_processing_microseconds: maximum_processing.as_secs_f64() * 1_000_000.0,
        callback_busy_ratio: total_processing.as_secs_f64() / elapsed_seconds.max(f64::EPSILON),
        observed_peak,
        rss_start_bytes: rss_start,
        rss_max_bytes: rss_max,
        rss_final_bytes: rss_final,
        rss_growth_bytes: rss_growth,
        engine_input_underruns: metrics.input_underruns,
        engine_output_underruns: metrics.output_underruns,
        engine_dropped_blocks: metrics.dropped_blocks,
        engine_fault: format!("{:?}", metrics.fault),
        violations,
        truth_boundary: "Wall-clock software pacing and process-memory evidence only. This does not prove physical device scheduling, eARC/USB/TDM timing, DAC behavior, protected-service compatibility, or acoustics.",
    };

    if let Some(parent) = report_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&report_path, serde_json::to_string_pretty(&report)? + "\n")?;
    if !report.accepted {
        return Err(format!("wall-clock soak failed: {:?}", report.violations).into());
    }
    println!(
        "AURORA WALL CLOCK SOAK PASS wall_seconds={:.3} callbacks={} max_process_us={:.1} scheduler_late={} rss_growth={:?} report={}",
        report.wall_seconds_observed,
        report.callbacks,
        report.maximum_processing_microseconds,
        report.scheduler_late_callbacks,
        report.rss_growth_bytes,
        report_path.display()
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn resident_set_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kib = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    kib.checked_mul(1024)
}

#[cfg(not(target_os = "linux"))]
fn resident_set_bytes() -> Option<u64> {
    None
}
