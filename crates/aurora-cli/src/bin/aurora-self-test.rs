//! Hardware-free validation of the exact shared S6 output processor.
use anyhow::{bail, Context, Result};
use aurora_core::StandardLayout;
use aurora_dsp_basic::output::{OutputDspConfig, SpeakerPostProcessor, CHANNELS, SAMPLE_RATE};
use clap::Parser;
use serde::Serialize;
use std::{fs, path::PathBuf, time::Instant};

#[derive(Parser)]
#[command(
    about = "Validate the shared cinema DSP and export twelve-channel routing evidence without opening audio hardware"
)]
struct Cli {
    #[arg(long, default_value = "output/self-test")]
    output_dir: PathBuf,
    #[arg(long)]
    calibration: Option<PathBuf>,
}

#[derive(Serialize)]
struct ChannelResult {
    role: String,
    peak: f32,
    rms: f64,
    unintended_spatial_peak: f32,
    passed: bool,
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    passed: bool,
    evidence: &'static str,
    sample_rate: u32,
    block_frames: usize,
    revision: String,
    build_profile: &'static str,
    channels: Vec<ChannelResult>,
    host_processing_us_p50: f64,
    host_processing_us_p99: f64,
    host_processing_us_max: f64,
    timing_boundary: &'static str,
    source_calibration: Option<PathBuf>,
}

fn run(cli: Cli) -> Result<()> {
    const FRAMES: usize = 12_000;
    const QUANTUM: usize = 40;
    let mut processor = SpeakerPostProcessor::new(OutputDspConfig::default())?;
    if let Some(path) = &cli.calibration {
        processor.configure_calibration(&serde_json::from_slice(&fs::read(path)?)?)?;
    }
    let mut wav = (0..CHANNELS)
        .map(|_| Vec::with_capacity(FRAMES * CHANNELS))
        .collect::<Vec<_>>();
    let mut results = Vec::with_capacity(CHANNELS);
    let mut timings = Vec::with_capacity(FRAMES / QUANTUM * CHANNELS);
    let roles = StandardLayout::SevenOneFour.canonical_roles();
    for (active, role) in roles.iter().enumerate() {
        processor.reset();
        let mut peak = 0.0_f32;
        let mut power = 0.0_f64;
        let mut unintended = 0.0_f32;
        let mut finite = true;
        for offset in (0..FRAMES).step_by(QUANTUM) {
            let mut block = [0.0_f32; QUANTUM * CHANNELS];
            for (frame, samples) in block.chunks_exact_mut(CHANNELS).enumerate() {
                let frequency = if active == 3 { 60.0 } else { 1000.0 };
                let elapsed = (offset + frame) as f32 / SAMPLE_RATE as f32;
                // Fade source endpoints to keep exported routing tones comfortable.
                let fade = (elapsed / 0.005)
                    .min((0.25 - elapsed) / 0.005)
                    .clamp(0.0, 1.0);
                samples[active] = 0.1 * fade * (std::f32::consts::TAU * frequency * elapsed).sin();
            }
            let start = Instant::now();
            processor.process_block(&mut block)?;
            timings.push(start.elapsed().as_secs_f64() * 1_000_000.0);
            for samples in block.chunks_exact(CHANNELS) {
                for (channel, &sample) in samples.iter().enumerate() {
                    finite &= sample.is_finite();
                    if channel != active && channel != 3 {
                        unintended = unintended.max(sample.abs());
                    }
                    wav[channel].push(sample);
                }
                peak = peak.max(samples[active].abs());
                power += f64::from(samples[active]).powi(2);
            }
        }
        results.push(ChannelResult {
            role: role.as_str().to_owned(),
            peak,
            rms: (power / FRAMES as f64).sqrt(),
            unintended_spatial_peak: unintended,
            passed: finite
                && peak > 0.0001
                && peak <= 10.0_f32.powf(-1.0 / 20.0) + 1e-6
                && unintended < 1e-7,
        });
    }
    timings.sort_by(f64::total_cmp);
    let passed = results.iter().all(|result| result.passed);
    let report = Report {
        schema_version: 1, passed, evidence: "host-software-only",
        sample_rate: SAMPLE_RATE, block_frames: QUANTUM,
        revision: std::env::var("GITHUB_SHA").unwrap_or_else(|_| "unrecorded-local-build".into()),
        build_profile: if cfg!(debug_assertions) { "debug" } else { "release" },
        channels: results,
        host_processing_us_p50: timings[timings.len() / 2],
        host_processing_us_p99: timings[timings.len() * 99 / 100],
        host_processing_us_max: *timings.last().unwrap(),
        timing_boundary: "Host wall-clock DSP timing only; excludes decoding, ASRC, USB and hardware; not S6 performance or measured audio latency",
        source_calibration: cli.calibration,
    };
    fs::create_dir_all(&cli.output_dir)?;
    aurora_audio_io::write_wav_f32_with_channel_roles(
        cli.output_dir.join("channel-test.wav"),
        SAMPLE_RATE,
        &wav,
        roles,
    )?;
    fs::write(
        cli.output_dir.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("shared-output-dsp passed={passed} channels=12 evidence=host-software-only");
    if !passed {
        bail!("one or more output channel gates failed; inspect report.json");
    }
    Ok(())
}

fn main() -> Result<()> {
    run(Cli::parse()).context("Aurora software self-test")
}
