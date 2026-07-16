use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use aurora_core::StandardLayout;
use aurora_realtime_audio_sim::{
    builtin_profile, load_fault_timeline, run_duplex_simulation, simulate_latency,
    validate_output_routing, DuplexSimulationConfig, FaultAction, FaultEvent,
    LatencySimulationConfig,
};

pub struct SimulateDuplexOptions {
    pub profile: String,
    pub duration_hours: u64,
    pub seed: u64,
    pub report: PathBuf,
    pub input_ppm: Option<i32>,
    pub output_ppm: Option<i32>,
    pub callback_jitter_frames: Option<usize>,
    pub device_latency_frames: Option<usize>,
    pub loss_at_seconds: Option<u64>,
    pub sample_rate: Option<u32>,
    pub block_size: usize,
    pub channels: Option<usize>,
    pub fault_script: Option<PathBuf>,
}

pub fn simulate_duplex(options: SimulateDuplexOptions) -> Result<()> {
    let profile = builtin_profile(&options.profile)
        .with_context(|| format!("unknown simulation profile: {}", options.profile))?;
    let mut faults = options.loss_at_seconds.map_or_else(Vec::new, |seconds| {
        vec![FaultEvent {
            at_milliseconds: seconds.saturating_mul(1_000),
            action: FaultAction::OutputLoss,
            duration_milliseconds: 5_000,
            value: 0,
        }]
    });
    if let Some(path) = options.fault_script {
        faults.extend(load_fault_timeline(path).context("load simulation fault timeline")?);
    }
    let report = run_duplex_simulation(&DuplexSimulationConfig {
        profile,
        duration_seconds: options.duration_hours.saturating_mul(60 * 60),
        seed: options.seed,
        input_ppm: options.input_ppm,
        output_ppm: options.output_ppm,
        callback_jitter_frames: options.callback_jitter_frames,
        device_latency_frames: options.device_latency_frames,
        sample_rate: options.sample_rate,
        block_size: options.block_size,
        channels: options.channels,
        faults,
    })?;
    write_json(&options.report, &report)?;
    println!("result_source={}", report.source);
    println!("profile={}", report.profile);
    println!(
        "simulated_duration_seconds={}",
        report.simulated_duration_seconds
    );
    println!(
        "execution_duration_seconds={:.6}",
        report.execution_duration_seconds
    );
    println!("acceleration_factor={:.3}", report.acceleration_factor);
    println!("input_callbacks={}", report.input_callbacks);
    println!("output_callbacks={}", report.output_callbacks);
    println!("frames_captured={}", report.frames_captured);
    println!("frames_rendered={}", report.frames_rendered);
    println!("ring_fill_min={:.3}", report.ring_fill_minimum);
    println!("ring_fill_max={:.3}", report.ring_fill_maximum);
    println!("ring_fill_current={:.3}", report.ring_fill_current);
    println!("asrc_ratio_min={:.9}", report.asrc_ratio_minimum);
    println!("asrc_ratio_max={:.9}", report.asrc_ratio_maximum);
    println!("asrc_ratio_average={:.9}", report.asrc_ratio_average);
    println!(
        "estimated_correction_ppm={:.3}",
        report.estimated_correction_ppm
    );
    println!(
        "controller_saturation={}",
        report.controller_saturation_count
    );
    println!("underruns={}", report.underruns);
    println!("overflows={}", report.overflows);
    println!("state={}", report.final_state);
    println!("bounded={}", report.bounded);
    println!(
        "sample_pipeline_probe_callbacks={}",
        report.sample_pipeline_probe_callbacks
    );
    println!(
        "sample_pipeline_probe_finite={}",
        report.sample_pipeline_probe_finite
    );
    println!(
        "sample_pipeline_checksum={}",
        report.sample_pipeline_checksum
    );
    println!("deterministic_checksum={}", report.deterministic_checksum);
    println!("report={}", options.report.display());
    Ok(())
}

pub fn simulate_latency_command(
    profile: String,
    loopback_delay_frames: usize,
    jitter_frames: usize,
    noise_db: f32,
    seed: u64,
) -> Result<()> {
    let profile = builtin_profile(&profile).context("unknown simulation profile")?;
    let sample_rate = profile.output.supported_sample_rates[0];
    let report = simulate_latency(LatencySimulationConfig {
        loopback_delay_frames,
        jitter_frames,
        noise_db,
        attenuation_db: -3.0,
        polarity_inverted: false,
        low_pass_alpha: None,
        seed,
        sample_rate,
    })?;
    println!("result_source={}", report.source);
    println!("true_delay_frames={}", report.true_delay_frames);
    println!(
        "estimated_median_frames={:.3}",
        report.estimated_median_frames
    );
    println!("minimum_frames={}", report.minimum_frames);
    println!("maximum_frames={}", report.maximum_frames);
    println!("absolute_error_frames={:.3}", report.absolute_error_frames);
    println!("jitter_estimate_frames={:.3}", report.jitter_frames);
    println!("confidence={:.6}", report.confidence);
    println!("valid_measurements={}", report.valid_measurements);
    println!("pass={}", report.passed);
    Ok(())
}

pub fn simulate_output_validation(
    profile: String,
    layout: String,
    report_path: PathBuf,
) -> Result<()> {
    let profile = builtin_profile(&profile).context("unknown simulation profile")?;
    let layout = match layout.as_str() {
        "stereo" => StandardLayout::Stereo,
        "5.1" => StandardLayout::FiveOne,
        "7.1" => StandardLayout::SevenOne,
        "5.1.2" => StandardLayout::FiveOneTwo,
        _ => bail!("unsupported simulation layout: {layout}"),
    };
    let report = validate_output_routing(&profile, layout)?;
    write_json(&report_path, &report)?;
    println!("result_source={}", report.source);
    println!("profile={}", report.profile);
    println!("layout={}", report.layout);
    for (index, role) in report.canonical_roles.iter().enumerate() {
        println!("device_channel_index={index} channel_role={role}");
    }
    println!("unique_output_routing={}", report.unique_output_routing);
    println!(
        "inactive_channels_silent={}",
        report.inactive_channels_silent
    );
    println!("maximum_gain_error={:.9}", report.maximum_gain_error);
    println!("polarity_consistent={}", report.polarity_consistent);
    if let Some(note) = &report.metadata_note {
        println!("metadata_note={note}");
    }
    println!("pass={}", report.passed);
    println!("deterministic_checksum={}", report.deterministic_checksum);
    println!("report={}", report_path.display());
    Ok(())
}

fn write_json(path: &PathBuf, value: &impl serde::Serialize) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).context("create simulation report directory")?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(value)?).context("write simulation report")
}
