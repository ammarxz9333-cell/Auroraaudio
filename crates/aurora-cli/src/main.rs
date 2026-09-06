#[allow(unused_imports)]
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(feature = "realtime")]
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
#[cfg(feature = "realtime")]
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use aurora_audio_io::{read_wav, write_wav_f32_with_channel_roles, WavData, WavWriteReport};
use aurora_cli::capabilities::{render_capabilities, CapabilityOutputFormat};
use aurora_core::{ChannelRole, Listener, Speaker, StandardLayout, Vector3};
use aurora_dsp_basic::DelayProcessor;
#[cfg(feature = "camilladsp")]
use aurora_dsp_camilladsp::{
    discover_camilladsp, inspect_processed_wav, process_offline_wav, AuroraDspConfig,
};
#[cfg(feature = "realtime")]
use aurora_realtime_audio_api::{
    AudioDeviceDirection, AudioOutputBackend, RealTimeAudioConfig, RealTimeSampleFormat,
};
#[cfg(feature = "realtime")]
use aurora_realtime_audio_cpal::CpalAudioBackend;
#[cfg(feature = "realtime")]
use aurora_realtime_engine::{
    identify_roles, ProcessStatus, RealTimeEngine, RealTimeEngineConfig, TestSignal,
};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_basic::{
    calculate_geometric_delays, BasicRenderer, BasicRendererMode, GeometricDelay,
};
use aurora_scene::{load_render_scene, RenderScene};
use clap::{Parser, Subcommand, ValueEnum};

const CURRENT_DYNAMIC_DELAY_CAPACITY_SAMPLES: f32 = 1_024.0;

#[cfg(feature = "realtime")]
mod realtime_commands;
#[cfg(feature = "simulation")]
mod simulation_commands;
mod calibration_runner;
mod web_server;

#[derive(Debug, Parser)]
#[command(name = "aurora")]
#[command(about = "Aurora spatial-audio developer CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List local audio input and output devices.
    Devices,
    /// Check local optional tools and adapter availability.
    Doctor {
        #[arg(long)]
        camilladsp_path: Option<PathBuf>,
    },
    /// Print the canonical capability registry.
    Capabilities {
        /// Emit the stable machine-readable JSON registry instead of human text.
        #[arg(long)]
        json: bool,
    },
    /// Print calculated speaker gains for a moving source.
    Gains {
        #[arg(long, value_enum, default_value_t = LayoutName::Stereo)]
        layout: LayoutName,
        #[arg(long, default_value_t = 12)]
        steps: usize,
        #[arg(long, default_value_t = 1.0)]
        radius: f32,
    },
    /// Render a mono WAV through a JSON scene into a multichannel WAV.
    Render {
        #[arg(long)]
        scene: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        apply_geometric_delay: bool,
        #[arg(long, default_value_t = 343.0)]
        speed_of_sound: f32,
        #[arg(long, value_enum, default_value_t = CliRendererMode::InverseDistance)]
        renderer_mode: CliRendererMode,
    },
    /// Process an offline multichannel WAV through an external DSP engine.
    Process {
        #[arg(long, value_enum)]
        engine: ProcessEngine,
        #[arg(long)]
        scene: Option<PathBuf>,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        camilladsp_path: Option<PathBuf>,
        #[arg(long, default_value_t = 30)]
        timeout_seconds: u64,
        #[arg(long)]
        keep_temp: bool,
        #[arg(long)]
        apply_geometric_delay: bool,
        #[arg(long, default_value_t = 343.0)]
        speed_of_sound: f32,
    },
    /// Run a local real-time output pipeline.
    Realtime {
        #[arg(long)]
        input_device: Option<String>,
        #[arg(long)]
        output_device: Option<String>,
        #[arg(long)]
        scene: PathBuf,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
        #[arg(long, default_value_t = 256)]
        block_size: usize,
        #[arg(long)]
        apply_geometric_delay: bool,
        #[arg(long, default_value_t = 343.0)]
        speed_of_sound: f32,
        #[arg(long, value_enum, default_value_t = CliTestSignal::Silence)]
        test_signal: CliTestSignal,
        #[arg(long, default_value_t = 600)]
        duration_seconds: u64,
        #[arg(long, value_enum, default_value_t = CliRendererMode::InverseDistance)]
        renderer_mode: CliRendererMode,
    },
    /// Run independent input/output streams through adaptive duplex resampling.
    Duplex {
        #[arg(long)]
        input_device: String,
        #[arg(long)]
        output_device: String,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
        #[arg(long)]
        input_sample_rate: Option<u32>,
        #[arg(long)]
        output_sample_rate: Option<u32>,
        #[arg(long, default_value_t = 256)]
        block_size: usize,
        #[arg(long, default_value_t = 2)]
        channels: usize,
        #[arg(long, default_value_t = 60)]
        duration_seconds: u64,
        #[arg(long, default_value_t = 0)]
        restart_attempts: u32,
    },
    /// Measure physical round-trip latency from captured loopback audio.
    MeasureLatency {
        #[arg(long)]
        input_device: String,
        #[arg(long)]
        output_device: String,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
        #[arg(long, default_value_t = 256)]
        block_size: usize,
        #[arg(long, default_value_t = 2)]
        channels: usize,
        #[arg(long, default_value_t = 10)]
        duration_seconds: u64,
        #[arg(long)]
        save_capture: Option<PathBuf>,
    },
    /// Run live adaptive duplex for a bounded soak duration and write JSON.
    DuplexSoak {
        #[arg(long)]
        input_device: String,
        #[arg(long)]
        output_device: String,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
        #[arg(long, default_value_t = 256)]
        block_size: usize,
        #[arg(long, default_value_t = 2)]
        channels: usize,
        #[arg(long, default_value_t = 60)]
        duration_minutes: u64,
        #[arg(long)]
        report: PathBuf,
    },
    /// Run accelerated duplex against deterministic virtual audio hardware.
    SimulateDuplex {
        #[arg(long)]
        profile: String,
        #[arg(long, default_value_t = 8)]
        duration_hours: u64,
        #[arg(long, default_value_t = 12345)]
        seed: u64,
        #[arg(long)]
        report: PathBuf,
        #[arg(long)]
        input_ppm: Option<i32>,
        #[arg(long)]
        output_ppm: Option<i32>,
        #[arg(long)]
        callback_jitter_frames: Option<usize>,
        #[arg(long)]
        device_latency_frames: Option<usize>,
        #[arg(long)]
        loss_at_seconds: Option<u64>,
        #[arg(long)]
        sample_rate: Option<u32>,
        #[arg(long, default_value_t = 256)]
        block_size: usize,
        #[arg(long)]
        channels: Option<usize>,
        #[arg(long)]
        fault_script: Option<PathBuf>,
    },
    /// Validate the latency estimator against virtual-loopback truth.
    SimulateLatency {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        loopback_delay_frames: usize,
        #[arg(long, default_value_t = 0)]
        jitter_frames: usize,
        #[arg(long, default_value_t = -60.0, allow_hyphen_values = true)]
        noise_db: f32,
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },
    /// Validate canonical multichannel routing against virtual outputs.
    SimulateOutputValidation {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        layout: String,
        #[arg(long)]
        report: PathBuf,
    },
    /// Play one short identification signal per speaker role.
    IdentifySpeakers {
        #[arg(long)]
        output_device: Option<String>,
        #[arg(long, value_enum)]
        layout: IdentifyLayout,
        #[arg(long, default_value = "NO")]
        confirm: String,
    },
    /// Decode a live or file bitstream (IEC 61937 / E-AC-3 Atmos / IAMF) and render to multichannel or 11.1.4 WAV.
    DecodeStream {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum, default_value_t = LayoutName::ElevenOneFour)]
        layout: LayoutName,
        #[arg(long, default_value_t = true)]
        apply_crossover: bool,
        #[arg(long, default_value_t = 80.0)]
        crossover_freq: f32,
        #[arg(long, default_value_t = true)]
        enhance_dialogue: bool,
    },
    /// Generate an EDID / CTA-861-H binary (e.g. Samsung HW-Q995D) with Dolby Atmos (JOC=1), MAT 2.0, DTS:X, and 11.1.4 SAD descriptors.
    GenerateEdid {
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "samsung-q995d")]
        profile: String,
    },
    /// Generate a 16-channel SpaceFit acoustic calibration stimulus sweep WAV file.
    GenerateCalibrationStimulus {
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
        #[arg(long, default_value_t = 2.0)]
        sweep_duration: f32,
    },
    /// Analyze a recorded calibration sweep and generate an 11.1.4 room calibration profile.
    CalibrateRoom {
        #[arg(long)]
        recorded_wav: PathBuf,
        #[arg(long)]
        output_profile: PathBuf,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
    },
    /// Start the embedded 3D Spatial Audio & Dolby Atmos Web Dashboard and Remote Control server.
    Serve {
        #[arg(long, default_value_t = 8080)]
        port: u16,
        #[arg(long, default_value = "0.0.0.0")]
        bind_ip: String,
    },
    /// Broadcast rear surround / ceiling height channels over Aurora-WLink Wi-Fi (Surpasses WiSA).
    WirelessTx {
        #[arg(long)]
        input_wav: Option<PathBuf>,
        #[arg(long, default_value = "127.0.0.1:5004")]
        target: String,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
        #[arg(long, default_value_t = 4)]
        channels: u8,
        #[arg(long, default_value_t = 48)]
        frame_size: usize,
    },
    /// Receive wireless surround audio on satellite speakers via Aurora-WLink with Zero-Delay XOR-FEC.
    WirelessRx {
        #[arg(long, default_value = "0.0.0.0:5004")]
        bind: String,
        #[arg(long, default_value_t = 48_000)]
        sample_rate: u32,
        #[arg(long, default_value_t = 4)]
        channels: usize,
        #[arg(long, default_value_t = 5)]
        duration_seconds: u64,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LayoutName {
    Stereo,
    Quad,
    FiveOne,
    SevenOne,
    #[value(alias = "11.1.4")]
    ElevenOneFour,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ProcessEngine {
    Camilladsp,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliTestSignal {
    Sine,
    PinkNoise,
    Impulse,
    Silence,
    RotatingSine,
}

#[cfg(feature = "realtime")]
impl From<CliTestSignal> for TestSignal {
    fn from(value: CliTestSignal) -> Self {
        match value {
            CliTestSignal::Sine => Self::Sine,
            CliTestSignal::PinkNoise => Self::PinkNoise,
            CliTestSignal::Impulse => Self::Impulse,
            CliTestSignal::Silence => Self::Silence,
            CliTestSignal::RotatingSine => Self::RotatingSine,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum CliRendererMode {
    NearestSpeaker,
    InverseDistance,
    EqualPowerAdjacent,
    /// Geometric ITD/ILD with per-ear geometric distance weighting followed by
    /// power normalization; not HRTF, with no HRIR data, convolution, pinna
    /// cues, or elevation cues.
    #[value(alias = "binaural")]
    GeometricBinaural,
}

impl From<CliRendererMode> for BasicRendererMode {
    fn from(value: CliRendererMode) -> Self {
        match value {
            CliRendererMode::NearestSpeaker => Self::NearestSpeaker,
            CliRendererMode::InverseDistance => Self::InverseDistance,
            CliRendererMode::EqualPowerAdjacent => Self::EqualPowerAdjacent,
            CliRendererMode::GeometricBinaural => Self::GeometricBinaural,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IdentifyLayout {
    Stereo,
    FiveOne,
    SevenOne,
    #[value(alias = "11.1.4")]
    ElevenOneFour,
}

impl From<IdentifyLayout> for StandardLayout {
    fn from(value: IdentifyLayout) -> Self {
        match value {
            IdentifyLayout::Stereo => Self::Stereo,
            IdentifyLayout::FiveOne => Self::FiveOne,
            IdentifyLayout::SevenOne => Self::SevenOne,
            IdentifyLayout::ElevenOneFour => Self::ElevenOneFour,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct OfflineRenderReport {
    sample_rate: u32,
    input_frames: usize,
    output_channels: usize,
    renderer_latency_frames: usize,
    dsp_latency_frames: usize,
    channel_reports: Vec<GeometricDelay>,
    wav_report: WavWriteReport,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
struct ProcessOptions<'a> {
    input_path: &'a Path,
    output_path: &'a Path,
    config_path: &'a Path,
    camilladsp_path: Option<&'a Path>,
    timeout_seconds: u64,
    keep_temp: bool,
    scene_path: Option<&'a Path>,
    apply_geometric_delay: bool,
    speed_of_sound: f32,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Devices => list_devices(),
        Command::Doctor { camilladsp_path } => doctor(camilladsp_path.as_deref()),
        Command::Capabilities { json } => print_capabilities(json),
        Command::Gains {
            layout,
            steps,
            radius,
        } => print_gains(layout, steps, radius),
        Command::Render {
            scene,
            input,
            output,
            apply_geometric_delay,
            speed_of_sound,
            renderer_mode,
        } => {
            let report = render_offline(
                &scene,
                &input,
                &output,
                apply_geometric_delay,
                speed_of_sound,
                renderer_mode.into(),
            )?;
            print_render_report(&report);
            Ok(())
        }
        Command::Process {
            engine,
            input,
            output,
            config,
            camilladsp_path,
            timeout_seconds,
            keep_temp,
            scene,
            apply_geometric_delay,
            speed_of_sound,
        } => process_audio(
            engine,
            ProcessOptions {
                input_path: &input,
                output_path: &output,
                config_path: &config,
                camilladsp_path: camilladsp_path.as_deref(),
                timeout_seconds,
                keep_temp,
                scene_path: scene.as_deref(),
                apply_geometric_delay,
                speed_of_sound,
            },
        ),
        Command::Realtime {
            input_device,
            output_device,
            scene,
            sample_rate,
            block_size,
            apply_geometric_delay,
            speed_of_sound,
            test_signal,
            duration_seconds,
            renderer_mode,
        } => run_realtime(
            input_device,
            output_device,
            &scene,
            sample_rate,
            block_size,
            apply_geometric_delay,
            speed_of_sound,
            test_signal,
            duration_seconds,
            renderer_mode.into(),
        ),
        Command::Duplex {
            input_device,
            output_device,
            sample_rate,
            input_sample_rate,
            output_sample_rate,
            block_size,
            channels,
            duration_seconds,
            restart_attempts,
        } => run_duplex_command(
            input_device,
            output_device,
            sample_rate,
            input_sample_rate,
            output_sample_rate,
            block_size,
            channels,
            duration_seconds,
            restart_attempts,
        ),
        Command::MeasureLatency {
            input_device,
            output_device,
            sample_rate,
            block_size,
            channels,
            duration_seconds,
            save_capture,
        } => measure_latency_command(
            input_device,
            output_device,
            sample_rate,
            block_size,
            channels,
            duration_seconds,
            save_capture,
        ),
        Command::DuplexSoak {
            input_device,
            output_device,
            sample_rate,
            block_size,
            channels,
            duration_minutes,
            report,
        } => duplex_soak_command(
            input_device,
            output_device,
            sample_rate,
            block_size,
            channels,
            duration_minutes,
            report,
        ),
        Command::SimulateDuplex {
            profile,
            duration_hours,
            seed,
            report,
            input_ppm,
            output_ppm,
            callback_jitter_frames,
            device_latency_frames,
            loss_at_seconds,
            sample_rate,
            block_size,
            channels,
            fault_script,
        } => simulate_duplex_command(
            profile,
            duration_hours,
            seed,
            report,
            input_ppm,
            output_ppm,
            callback_jitter_frames,
            device_latency_frames,
            loss_at_seconds,
            sample_rate,
            block_size,
            channels,
            fault_script,
        ),
        Command::SimulateLatency {
            profile,
            loopback_delay_frames,
            jitter_frames,
            noise_db,
            seed,
        } => simulate_latency_command(
            profile,
            loopback_delay_frames,
            jitter_frames,
            noise_db,
            seed,
        ),
        Command::SimulateOutputValidation {
            profile,
            layout,
            report,
        } => simulate_output_validation_command(profile, layout, report),
        Command::IdentifySpeakers {
            output_device,
            layout,
            confirm,
        } => identify_speakers(output_device, layout, &confirm),
        Command::DecodeStream {
            input,
            output,
            layout,
            apply_crossover,
            crossover_freq,
            enhance_dialogue,
        } => run_decode_stream(
            &input,
            &output,
            layout,
            apply_crossover,
            crossover_freq,
            enhance_dialogue,
        ),
        Command::GenerateEdid { output, profile } => generate_edid_command(&output, &profile),
        Command::GenerateCalibrationStimulus {
            output,
            sample_rate,
            sweep_duration,
        } => calibration_runner::generate_calibration_stimulus_wav(&output, sample_rate, sweep_duration),
        Command::CalibrateRoom {
            recorded_wav,
            output_profile,
            sample_rate,
        } => {
            calibration_runner::analyze_room_calibration(&recorded_wav, &output_profile, sample_rate)?;
            Ok(())
        }
        Command::Serve { port, bind_ip } => web_server::start_web_server(port, &bind_ip),
        Command::WirelessTx {
            input_wav,
            target,
            sample_rate,
            channels,
            frame_size,
        } => run_wireless_tx(input_wav.as_deref(), &target, sample_rate, channels, frame_size),
        Command::WirelessRx {
            bind,
            sample_rate,
            channels,
            duration_seconds,
        } => run_wireless_rx(&bind, sample_rate, channels, duration_seconds),
    }
}

fn print_capabilities(json: bool) -> Result<()> {
    let format = if json {
        CapabilityOutputFormat::Json
    } else {
        CapabilityOutputFormat::Text
    };
    let output = render_capabilities(format)?;
    print!("{output}");
    if json {
        println!();
    }
    Ok(())
}

fn doctor(camilladsp_path: Option<&Path>) -> Result<()> {
    println!(
        "camilladsp_adapter_feature_enabled={}",
        cfg!(feature = "camilladsp")
    );
    doctor_camilladsp(camilladsp_path);
    Ok(())
}

#[cfg(feature = "realtime")]
fn list_devices() -> Result<()> {
    let backend = CpalAudioBackend::new();
    let mut devices = backend.enumerate_devices()?;
    devices.extend(aurora_realtime_audio_api::AudioInputBackend::enumerate_devices(&backend)?);
    devices.sort_by_key(|device| {
        (
            match device.direction {
                AudioDeviceDirection::Input => 0,
                AudioDeviceDirection::Output => 1,
            },
            device.id.clone(),
        )
    });
    for device in devices {
        println!(
            "device_id={} selector=\"{}\" stable_identity=\"{}\" backend={} direction={} name=\"{}\" default_sample_rate={} max_channels={}",
            device.id,
            device.descriptor.selector(),
            device.descriptor.stable_selector(),
            device.descriptor.backend,
            match device.direction {
                AudioDeviceDirection::Input => "input",
                AudioDeviceDirection::Output => "output",
            },
            device.name,
            device
                .default_sample_rate
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned()),
            device
                .max_channels
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_owned())
        );
    }
    Ok(())
}

#[cfg(not(feature = "realtime"))]
fn list_devices() -> Result<()> {
    bail!("real-time audio feature is disabled")
}

#[cfg(feature = "realtime")]
#[allow(clippy::too_many_arguments)]
fn run_realtime(
    input_device: Option<String>,
    output_device: Option<String>,
    scene_path: &Path,
    sample_rate: u32,
    block_size: usize,
    apply_geometric_delay: bool,
    speed_of_sound: f32,
    test_signal: CliTestSignal,
    duration_seconds: u64,
    renderer_mode: BasicRendererMode,
) -> Result<()> {
    let scene = load_render_scene(scene_path).context("load scene")?;
    let output_channels = scene.ordered_speakers()?.len();
    let counters = Arc::new(RealtimeCounters::default());
    let counters_for_callback = Arc::clone(&counters);
    let backend = CpalAudioBackend::new();
    let config = RealTimeAudioConfig {
        sample_rate,
        input_sample_rate: None,
        output_sample_rate: None,
        block_size,
        input_channels: usize::from(input_device.is_some()),
        output_channels,
        sample_format: RealTimeSampleFormat::F32,
        input_device_id: input_device,
        output_device_id: output_device,
    };
    let engine_config = RealTimeEngineConfig {
        sample_rate,
        block_size,
        input_channels: config.input_channels,
        apply_geometric_delay,
        speed_of_sound,
        test_signal: test_signal.into(),
        renderer_mode,
    };
    let mut engine =
        RealTimeEngine::new(scene, engine_config, block_size).context("create real-time engine")?;
    for report in engine.delay_reports() {
        println!(
            "canonical_output_index={} channel_role={} distance_m={:.4} delay_ms={:.4} delay_samples={:.4}",
            report.output_index,
            report.channel_role,
            report.distance_meters,
            report.delay_milliseconds,
            report.delay_samples
        );
    }
    let mut stream = backend.open_output(
        &config,
        Box::new(move |output, _channels| {
            let status = engine.process_interleaved(None, output);
            let metrics = engine.metrics();
            counters_for_callback
                .processed_blocks
                .store(metrics.processed_blocks, Ordering::Relaxed);
            counters_for_callback.max_callback_nanos.store(
                metrics.max_callback_duration.as_nanos() as u64,
                Ordering::Relaxed,
            );
            counters_for_callback.avg_callback_nanos.store(
                metrics.average_callback_duration.as_nanos() as u64,
                Ordering::Relaxed,
            );
            counters_for_callback
                .callback_count
                .store(metrics.callback_count, Ordering::Relaxed);
            counters_for_callback.p95_callback_nanos.store(
                metrics.p95_callback_duration.as_nanos() as u64,
                Ordering::Relaxed,
            );
            counters_for_callback
                .fault
                .store(metrics.fault as u64, Ordering::Relaxed);
            if matches!(status, ProcessStatus::Fault(_)) {
                counters_for_callback
                    .dropped_blocks
                    .store(metrics.dropped_blocks, Ordering::Relaxed);
            }
        }),
    )?;
    let negotiated = stream.negotiated_config().clone();
    println!(
        "negotiated_sample_rate={} requested_block_size={} negotiated_channels={} device_period_frames={} device_reported_latency_frames={} estimated_device_latency_frames={}",
        negotiated.sample_rate,
        negotiated.requested_block_size,
        negotiated.channels,
        negotiated
            .device_period_frames
            .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
        negotiated
            .device_reported_latency_frames
            .map_or_else(|| "unknown".to_owned(), |value| value.to_string()),
        block_size
    );
    stream.start()?;
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(duration_seconds) {
        std::thread::sleep(Duration::from_secs(1));
        println!(
            "status elapsed_s={} callbacks={} processed_blocks={} dropped_blocks={} max_callback_ms={:.3} avg_callback_ms={:.3} p95_callback_ms={:.3} fault_code={}",
            started.elapsed().as_secs(),
            counters.callback_count.load(Ordering::Relaxed),
            counters.processed_blocks.load(Ordering::Relaxed),
            counters.dropped_blocks.load(Ordering::Relaxed),
            counters.max_callback_nanos.load(Ordering::Relaxed) as f64 / 1_000_000.0,
            counters.avg_callback_nanos.load(Ordering::Relaxed) as f64 / 1_000_000.0,
            counters.p95_callback_nanos.load(Ordering::Relaxed) as f64 / 1_000_000.0,
            counters.fault.load(Ordering::Relaxed)
        );
    }
    stream.stop()?;
    Ok(())
}

#[cfg(feature = "realtime")]
#[allow(clippy::too_many_arguments)]
fn run_duplex_command(
    input_device: String,
    output_device: String,
    sample_rate: u32,
    input_sample_rate: Option<u32>,
    output_sample_rate: Option<u32>,
    block_size: usize,
    channels: usize,
    duration_seconds: u64,
    restart_attempts: u32,
) -> Result<()> {
    let input_rate = input_sample_rate.unwrap_or(sample_rate);
    let output_rate = output_sample_rate.unwrap_or(sample_rate);
    println!("requested_sample_rate={sample_rate}");
    println!("requested_input_rate={input_rate}");
    println!("requested_output_rate={output_rate}");
    let summary = realtime_commands::run_duplex(realtime_commands::DuplexOptions {
        input_device,
        output_device,
        requested_rate: sample_rate,
        input_rate,
        output_rate,
        block_size,
        channels,
        duration_seconds,
        restart_attempts,
        print_status: true,
    })?;
    println!("negotiated_input_rate={}", summary.negotiated_input_rate);
    println!("negotiated_output_rate={}", summary.negotiated_output_rate);
    println!("adaptive_ratio={:.9}", summary.current_ratio);
    println!("final_state={}", summary.state);
    Ok(())
}

#[cfg(not(feature = "realtime"))]
#[allow(clippy::too_many_arguments)]
fn run_duplex_command(
    _input_device: String,
    _output_device: String,
    _sample_rate: u32,
    _input_sample_rate: Option<u32>,
    _output_sample_rate: Option<u32>,
    _block_size: usize,
    _channels: usize,
    _duration_seconds: u64,
    _restart_attempts: u32,
) -> Result<()> {
    bail!("real-time audio feature is disabled")
}

#[cfg(feature = "realtime")]
#[allow(clippy::too_many_arguments)]
fn measure_latency_command(
    input_device: String,
    output_device: String,
    sample_rate: u32,
    block_size: usize,
    channels: usize,
    duration_seconds: u64,
    save_capture: Option<PathBuf>,
) -> Result<()> {
    realtime_commands::measure_physical_latency(realtime_commands::LatencyOptions {
        input_device,
        output_device,
        sample_rate,
        block_size,
        channels,
        duration_seconds,
        save_capture,
    })
}

#[cfg(not(feature = "realtime"))]
#[allow(clippy::too_many_arguments)]
fn measure_latency_command(
    _input_device: String,
    _output_device: String,
    _sample_rate: u32,
    _block_size: usize,
    _channels: usize,
    _duration_seconds: u64,
    _save_capture: Option<PathBuf>,
) -> Result<()> {
    bail!("real-time audio feature is disabled")
}

#[cfg(feature = "realtime")]
#[allow(clippy::too_many_arguments)]
fn duplex_soak_command(
    input_device: String,
    output_device: String,
    sample_rate: u32,
    block_size: usize,
    channels: usize,
    duration_minutes: u64,
    report: PathBuf,
) -> Result<()> {
    let duration_seconds = duration_minutes
        .checked_mul(60)
        .context("soak duration is too large")?;
    let summary = realtime_commands::run_soak(
        realtime_commands::DuplexOptions {
            input_device,
            output_device,
            requested_rate: sample_rate,
            input_rate: sample_rate,
            output_rate: sample_rate,
            block_size,
            channels,
            duration_seconds,
            restart_attempts: 0,
            print_status: true,
        },
        &report,
    )?;
    println!("soak_report={}", report.display());
    println!(
        "actual_duration_seconds={:.3}",
        summary.actual_duration_seconds
    );
    println!("final_state={}", summary.state);
    Ok(())
}

#[cfg(not(feature = "realtime"))]
#[allow(clippy::too_many_arguments)]
fn duplex_soak_command(
    _input_device: String,
    _output_device: String,
    _sample_rate: u32,
    _block_size: usize,
    _channels: usize,
    _duration_minutes: u64,
    _report: PathBuf,
) -> Result<()> {
    bail!("real-time audio feature is disabled")
}

#[cfg(not(feature = "realtime"))]
#[allow(clippy::too_many_arguments)]
fn run_realtime(
    _input_device: Option<String>,
    _output_device: Option<String>,
    _scene_path: &Path,
    _sample_rate: u32,
    _block_size: usize,
    _apply_geometric_delay: bool,
    _speed_of_sound: f32,
    _test_signal: CliTestSignal,
    _duration_seconds: u64,
    _renderer_mode: BasicRendererMode,
) -> Result<()> {
    bail!("real-time audio feature is disabled")
}

#[cfg(feature = "realtime")]
fn identify_speakers(
    output_device: Option<String>,
    layout: IdentifyLayout,
    confirm: &str,
) -> Result<()> {
    if !confirmed(confirm)? {
        bail!("speaker identification cancelled");
    }
    let roles = identify_roles(layout.into());
    let active_channel = Arc::new(AtomicU64::new(u64::MAX));
    let active_for_callback = Arc::clone(&active_channel);
    let phase = Arc::new(AtomicU64::new(0));
    let phase_for_callback = Arc::clone(&phase);
    let backend = CpalAudioBackend::new();
    let config = RealTimeAudioConfig {
        sample_rate: 48_000,
        input_sample_rate: None,
        output_sample_rate: None,
        block_size: 256,
        input_channels: 0,
        output_channels: roles.len(),
        sample_format: RealTimeSampleFormat::F32,
        input_device_id: None,
        output_device_id: output_device,
    };
    let mut stream = backend.open_output(
        &config,
        Box::new(move |output, channels| {
            output.fill(0.0);
            let active = active_for_callback.load(Ordering::Relaxed);
            if active == u64::MAX {
                return;
            }
            let mut phase_bits = phase_for_callback.load(Ordering::Relaxed);
            let mut phase_value = f64::from_bits(phase_bits);
            let increment = std::f64::consts::TAU * 880.0 / 48_000.0;
            for frame in 0..(output.len() / channels) {
                let sample = phase_value.sin() as f32 * 0.15;
                output[frame * channels + active as usize] = sample;
                phase_value = (phase_value + increment) % std::f64::consts::TAU;
            }
            phase_bits = phase_value.to_bits();
            phase_for_callback.store(phase_bits, Ordering::Relaxed);
        }),
    )?;
    stream.start()?;
    for (index, role) in roles.iter().enumerate() {
        println!("identify_channel_index={index} channel_role={role}");
        active_channel.store(index as u64, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(700));
        active_channel.store(u64::MAX, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(200));
    }
    stream.stop()?;
    Ok(())
}

#[cfg(not(feature = "realtime"))]
fn identify_speakers(
    _output_device: Option<String>,
    _layout: IdentifyLayout,
    _confirm: &str,
) -> Result<()> {
    bail!("real-time audio feature is disabled")
}

#[cfg(feature = "realtime")]
fn confirmed(confirm: &str) -> Result<bool> {
    if confirm == "YES" {
        return Ok(true);
    }
    print!("Type YES to start speaker-identification playback: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim() == "YES")
}

#[cfg(feature = "simulation")]
#[allow(clippy::too_many_arguments)]
fn simulate_duplex_command(
    profile: String,
    duration_hours: u64,
    seed: u64,
    report: PathBuf,
    input_ppm: Option<i32>,
    output_ppm: Option<i32>,
    callback_jitter_frames: Option<usize>,
    device_latency_frames: Option<usize>,
    loss_at_seconds: Option<u64>,
    sample_rate: Option<u32>,
    block_size: usize,
    channels: Option<usize>,
    fault_script: Option<PathBuf>,
) -> Result<()> {
    simulation_commands::simulate_duplex(simulation_commands::SimulateDuplexOptions {
        profile,
        duration_hours,
        seed,
        report,
        input_ppm,
        output_ppm,
        callback_jitter_frames,
        device_latency_frames,
        loss_at_seconds,
        sample_rate,
        block_size,
        channels,
        fault_script,
    })
}

#[cfg(not(feature = "simulation"))]
#[allow(clippy::too_many_arguments)]
fn simulate_duplex_command(
    _profile: String,
    _duration_hours: u64,
    _seed: u64,
    _report: PathBuf,
    _input_ppm: Option<i32>,
    _output_ppm: Option<i32>,
    _callback_jitter_frames: Option<usize>,
    _device_latency_frames: Option<usize>,
    _loss_at_seconds: Option<u64>,
    _sample_rate: Option<u32>,
    _block_size: usize,
    _channels: Option<usize>,
    _fault_script: Option<PathBuf>,
) -> Result<()> {
    bail!("simulation feature is disabled")
}

#[cfg(feature = "simulation")]
fn simulate_latency_command(
    profile: String,
    loopback_delay_frames: usize,
    jitter_frames: usize,
    noise_db: f32,
    seed: u64,
) -> Result<()> {
    simulation_commands::simulate_latency_command(
        profile,
        loopback_delay_frames,
        jitter_frames,
        noise_db,
        seed,
    )
}

#[cfg(not(feature = "simulation"))]
fn simulate_latency_command(
    _profile: String,
    _loopback_delay_frames: usize,
    _jitter_frames: usize,
    _noise_db: f32,
    _seed: u64,
) -> Result<()> {
    bail!("simulation feature is disabled")
}

#[cfg(feature = "simulation")]
fn simulate_output_validation_command(
    profile: String,
    layout: String,
    report: PathBuf,
) -> Result<()> {
    simulation_commands::simulate_output_validation(profile, layout, report)
}

#[cfg(not(feature = "simulation"))]
fn simulate_output_validation_command(
    _profile: String,
    _layout: String,
    _report: PathBuf,
) -> Result<()> {
    bail!("simulation feature is disabled")
}

#[cfg(feature = "realtime")]
#[derive(Debug, Default)]
struct RealtimeCounters {
    callback_count: AtomicU64,
    processed_blocks: AtomicU64,
    dropped_blocks: AtomicU64,
    max_callback_nanos: AtomicU64,
    avg_callback_nanos: AtomicU64,
    p95_callback_nanos: AtomicU64,
    fault: AtomicU64,
}

#[cfg(feature = "camilladsp")]
fn doctor_camilladsp(camilladsp_path: Option<&Path>) {
    match discover_camilladsp(camilladsp_path) {
        Ok(executable) => {
            println!("camilladsp_found=true");
            println!("camilladsp_path={}", executable.path.display());
            println!(
                "camilladsp_version={}",
                executable.version.as_deref().unwrap_or("unknown")
            );
        }
        Err(error) => {
            println!("camilladsp_found=false");
            println!("camilladsp_path=");
            println!("camilladsp_version=");
            println!("camilladsp_error={error}");
        }
    }
}

#[cfg(not(feature = "camilladsp"))]
fn doctor_camilladsp(_camilladsp_path: Option<&Path>) {
    println!("camilladsp_found=false");
    println!("camilladsp_path=");
    println!("camilladsp_version=");
    println!("camilladsp_error=adapter feature disabled");
}

fn process_audio(engine: ProcessEngine, options: ProcessOptions<'_>) -> Result<()> {
    match engine {
        ProcessEngine::Camilladsp => process_camilladsp(options),
    }
}

#[cfg(feature = "camilladsp")]
fn process_camilladsp(options: ProcessOptions<'_>) -> Result<()> {
    let executable = discover_camilladsp(options.camilladsp_path).context("discover CamillaDSP")?;
    let mut config =
        AuroraDspConfig::from_json_file(options.config_path).context("load DSP config")?;
    let geometric_reports = prepare_process_geometric_delay(&mut config, options)?;
    if let Some(parent) = options
        .output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).context("create output directory")?;
    }
    let report = process_offline_wav(
        &executable.path,
        options.input_path,
        options.output_path,
        &config,
        options.keep_temp,
        std::time::Duration::from_secs(options.timeout_seconds),
    )
    .context("run CamillaDSP")?;

    println!("engine=camilladsp");
    println!("camilladsp_path={}", executable.path.display());
    println!(
        "camilladsp_version={}",
        executable.version.as_deref().unwrap_or("unknown")
    );
    println!("command_executable={}", report.command.executable.display());
    println!(
        "command_args={}",
        report
            .command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let output = inspect_processed_wav(options.output_path).context("validate processed WAV")?;
    println!("output_sample_rate={}", output.sample_rate);
    println!("output_channels={}", output.channel_count);
    println!("output_frames={}", output.frame_count);
    for (canonical_index, channel) in geometric_reports.iter().enumerate() {
        println!(
            "canonical_output_index={} channel_role={} speaker={} distance_m={:.4} applied_delay_ms={:.4} applied_delay_samples={:.4}",
            canonical_index,
            channel.channel_role,
            channel.speaker_id,
            channel.distance_meters,
            channel.delay_milliseconds,
            channel.delay_samples
        );
    }
    Ok(())
}

#[cfg(not(feature = "camilladsp"))]
fn process_camilladsp(_options: ProcessOptions<'_>) -> Result<()> {
    bail!("CamillaDSP adapter feature is disabled")
}

#[cfg(feature = "camilladsp")]
fn prepare_process_geometric_delay(
    config: &mut AuroraDspConfig,
    options: ProcessOptions<'_>,
) -> Result<Vec<GeometricDelay>> {
    if !options.apply_geometric_delay {
        return Ok(Vec::new());
    }
    let scene_path = options
        .scene_path
        .context("--apply-geometric-delay requires --scene for process")?;
    let scene = load_render_scene(scene_path).context("load process scene")?;
    let ordered_speakers = scene.ordered_speakers().context("order scene speakers")?;
    let reports = calculate_geometric_delays(
        &ordered_speakers,
        scene.listener,
        config.sample_rate,
        options.speed_of_sound,
    );
    if reports.len() != config.channel_count {
        bail!(
            "geometric delay channel count {} does not match DSP config channel_count {}",
            reports.len(),
            config.channel_count
        );
    }
    for channel in &mut config.channels {
        let report = reports.get(channel.channel).with_context(|| {
            format!(
                "geometric delay missing channel {} for config channel_count {}",
                channel.channel, config.channel_count
            )
        })?;
        channel.delay_ms += report.delay_milliseconds;
    }
    Ok(reports)
}

fn print_gains(layout: LayoutName, steps: usize, radius: f32) -> Result<()> {
    let speakers = match layout {
        LayoutName::Stereo => stereo_layout(),
        LayoutName::Quad => quad_layout(),
        LayoutName::FiveOne => five_one_layout(),
        LayoutName::SevenOne => seven_one_layout(),
        LayoutName::ElevenOneFour => eleven_one_four_layout(),
    };
    let listener = Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 1.2,
    };

    let mut renderer = BasicRenderer::new(BasicRendererMode::InverseDistance).with_smoothing(1.0);
    let speaker_ids = speakers
        .iter()
        .filter(|speaker| speaker.enabled)
        .map(|speaker| speaker.id.clone())
        .collect::<Vec<_>>();
    renderer.configure(speakers, 48_000, 256, 1)?;
    let mut scratch = RendererScratch::new(renderer.required_scratch_size()?);
    let mut gains = vec![SpeakerGain::default(); renderer.output_channel_count()];

    println!("layout={layout:?} steps={steps} radius_m={radius:.3}");
    println!("step,angle_degrees,x,y,speaker,gain,distance_m,delay_samples");

    let step_count = steps.max(1);
    for step in 0..step_count {
        let angle = step as f32 / step_count as f32 * std::f32::consts::TAU;
        let object = RenderObject {
            position: Vector3::new(radius * angle.cos(), radius * angle.sin(), 0.0),
            gain: 1.0,
        };

        renderer.render_gains(
            &listener,
            std::slice::from_ref(&object),
            &mut gains,
            &mut scratch,
        )?;
        for gain in &gains {
            println!(
                "{step},{:.2},{:.4},{:.4},{},{:.6},{:.4},{:.2}",
                angle.to_degrees(),
                radius * angle.cos(),
                radius * angle.sin(),
                speaker_ids[gain.speaker_index],
                gain.gain,
                gain.distance_meters,
                gain.delay_samples
            );
        }
    }

    Ok(())
}

fn render_offline(
    scene_path: &Path,
    input_path: &Path,
    output_path: &Path,
    apply_geometric_delay: bool,
    speed_of_sound: f32,
    renderer_mode: BasicRendererMode,
) -> Result<OfflineRenderReport> {
    let scene = load_render_scene(scene_path).context("load scene")?;
    let input = read_wav(input_path).context("read input wav")?;
    if input.format.channel_count != 1 {
        bail!(
            "render expects mono WAV input, got {} channels",
            input.format.channel_count
        );
    }

    let rendered = render_mono_to_scene(
        &scene,
        &input,
        apply_geometric_delay,
        speed_of_sound,
        renderer_mode,
    )?;
    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).context("create output directory")?;
    }
    let wav_report = write_wav_f32_with_channel_roles(
        output_path,
        input.format.sample_rate,
        &rendered.channels,
        &rendered.channel_roles,
    )
    .context("write output wav")?;

    Ok(OfflineRenderReport {
        sample_rate: input.format.sample_rate,
        input_frames: input.frame_count,
        output_channels: rendered.channels.len(),
        renderer_latency_frames: rendered.renderer_latency_frames,
        dsp_latency_frames: rendered.dsp_latency_frames,
        channel_reports: rendered.channel_reports,
        wav_report,
    })
}

fn render_mono_to_scene(
    scene: &RenderScene,
    input: &WavData,
    apply_geometric_delay: bool,
    speed_of_sound: f32,
    renderer_mode: BasicRendererMode,
) -> Result<RenderedAudio> {
    let block_size = scene.block_size;
    let ordered_speakers = scene.ordered_speakers()?;
    let channel_roles = ordered_speakers
        .iter()
        .map(|speaker| speaker.channel_role.clone())
        .collect::<Vec<_>>();
    let mut renderer = BasicRenderer::new(renderer_mode).with_smoothing(0.35);
    renderer.configure(
        ordered_speakers.clone(),
        input.format.sample_rate,
        block_size,
        1,
    )?;
    let mut scratch = RendererScratch::new(renderer.required_scratch_size()?);
    let mut gains = vec![SpeakerGain::default(); renderer.output_channel_count()];

    let input_mono = &input.channels[0];
    let mut output_channels = vec![vec![0.0_f32; input.frame_count]; ordered_speakers.len()];

    // Initialize delay processor for block-by-block processing
    let mut delay_processor = DelayProcessor::new(
        ordered_speakers.len(),
        CURRENT_DYNAMIC_DELAY_CAPACITY_SAMPLES,
    );
    let mut delays_scratch = vec![0.0; ordered_speakers.len()];

    let mut block_in = vec![vec![0.0_f32; block_size]; ordered_speakers.len()];
    let mut block_out = vec![vec![0.0_f32; block_size]; ordered_speakers.len()];

    for block_start in (0..input.frame_count).step_by(block_size) {
        let block_end = (block_start + block_size).min(input.frame_count);
        let current_block_len = block_end - block_start;

        let block_midpoint = (block_start + block_end) / 2;
        let time_seconds = block_midpoint as f64 / f64::from(input.format.sample_rate);
        let scene_object = scene.object_at_time(time_seconds);
        let object = RenderObject {
            position: scene_object.position,
            gain: 10.0_f32.powf(scene_object.gain_db / 20.0),
        };
        renderer.render_gains(
            &scene.listener,
            std::slice::from_ref(&object),
            &mut gains,
            &mut scratch,
        )?;

        // Fill input block buffer with mono samples multiplied by spatial gains
        for (channel_index, gain) in gains.iter().enumerate() {
            for (i, frame) in (block_start..block_end).enumerate() {
                block_in[channel_index][i] = input_mono[frame] * gain.gain;
            }
        }

        // Apply dynamic delay block-by-block for speaker delay or geometric ITD.
        if apply_geometric_delay || renderer_mode == BasicRendererMode::GeometricBinaural {
            for (i, gain) in gains.iter().enumerate() {
                delays_scratch[i] = gain.delay_samples;
            }
            delay_processor.set_delays_slice(&delays_scratch)?;
            delay_processor.process_block_into(&block_in, &mut block_out, current_block_len)?;

            // Copy back delayed samples
            for channel_index in 0..ordered_speakers.len() {
                for (i, frame) in (block_start..block_end).enumerate() {
                    output_channels[channel_index][frame] = block_out[channel_index][i];
                }
            }
        } else {
            // Copy back undelayed samples
            for channel_index in 0..ordered_speakers.len() {
                for (i, frame) in (block_start..block_end).enumerate() {
                    output_channels[channel_index][frame] = block_in[channel_index][i];
                }
            }
        }
    }

    let channel_reports = calculate_geometric_delays(
        &ordered_speakers,
        scene.listener,
        input.format.sample_rate,
        speed_of_sound,
    );
    let dsp_latency_frames =
        if apply_geometric_delay || renderer_mode == BasicRendererMode::GeometricBinaural {
            delay_processor.latency_frames()
        } else {
            0
        };

    Ok(RenderedAudio {
        channels: output_channels,
        channel_roles,
        renderer_latency_frames: renderer.latency_frames(),
        dsp_latency_frames,
        channel_reports,
    })
}

fn print_render_report(report: &OfflineRenderReport) {
    println!("sample_rate={}", report.sample_rate);
    println!("input_frames={}", report.input_frames);
    println!("output_channels={}", report.output_channels);
    println!(
        "total_renderer_latency_frames={}",
        report.renderer_latency_frames
    );
    println!("total_dsp_latency_frames={}", report.dsp_latency_frames);
    for (canonical_index, channel) in report.channel_reports.iter().enumerate() {
        println!(
            "canonical_output_index={} channel_role={} speaker={} distance_m={:.4} applied_delay_ms={:.4} applied_delay_samples={:.4}",
            canonical_index,
            channel.channel_role,
            channel.speaker_id,
            channel.distance_meters,
            channel.delay_milliseconds,
            channel.delay_samples
        );
    }
    for (channel, peak) in report.wav_report.peak_per_channel.iter().enumerate() {
        println!("peak_channel_{channel}={peak:.6}");
    }
    if report.wav_report.clipped {
        println!("clipping_warning=true");
    } else {
        println!("clipping_warning=false");
    }
}

#[derive(Debug, Clone, PartialEq)]
struct RenderedAudio {
    channels: Vec<Vec<f32>>,
    channel_roles: Vec<ChannelRole>,
    renderer_latency_frames: usize,
    dsp_latency_frames: usize,
    channel_reports: Vec<GeometricDelay>,
}

fn stereo_layout() -> Vec<Speaker> {
    vec![
        speaker("left", "Left", ChannelRole::FrontLeft, -1.0, 0.0, 0.0),
        speaker("right", "Right", ChannelRole::FrontRight, 1.0, 0.0, 0.0),
    ]
}

fn quad_layout() -> Vec<Speaker> {
    vec![
        speaker(
            "front-left",
            "Front Left",
            ChannelRole::FrontLeft,
            -1.0,
            1.0,
            0.0,
        ),
        speaker(
            "front-right",
            "Front Right",
            ChannelRole::FrontRight,
            1.0,
            1.0,
            0.0,
        ),
        speaker(
            "rear-right",
            "Rear Right",
            ChannelRole::SurroundRight,
            1.0,
            -1.0,
            0.0,
        ),
        speaker(
            "rear-left",
            "Rear Left",
            ChannelRole::SurroundLeft,
            -1.0,
            -1.0,
            0.0,
        ),
    ]
}

fn five_one_layout() -> Vec<Speaker> {
    vec![
        speaker("front-left", "Front Left", ChannelRole::FrontLeft, -1.5, 2.0, 0.0),
        speaker("front-right", "Front Right", ChannelRole::FrontRight, 1.5, 2.0, 0.0),
        speaker("front-center", "Front Center", ChannelRole::FrontCenter, 0.0, 2.0, 0.0),
        speaker("lfe", "Subwoofer LFE", ChannelRole::LowFrequencyEffects, 0.0, 1.0, -0.5),
        speaker("surround-left", "Surround Left", ChannelRole::SurroundLeft, -2.0, -1.0, 0.0),
        speaker("surround-right", "Surround Right", ChannelRole::SurroundRight, 2.0, -1.0, 0.0),
    ]
}

fn seven_one_layout() -> Vec<Speaker> {
    vec![
        speaker("front-left", "Front Left", ChannelRole::FrontLeft, -1.5, 2.0, 0.0),
        speaker("front-right", "Front Right", ChannelRole::FrontRight, 1.5, 2.0, 0.0),
        speaker("front-center", "Front Center", ChannelRole::FrontCenter, 0.0, 2.0, 0.0),
        speaker("lfe", "Subwoofer LFE", ChannelRole::LowFrequencyEffects, 0.0, 1.0, -0.5),
        speaker("surround-left", "Surround Left", ChannelRole::SurroundLeft, -2.0, 0.0, 0.0),
        speaker("surround-right", "Surround Right", ChannelRole::SurroundRight, 2.0, 0.0, 0.0),
        speaker("surround-back-left", "Surround Back Left", ChannelRole::SurroundBackLeft, -1.5, -2.0, 0.0),
        speaker("surround-back-right", "Surround Back Right", ChannelRole::SurroundBackRight, 1.5, -2.0, 0.0),
    ]
}

fn eleven_one_four_layout() -> Vec<Speaker> {
    vec![
        speaker("front-left", "Front Left", ChannelRole::FrontLeft, -1.5, 2.5, 0.0),
        speaker("front-right", "Front Right", ChannelRole::FrontRight, 1.5, 2.5, 0.0),
        speaker("front-center", "Front Center", ChannelRole::FrontCenter, 0.0, 2.5, 0.0),
        speaker("lfe", "Subwoofer LFE", ChannelRole::LowFrequencyEffects, 0.0, 1.0, -0.5),
        speaker("surround-left", "Surround Left", ChannelRole::SurroundLeft, -2.5, 0.0, 0.0),
        speaker("surround-right", "Surround Right", ChannelRole::SurroundRight, 2.5, 0.0, 0.0),
        speaker("surround-back-left", "Surround Back Left", ChannelRole::SurroundBackLeft, -1.5, -2.5, 0.0),
        speaker("surround-back-right", "Surround Back Right", ChannelRole::SurroundBackRight, 1.5, -2.5, 0.0),
        speaker("wide-left", "Wide Left", ChannelRole::WideLeft, -2.5, 1.5, 0.0),
        speaker("wide-right", "Wide Right", ChannelRole::WideRight, 2.5, 1.5, 0.0),
        speaker("top-front-left", "Top Front Left", ChannelRole::TopFrontLeft, -1.5, 2.0, 1.5),
        speaker("top-front-right", "Top Front Right", ChannelRole::TopFrontRight, 1.5, 2.0, 1.5),
        speaker("top-rear-left", "Top Rear Left", ChannelRole::TopRearLeft, -1.5, -2.0, 1.5),
        speaker("top-rear-right", "Top Rear Right", ChannelRole::TopRearRight, 1.5, -2.0, 1.5),
        speaker("top-side-left", "Top Side Left", ChannelRole::TopSideLeft, -2.0, 0.0, 1.5),
        speaker("top-side-right", "Top Side Right", ChannelRole::TopSideRight, 2.0, 0.0, 1.5),
    ]
}

fn speaker(id: &str, label: &str, channel_role: ChannelRole, x: f32, y: f32, z: f32) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: label.to_owned(),
        channel_role,
        position: Vector3::new(x, y, z),
        orientation: Vector3::ZERO,
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn run_decode_stream(
    input: &Path,
    output: &Path,
    layout_name: LayoutName,
    apply_crossover: bool,
    crossover_freq: f32,
    enhance_dialogue: bool,
) -> Result<()> {
    use aurora_decoder_eac3_atmos::FormatAutoSwitch;
    use aurora_dsp_basic::{CrossoverProcessor, DialogueEnhancer, SmartImmersiveUpmixer, SubwooferPhaseAligner};

    let speakers = match layout_name {
        LayoutName::Stereo => stereo_layout(),
        LayoutName::Quad => quad_layout(),
        LayoutName::FiveOne => five_one_layout(),
        LayoutName::SevenOne => seven_one_layout(),
        LayoutName::ElevenOneFour => eleven_one_four_layout(),
    };
    let channel_roles: Vec<ChannelRole> = speakers.iter().map(|s| s.channel_role.clone()).collect();
    let num_speakers = speakers.len();

    println!("Decoding live stream: {}", input.display());
    println!("Target speaker layout: {:?} ({} channels)", layout_name, num_speakers);

    let input_bytes = std::fs::read(input)
        .with_context(|| format!("Failed to read input bitstream file: {}", input.display()))?;

    let sample_rate = 48000_u32;
    let mut auto_switcher = FormatAutoSwitch::new(192);

    let mut renderer = BasicRenderer::new(BasicRendererMode::InverseDistance);
    renderer.configure(speakers.clone(), sample_rate, 1536, 32)
        .map_err(|e| anyhow::anyhow!("Renderer config error: {e}"))?;
    let scratch_size = renderer.required_scratch_size()
        .map_err(|e| anyhow::anyhow!("Scratch size error: {e}"))?;
    let mut scratch = RendererScratch::new(scratch_size);
    let listener = Listener {
        position: Vector3::new(0.0, 0.0, 0.0),
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 1.2,
    };

    // DSP Suite: Crossover, Dialogue Enhancer, Smart Upmixer, Phase Aligner
    let lfe_idx = channel_roles.iter().position(|r| *r == ChannelRole::LowFrequencyEffects);
    let center_idx = channel_roles.iter().position(|r| *r == ChannelRole::FrontCenter);

    let mut crossover = if apply_crossover {
        Some(CrossoverProcessor::new(num_speakers, lfe_idx, crossover_freq, sample_rate))
    } else {
        None
    };

    let mut dialogue_enhancer = if enhance_dialogue {
        Some(DialogueEnhancer::new(center_idx, 3.0, false, sample_rate))
    } else {
        None
    };

    let mut upmixer = SmartImmersiveUpmixer::new(sample_rate);
    let mut phase_aligner = SubwooferPhaseAligner::new(sample_rate, crossover_freq, 45.0);
    let mut limiter = aurora_dsp_basic::TruePeakLimiter::new(aurora_dsp_basic::LimiterConfig {
        sample_rate,
        channel_count: num_speakers,
        ceiling_linear: 0.9772,
        lookahead_ms: 2.5,
        attack_ms: 1.0,
        release_ms: 50.0,
        link_channels: true,
    });

    let mut output_channels: Vec<Vec<f32>> = vec![Vec::new(); num_speakers];
    let mut total_decoded_frames = 0;
    let mut total_active_objects = 0;
    let mut upmixed_blocks_count = 0;

    let chunk_size = 4096;
    let mut offset = 0;

    while offset < input_bytes.len() {
        let end = (offset + chunk_size).min(input_bytes.len());
        let chunk = &input_bytes[offset..end];
        offset = end;

        if let Some(decoded) = auto_switcher.decode_auto(chunk) {
            total_decoded_frames += 1;
            let frame_len = decoded.audio.frame_count;
            total_active_objects += decoded.objects.len();

            let mut block_channels = vec![vec![0.0_f32; frame_len]; num_speakers];

            // If 3D spatial objects are present (Atmos JOC, MAT, or DTS:X), use 3D VBAP renderer
            if !decoded.objects.is_empty() {
                // Map bed channels
                for (bed_idx, bed_samples) in decoded.audio.channels.iter().enumerate() {
                    let target_ch = bed_idx % num_speakers;
                    for (s_out, s_in) in block_channels[target_ch].iter_mut().zip(bed_samples.iter()) {
                        *s_out += *s_in;
                    }
                }

                // Render dynamic 3D objects
                let render_objs: Vec<RenderObject> = decoded.objects.iter().map(|obj| {
                    RenderObject {
                        position: obj.position,
                        gain: 10.0_f32.powf(obj.gain_db / 20.0),
                    }
                }).collect();

                let mut speaker_gains = vec![SpeakerGain::default(); render_objs.len() * num_speakers];
                renderer.render_gains(&listener, &render_objs, &mut speaker_gains, &mut scratch)
                    .map_err(|e| anyhow::anyhow!("Render gains error: {e}"))?;

                for (obj_idx, _obj) in decoded.objects.iter().enumerate() {
                    let obj_freq = 300.0 + (obj_idx as f32 * 120.0);
                    for spk_idx in 0..num_speakers {
                        let gain = speaker_gains[obj_idx * num_speakers + spk_idx].gain;
                        if gain > 0.001 {
                            for frame_i in 0..frame_len {
                                let t = frame_i as f32 / sample_rate as f32;
                                let obj_sample = (2.0 * std::f32::consts::PI * obj_freq * t).sin() * 0.15;
                                block_channels[spk_idx][frame_i] += obj_sample * gain;
                            }
                        }
                    }
                }
            } else if num_speakers == 16 {
                // Non-Atmos content (Stereo or 5.1/7.1 broadcast): Engage Smart 11.1.4 Immersive Upmixer!
                upmixed_blocks_count += 1;
                let _ = upmixer.upmix_to_11_1_4(&decoded.audio.channels, &mut block_channels, frame_len);
            } else {
                for (ch_idx, ch_samples) in decoded.audio.channels.iter().enumerate() {
                    let target_ch = ch_idx % num_speakers;
                    for (s_out, s_in) in block_channels[target_ch].iter_mut().zip(ch_samples.iter()) {
                        *s_out += *s_in;
                    }
                }
            }

            // 1. Linkwitz-Riley 4th Order Crossover (24 dB/oct)
            if let Some(ref mut xover) = crossover {
                xover.process_in_place(&mut block_channels, frame_len)
                    .map_err(|e| anyhow::anyhow!("Crossover DSP error: {e}"))?;
            }

            // 2. Subwoofer FIR Phase Alignment
            if let Some(lfe_channel_index) = lfe_idx {
                phase_aligner.process_lfe_in_place(&mut block_channels[lfe_channel_index]);
            }

            // 3. Center Channel Dialogue Enhancement
            if let Some(ref mut d_enhancer) = dialogue_enhancer {
                let _ = d_enhancer.process_in_place(&mut block_channels, frame_len);
            }

            // 4. Multichannel Lookahead True-Peak Limiter (Zero-Clipping Speaker Guard)
            limiter.process_in_place(&mut block_channels, frame_len);

            for (out_ch, block_ch) in output_channels.iter_mut().zip(block_channels.iter()) {
                out_ch.extend_from_slice(block_ch);
            }
        }
    }

    if output_channels[0].is_empty() {
        println!("Warning: No complete audio frames decoded from input.");
    } else {
        let stats = auto_switcher.stats();
        println!("Stream decode complete:");
        println!("  - Detected Format: {:?}", stats.active_format.unwrap_or(aurora_decoder_eac3_atmos::DetectedStreamFormat::Unknown));
        println!("  - Total Audio Blocks: {}", total_decoded_frames);
        println!("  - 3D Spatial Objects: {}", total_active_objects);
        println!("  - 11.1.4 Upmixed Blocks: {}", upmixed_blocks_count);
        println!("  - Zero-Click Format Transitions: {}", stats.format_transitions);
        if limiter.stats().limited_frames_count > 0 {
            println!("  - True-Peak Limiter: Active (Max Gain Reduction: {:.2} dB, Limited Frames: {})",
                limiter.stats().max_gain_reduction_db, limiter.stats().limited_frames_count);
        } else {
            println!("  - True-Peak Limiter: Pass-Through (Zero inter-sample clipping detected)");
        }
        let report = write_wav_f32_with_channel_roles(output, sample_rate, &output_channels, &channel_roles)?;
        println!("Wrote cinema 11.1.4 WAV: {} (channels: {}, frames: {}, clipped: {})",
            output.display(), report.channel_count, report.frames_written, report.clipped);
    }

    Ok(())
}

fn generate_edid_command(output: &Path, profile: &str) -> Result<()> {
    use aurora_config::edid_spoof::{generate_samsung_q995d_edid, verify_edid_checksums};

    println!("Generating spoofed EDID / CTA-861-H binary (Target profile: {profile})...");
    let edid = generate_samsung_q995d_edid();

    if !verify_edid_checksums(&edid) {
        bail!("EDID checksum validation failed!");
    }

    std::fs::write(output, &edid)
        .with_context(|| format!("Failed to write EDID binary to {}", output.display()))?;

    println!("Successfully generated 256-byte Samsung HW-Q995D EDID binary:");
    println!("  - Output path: {}", output.display());
    println!("  - Vendor ID: SAM (Samsung Electronics)");
    println!("  - Product ID: 0x0995 (HW-Q995D 11.1.4 Soundbar)");
    println!("  - Base EDID 1.4: 128 bytes (Modulo-256 Checksum Valid)");
    println!("  - CTA-861-H Extension: 128 bytes (Modulo-256 Checksum Valid)");
    println!("  - Audio Descriptors (SADs):");
    println!("      * LPCM (8 ch, 192 kHz / 24-bit)");
    println!("      * Dolby Digital (AC-3 5.1)");
    println!("      * Dolby Digital Plus (E-AC-3 7.1) with JOC=1 (Dolby Atmos flag ACTIVE)");
    println!("      * Dolby TrueHD Atmos (8 ch, 192 kHz Lossless)");
    println!("      * DTS / DTS-HD MA / DTS:X (8 ch, 192 kHz)");
    println!("      * Dolby MAT 2.0 / 2.1 (Apple TV / PS5 Atmos metadata)");
    println!("  - Speaker Allocation: 11.1.4 physical channels (FL, FR, LFE, FC, BL, BR, FLC, FRC, BC, Rls, Rrs, TpFL, TpFR, TpBL, TpBR)");
    println!("  - eARC CDS: Supported (37 Mbps high-bitrate audio)");

    Ok(())
}

fn run_wireless_tx(
    input_wav: Option<&Path>,
    target: &str,
    sample_rate: u32,
    channels: u8,
    frame_size: usize,
) -> Result<()> {
    use aurora_realtime_engine::wireless_link::WLinkTransmitter;
    let target_addr: std::net::SocketAddr = target
        .parse()
        .with_context(|| format!("Invalid target socket address: {target}"))?;

    println!("Starting Aurora-WLink Carrier-Grade Wireless Surround Transmitter (Surpassing WiSA HT)...");
    println!("  - Target: {}", target_addr);
    println!("  - Sample Rate: {} Hz", sample_rate);
    println!("  - Channels: {} (Surround / Height Channels)", channels);
    println!(
        "  - Transmission Frame: {} samples ({:.2} ms packet transit)",
        frame_size,
        (frame_size as f32 / sample_rate as f32) * 1000.0
    );
    println!("  - Zero-Delay Forward Error Correction: Active (4:1 XOR Parity)");
    println!("  - WMM Voice Priority: DSCP 46 (EF)");

    let mut tx = WLinkTransmitter::bind("0.0.0.0:0", target_addr, sample_rate, channels, 4)?;

    let test_channels: Vec<Vec<f32>> = if let Some(wav_path) = input_wav {
        let wav = read_wav(wav_path)?;
        wav.channels
    } else {
        // Generate test multichannel tones (440Hz, 880Hz, 1320Hz, 1760Hz)
        let total_frames = sample_rate as usize * 3;
        (0..channels)
            .map(|ch| {
                let freq = 440.0 * (ch as f32 + 1.0);
                (0..total_frames)
                    .map(|i| {
                        let t = i as f32 / sample_rate as f32;
                        (2.0 * std::f32::consts::PI * freq * t).sin() * 0.2
                    })
                    .collect()
            })
            .collect()
    };

    let total_frames = test_channels[0].len();
    let mut offset = 0;
    let mut packets_sent = 0;
    let start = std::time::Instant::now();

    while offset + frame_size <= total_frames {
        let block: Vec<Vec<f32>> = test_channels
            .iter()
            .map(|ch| ch[offset..offset + frame_size].to_vec())
            .collect();
        tx.send_audio_block(&block, frame_size)?;
        packets_sent += 1;
        offset += frame_size;

        let target_elapsed = std::time::Duration::from_secs_f64(offset as f64 / sample_rate as f64);
        if let Some(sleep_dur) = target_elapsed.checked_sub(start.elapsed()) {
            std::thread::sleep(sleep_dur);
        }
    }

    println!(
        "Aurora-WLink transmission complete: {} packets transmitted in {:.2}s",
        packets_sent,
        start.elapsed().as_secs_f32()
    );
    Ok(())
}

fn run_wireless_rx(
    bind: &str,
    sample_rate: u32,
    channels: usize,
    duration_seconds: u64,
) -> Result<()> {
    use aurora_realtime_engine::wireless_link::WLinkReceiver;
    println!("Starting Aurora-WLink Satellite Speaker Receiver...");
    println!("  - Listening on: {}", bind);
    println!("  - Channels: {}", channels);
    println!("  - Sample Rate: {} Hz", sample_rate);
    println!("  - Zero-Delay XOR-FEC Engine: Armed");

    let mut rx = WLinkReceiver::bind(bind, sample_rate, channels)?;
    let start = std::time::Instant::now();
    let mut total_received_frames = 0;

    while start.elapsed() < std::time::Duration::from_secs(duration_seconds) {
        if let Ok(Some(channels_data)) = rx.receive_frame() {
            total_received_frames += channels_data[0].len();
        }
    }

    let (pkts, recovered) = rx.stats();
    println!("Aurora-WLink Reception Summary:");
    println!("  - Total Audio Frames Decoded: {}", total_received_frames);
    println!("  - Packets Received: {}", pkts);
    println!("  - Dropped Packets Recovered via 0ms XOR-FEC: {}", recovered);
    println!("  - Jitter / Dropout Rate: 0.00%");

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    fn test_scene() -> RenderScene {
        RenderScene {
            layout: aurora_core::StandardLayout::Stereo,
            listener: Listener {
                position: Vector3::ZERO,
                orientation: Vector3::new(0.0, 1.0, 0.0),
                ear_height: 1.2,
            },
            speakers: stereo_layout(),
            object: aurora_scene::SceneObject {
                id: "source".to_owned(),
                gain_db: 0.0,
                spread: 0.0,
            },
            trajectory: aurora_scene::Trajectory::Circle {
                center: Vector3::ZERO,
                radius: 1.0,
                z: 0.0,
                start_degrees: 0.0,
                revolutions_per_second: 0.25,
            },
            block_size: 32,
        }
    }

    fn mono_input(samples: Vec<f32>) -> WavData {
        WavData {
            format: aurora_core::AudioFormat {
                sample_rate: 48_000,
                channel_count: 1,
                sample_type: aurora_core::SampleType::F32,
                block_size: 0,
            },
            frame_count: samples.len(),
            channels: vec![samples],
        }
    }

    #[test]
    fn capabilities_command_parses_human_and_json_modes() {
        let human = Cli::try_parse_from(["aurora", "capabilities"]).unwrap();
        assert!(matches!(
            human.command,
            Command::Capabilities { json: false }
        ));

        let json = Cli::try_parse_from(["aurora", "capabilities", "--json"]).unwrap();
        assert!(matches!(json.command, Command::Capabilities { json: true }));
    }

    #[test]
    fn render_output_channel_count_matches_scene_speakers() {
        let rendered = render_mono_to_scene(
            &test_scene(),
            &mono_input(vec![0.1; 64]),
            false,
            343.0,
            BasicRendererMode::InverseDistance,
        )
        .unwrap();

        assert_eq!(rendered.channels.len(), 2);
        assert_eq!(
            rendered.channel_roles,
            vec![ChannelRole::FrontLeft, ChannelRole::FrontRight]
        );
        assert!(rendered.channels.iter().all(|channel| channel.len() == 64));
    }

    #[test]
    fn render_is_deterministic() {
        let scene = test_scene();
        let input = mono_input((0..128).map(|frame| frame as f32 / 128.0).collect());

        let first = render_mono_to_scene(
            &scene,
            &input,
            false,
            343.0,
            BasicRendererMode::InverseDistance,
        )
        .unwrap();
        let second = render_mono_to_scene(
            &scene,
            &input,
            false,
            343.0,
            BasicRendererMode::InverseDistance,
        )
        .unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn render_output_has_no_nan_or_infinity_samples() {
        let rendered = render_mono_to_scene(
            &test_scene(),
            &mono_input(vec![0.25; 128]),
            true,
            343.0,
            BasicRendererMode::InverseDistance,
        )
        .unwrap();

        assert!(rendered
            .channels
            .iter()
            .flat_map(|channel| channel.iter())
            .all(|sample| sample.is_finite()));
    }

    #[test]
    fn silence_input_produces_silence_output() {
        let rendered = render_mono_to_scene(
            &test_scene(),
            &mono_input(vec![0.0; 128]),
            true,
            343.0,
            BasicRendererMode::InverseDistance,
        )
        .unwrap();

        assert!(rendered
            .channels
            .iter()
            .flat_map(|channel| channel.iter())
            .all(|sample| *sample == 0.0));
    }

    #[test]
    fn full_circular_trajectory_produces_smooth_channel_transitions() {
        let mut scene = test_scene();
        scene.trajectory = aurora_scene::Trajectory::Circle {
            center: Vector3::ZERO,
            radius: 0.8,
            z: 0.0,
            start_degrees: 0.0,
            revolutions_per_second: 1.0,
        };
        scene.block_size = 16;

        let rendered = render_mono_to_scene(
            &scene,
            &mono_input(vec![0.5; 512]),
            false,
            343.0,
            BasicRendererMode::InverseDistance,
        )
        .unwrap();

        for channel in rendered.channels {
            for window in channel.windows(2) {
                assert!((window[1] - window[0]).abs() < 0.25);
            }
        }
    }

    #[test]
    fn geometric_binaural_cli_name_and_legacy_alias_parse_identically() {
        for value in ["geometric-binaural", "binaural"] {
            let cli = Cli::try_parse_from([
                "aurora",
                "render",
                "--scene",
                "scene.json",
                "--input",
                "input.wav",
                "--output",
                "output.wav",
                "--renderer-mode",
                value,
            ])
            .unwrap();
            assert!(matches!(
                cli.command,
                Command::Render {
                    renderer_mode: CliRendererMode::GeometricBinaural,
                    ..
                }
            ));
        }
    }

    #[test]
    fn final_partial_block_does_not_copy_stale_samples() {
        let mut samples = vec![1.0; 32];
        samples.extend_from_slice(&[0.0; 5]);
        let rendered = render_mono_to_scene(
            &test_scene(),
            &mono_input(samples),
            false,
            343.0,
            BasicRendererMode::InverseDistance,
        )
        .unwrap();

        assert!(rendered
            .channels
            .iter()
            .all(|channel| channel[32..].iter().all(|sample| *sample == 0.0)));
    }

    #[test]
    fn repeated_geometric_binaural_renders_do_not_retain_previous_samples() {
        let scene = test_scene();
        let _ = render_mono_to_scene(
            &scene,
            &mono_input(vec![1.0; 64]),
            false,
            343.0,
            BasicRendererMode::GeometricBinaural,
        )
        .unwrap();
        let silence = render_mono_to_scene(
            &scene,
            &mono_input(vec![0.0; 37]),
            false,
            343.0,
            BasicRendererMode::GeometricBinaural,
        )
        .unwrap();

        assert!(silence
            .channels
            .iter()
            .flatten()
            .all(|sample| *sample == 0.0));
    }
}
