//! Headless validation harness for Aurora's canonical direct-eARC software path.
//!
//! This binary deliberately starts at the eARC-receiver output boundary: canonical
//! IEC61937 carrier bytes / S32 slot words. It does not emulate HDMI/eARC signalling
//! and it never treats IEC61937 type 0x15 as JOC proof.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use aurora_core::{AudioBlock, AudioFormat, SampleType, StandardLayout};
use aurora_decoder_api::{DecodedFrame, Decoder};
use aurora_decoder_engine::{AuroraDecoderEngine, EngineConfig};
use aurora_dsp_basic::output::OutputDspConfig;
use aurora_encoded_input::EncodedInputConfig;
use aurora_encoded_runtime::{AuroraPlaybackRuntime, PlaybackBatch, SpeakerOutputStage};
use aurora_iec61937::{
    BurstParser, CarrierWordHalf, CodecFilter, S32LeCarrierNormalizer,
};
use aurora_sim_source::latency::{FixedLatencyHistogram, StageLatencyBook, ValidationStage};
use aurora_sim_source::{
    EAC3_BURST_PERIOD_BYTES, EAC3_CARRIER_BYTES_PER_MS, Eac3AccessUnitFramer,
    write_eac3_period,
};
use clap::{Args as ClapArgs, Parser, Subcommand};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 12;
const BLOCK_FRAMES: usize = 40;
const MAX_VALIDATION_OUTPUT_FRAMES: usize = 2_048;
const DEFAULT_BITRATE_KBPS: u32 = 384;
const EAC3_PERIOD: Duration = Duration::from_millis(32);
const DIRECT_EARC_REFERENCE_WORD_HALF: CarrierWordHalf = CarrierWordHalf::High;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const WAVE_FLOAT_SUBFORMAT: [u8; 16] = [
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B,
    0x71,
];
const CHANNEL_ID_FREQUENCIES_HZ: [f32; CHANNELS] = [
    1_000.0, 1_100.0, 1_200.0, 80.0, 1_300.0, 1_400.0, 1_500.0, 1_600.0, 1_700.0,
    1_800.0, 1_900.0, 2_000.0,
];

#[derive(Debug, Parser)]
#[command(
    name = "aurora-sim",
    about = "Aurora headless validation, latency, channel-ID and stress harness"
)]
struct Cli {
    #[command(subcommand)]
    command: SimCommand,
}

#[derive(Debug, Subcommand)]
enum SimCommand {
    /// Generate a canonical 7.1.4 channel-ID IEEE-F32 WAV.
    ChannelId(ChannelIdArgs),
    /// Measure software stages that can be isolated headlessly.
    LatencyReport(LatencyArgs),
    /// Run format/reset/fault/soak validation without physical audio hardware.
    Stress(StressArgs),
}

#[derive(Debug, ClapArgs)]
struct ChannelIdArgs {
    /// Destination WAV path.
    #[arg(long, default_value = "aurora-channel-id-7.1.4.wav")]
    output: PathBuf,

    /// Duration of each one-channel segment.
    #[arg(long, default_value_t = 0.5)]
    seconds_per_channel: f64,

    /// Peak sine amplitude in linear F32 units.
    #[arg(long, default_value_t = 0.20)]
    amplitude: f32,
}

#[derive(Debug, ClapArgs)]
struct LatencyArgs {
    /// Optional finite raw E-AC-3 elementary stream. If omitted, FFmpeg generates one locally.
    #[arg(long)]
    input: Option<PathBuf>,

    /// Generated E-AC-3 duration when --input is omitted.
    #[arg(long, default_value_t = 0.256)]
    seconds: f64,

    /// Generated E-AC-3 bitrate in kbit/s.
    #[arg(long, default_value_t = DEFAULT_BITRATE_KBPS)]
    bitrate_kbps: u32,

    /// Number of times to replay the proven finite AU set for statistical aggregation.
    #[arg(long, default_value_t = 4)]
    iterations: usize,
}

#[derive(Debug, ClapArgs)]
struct StressArgs {
    /// 5.1 -> 7.1 -> E-AC-3 -> canonical LPCM shared-output cycles.
    #[arg(long, default_value_t = 100)]
    switches: usize,

    /// Explicit reset + idle + resume cycles.
    #[arg(long, default_value_t = 1_000)]
    pause_resumes: usize,

    /// Continuous wall-clock soak duration. Full acceptance is 1800 seconds.
    #[arg(long, default_value_t = 1_800)]
    duration_seconds: u64,

    /// Generated E-AC-3 bitrate in kbit/s.
    #[arg(long, default_value_t = DEFAULT_BITRATE_KBPS)]
    bitrate_kbps: u32,

    /// Inject one nonfatal transport fault every N soak periods. Zero disables periodic injection.
    #[arg(long, default_value_t = 257)]
    inject_every: usize,

    /// Maximum allowed RSS growth during the post-warmup soak.
    #[arg(long, default_value_t = 5.0)]
    memory_growth_percent: f64,

    /// Do not pace the soak at the nominal 32 ms E-AC-3 carrier cadence. Intended for CI smoke runs.
    #[arg(long)]
    unpaced: bool,
}

fn main() -> Result<()> {
    match Cli::parse().command {
        SimCommand::ChannelId(args) => run_channel_id(args),
        SimCommand::LatencyReport(args) => run_latency_report(args),
        SimCommand::Stress(args) => run_stress(args),
    }
}

fn output_format() -> AudioFormat {
    AudioFormat {
        sample_rate: SAMPLE_RATE,
        channel_count: CHANNELS,
        sample_type: SampleType::F32,
        block_size: BLOCK_FRAMES,
    }
}

fn run_channel_id(args: ChannelIdArgs) -> Result<()> {
    if !args.seconds_per_channel.is_finite() || args.seconds_per_channel <= 0.0 {
        bail!("seconds-per-channel must be finite and greater than zero");
    }
    if !args.amplitude.is_finite()
        || !(0.0..=1.0).contains(&args.amplitude)
        || args.amplitude == 0.0
    {
        bail!("amplitude must be finite and in (0, 1]");
    }

    let frames_per_channel = (args.seconds_per_channel * f64::from(SAMPLE_RATE)).round();
    if frames_per_channel < 1.0 || frames_per_channel > usize::MAX as f64 {
        bail!("seconds-per-channel produces an invalid frame count");
    }
    let frames_per_channel = frames_per_channel as usize;
    let total_frames = frames_per_channel
        .checked_mul(CHANNELS)
        .context("channel-ID frame count overflow")?;

    let file = File::create(&args.output)
        .with_context(|| format!("failed creating {}", args.output.display()))?;
    let mut writer = BufWriter::new(file);
    write_f32_extensible_wav_header(&mut writer, total_frames)?;

    let roles = StandardLayout::SevenOneFour.canonical_roles();
    let wave_slots = canonical_to_wave_slots()?;
    for (index, role) in roles.iter().enumerate() {
        let start = index as f64 * args.seconds_per_channel;
        let end = start + args.seconds_per_channel;
        let wave_slot = wave_slots[index];
        eprintln!(
            "channel-id index={index:02} role={} wave_slot={wave_slot:02} frequency_hz={:.1} start_s={start:.3} end_s={end:.3}",
            role,
            CHANNEL_ID_FREQUENCIES_HZ[index]
        );

        // WAVE_FORMAT_EXTENSIBLE orders interleaved channels by ascending speaker
        // mask bit. Aurora's internal canonical order intentionally differs for
        // SL/SR vs SBL/SBR, so map the selected canonical role to its WAVE slot
        // rather than mislabelling a canonical interleave with a standard mask.
        for local_frame in 0..frames_per_channel {
            let phase = std::f32::consts::TAU
                * CHANNEL_ID_FREQUENCIES_HZ[index]
                * local_frame as f32
                / SAMPLE_RATE as f32;
            let active = phase.sin() * args.amplitude;
            for channel in 0..CHANNELS {
                let sample = if channel == wave_slot { active } else { 0.0 };
                writer
                    .write_all(&sample.to_le_bytes())
                    .context("failed writing channel-ID PCM")?;
            }
        }
    }
    writer.flush().context("failed flushing channel-ID WAV")?;
    println!(
        "PASS channel-id layout=7.1.4 channels={CHANNELS} sample_rate={SAMPLE_RATE} output={}",
        args.output.display()
    );
    Ok(())
}

fn write_f32_extensible_wav_header<W: Write>(writer: &mut W, frames: usize) -> Result<()> {
    let block_align = u16::try_from(CHANNELS * std::mem::size_of::<f32>())
        .context("WAV block alignment overflow")?;
    let data_bytes = frames
        .checked_mul(usize::from(block_align))
        .context("WAV data size overflow")?;
    let data_bytes = u32::try_from(data_bytes).context("channel-ID WAV exceeds RIFF32 limit")?;
    let byte_rate = SAMPLE_RATE
        .checked_mul(u32::from(block_align))
        .context("WAV byte-rate overflow")?;
    let riff_size = 60_u32
        .checked_add(data_bytes)
        .context("WAV RIFF size overflow")?;
    let channel_mask = canonical_channel_mask()?;

    writer.write_all(b"RIFF")?;
    writer.write_all(&riff_size.to_le_bytes())?;
    writer.write_all(b"WAVE")?;
    writer.write_all(b"fmt ")?;
    writer.write_all(&40_u32.to_le_bytes())?;
    writer.write_all(&WAVE_FORMAT_EXTENSIBLE.to_le_bytes())?;
    writer.write_all(&(CHANNELS as u16).to_le_bytes())?;
    writer.write_all(&SAMPLE_RATE.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&32_u16.to_le_bytes())?;
    writer.write_all(&22_u16.to_le_bytes())?;
    writer.write_all(&32_u16.to_le_bytes())?;
    writer.write_all(&channel_mask.to_le_bytes())?;
    writer.write_all(&WAVE_FLOAT_SUBFORMAT)?;
    writer.write_all(b"data")?;
    writer.write_all(&data_bytes.to_le_bytes())?;
    Ok(())
}

fn canonical_channel_mask() -> Result<u32> {
    let mut mask = 0_u32;
    for role in StandardLayout::SevenOneFour.canonical_roles() {
        let bit = role
            .wav_channel_mask_bit()
            .with_context(|| format!("canonical role {role} has no WAVE channel-mask bit"))?;
        mask |= bit;
    }
    Ok(mask)
}

fn canonical_to_wave_slots() -> Result<[usize; CHANNELS]> {
    let roles = StandardLayout::SevenOneFour.canonical_roles();
    if roles.len() != CHANNELS {
        bail!("canonical 7.1.4 role count changed unexpectedly");
    }
    let mut slots = [0_usize; CHANNELS];
    for (index, role) in roles.iter().enumerate() {
        let bit = role
            .wav_channel_mask_bit()
            .with_context(|| format!("canonical role {role} has no WAVE channel-mask bit"))?;
        let mut slot = 0_usize;
        for other in roles {
            let other_bit = other
                .wav_channel_mask_bit()
                .with_context(|| format!("canonical role {other} has no WAVE channel-mask bit"))?;
            if other_bit < bit {
                slot += 1;
            }
        }
        slots[index] = slot;
    }
    Ok(slots)
}

fn run_latency_report(args: LatencyArgs) -> Result<()> {
    if args.iterations == 0 {
        bail!("iterations must be greater than zero");
    }
    let units = load_access_units(args.input.as_deref(), args.seconds, args.bitrate_kbps)?;
    let mut normalizer = S32LeCarrierNormalizer::new(2, DIRECT_EARC_REFERENCE_WORD_HALF)?;
    let mut parser = BurstParser::new(CodecFilter::Eac3);
    let mut engine = AuroraDecoderEngine::new(EngineConfig::default());
    engine.configure(output_format())?;
    let mut output = SpeakerOutputStage::new(output_format(), OutputDspConfig::default())?;
    let mut stages = StageLatencyBook::new();
    let mut software_chain = FixedLatencyHistogram::new();
    let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut words = vec![0_i32; EAC3_BURST_PERIOD_BYTES / 2];
    let mut carrier_scratch = Vec::with_capacity(EAC3_BURST_PERIOD_BYTES);
    let mut sink_scratch = vec![0.0_f32; CHANNELS * MAX_VALIDATION_OUTPUT_FRAMES];
    let mut decoded_frames = 0_u64;

    for _ in 0..args.iterations {
        for unit in &units {
            write_eac3_period(unit, &mut period)
                .map_err(|error| anyhow::anyhow!("failed building validation carrier: {error}"))?;
            carrier_to_reference_s32(&period, &mut words)?;
            let chain_start = Instant::now();

            let capture_start = Instant::now();
            normalizer.push_s32_words_into(&words, &mut carrier_scratch)?;
            stages.record(ValidationStage::Capture, capture_start.elapsed());
            if carrier_scratch.as_slice() != period.as_slice() {
                bail!("simulated S32 capture normalization changed IEC61937 carrier bytes");
            }

            let parser_start = Instant::now();
            let observations = parser.push(&carrier_scratch);
            stages.record(ValidationStage::Parser, parser_start.elapsed());
            if observations.len() != 1 || observations[0].burst.payload != *unit {
                bail!("IEC61937 parser failed byte-exact one-AU latency validation");
            }

            let decode_start = Instant::now();
            let first = engine.decode_complete_eac3_access_unit(unit)?;
            stages.record(ValidationStage::Decode, decode_start.elapsed());
            decoded_frames = decoded_frames.saturating_add(drain_decoder_frames(
                &mut engine,
                &mut output,
                &mut stages,
                &mut sink_scratch,
                first,
            )? as u64);

            let joc = engine.joc_health();
            if joc.codec_classified_joc {
                if let Some(render_us) = joc.last_render_time_us {
                    stages.record_us(ValidationStage::JocRender, render_us);
                }
            }
            software_chain.record(chain_start.elapsed());
        }
    }

    let flush_start = Instant::now();
    engine.flush_pending()?;
    stages.record(ValidationStage::Decode, flush_start.elapsed());
    decoded_frames = decoded_frames.saturating_add(drain_decoder_frames(
        &mut engine,
        &mut output,
        &mut stages,
        &mut sink_scratch,
        None,
    )? as u64);
    normalizer.finish()?;
    parser.finish()?;

    println!("Aurora latency report (headless software isolation)");
    println!("stage\tsamples\tp50_us\tp99_us\tmax_us\toverflow");
    for stage in ValidationStage::ALL {
        let summary = stages.summary(stage);
        if summary.samples == 0 {
            println!("{}\tNOT_MEASURED", stage.label());
        } else {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}",
                stage.label(),
                summary.samples,
                summary.p50_us,
                summary.p99_us,
                summary.max_us,
                summary.overflow_samples
            );
        }
    }
    let total = software_chain.summary();
    println!(
        "software_chain\t{}\t{}\t{}\t{}\t{}",
        total.samples, total.p50_us, total.p99_us, total.max_us, total.overflow_samples
    );
    println!(
        "PASS latency-report access_units={} iterations={} decoded_frames={} capture=SIMULATED_S32_NORMALIZE output=SIMULATED_PREALLOCATED_COPY physical_io=NOT_MEASURED",
        units.len(),
        args.iterations,
        decoded_frames
    );
    Ok(())
}

fn drain_decoder_frames(
    engine: &mut AuroraDecoderEngine,
    output: &mut SpeakerOutputStage,
    stages: &mut StageLatencyBook,
    sink_scratch: &mut [f32],
    first: Option<DecodedFrame>,
) -> Result<usize> {
    let mut emitted = 0_usize;
    let mut next = first;
    loop {
        if let Some(frame) = next.take() {
            let output_start = Instant::now();
            let speaker = output.process_decoded_frame_ref(&frame)?;
            stages.record(
                ValidationStage::SpeakerPostProcessor,
                output_start.elapsed(),
            );
            if speaker.frame_count == 0
                || speaker.interleaved_f32.len() != speaker.frame_count * CHANNELS
                || speaker.interleaved_f32.iter().any(|sample| !sample.is_finite())
            {
                bail!("speaker postprocessor emitted invalid canonical PCM");
            }
            let samples = speaker.interleaved_f32.len();
            if samples > sink_scratch.len() {
                bail!(
                    "speaker output block requires {samples} samples; validation sink capacity is {}",
                    sink_scratch.len()
                );
            }
            let sink_start = Instant::now();
            sink_scratch[..samples].copy_from_slice(&speaker.interleaved_f32);
            std::hint::black_box(sink_scratch[0]);
            stages.record(ValidationStage::Output, sink_start.elapsed());
            output.recycle_output_frame(speaker);
            engine.recycle_decoded_frame(frame);
            emitted = emitted.saturating_add(1);
        }

        let poll_start = Instant::now();
        next = engine.decode_chunk(&[])?;
        stages.record(ValidationStage::Decode, poll_start.elapsed());
        if next.is_none() {
            break;
        }
    }
    Ok(emitted)
}

fn run_stress(args: StressArgs) -> Result<()> {
    if args.duration_seconds == 0 {
        bail!("duration-seconds must be greater than zero");
    }
    if !args.memory_growth_percent.is_finite() || args.memory_growth_percent < 0.0 {
        bail!("memory-growth-percent must be finite and non-negative");
    }

    let units = generate_access_units(0.256, args.bitrate_kbps)?;
    let mut runtime = playback_runtime()?;
    let mut pcm_stage = SpeakerOutputStage::new(output_format(), OutputDspConfig::default())?;
    let mut pcm_frame = silent_frame(480);
    let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut words = vec![0_i32; EAC3_BURST_PERIOD_BYTES / 2];
    let idle_words = vec![0_i32; EAC3_BURST_PERIOD_BYTES / 2];
    let jitter_words = vec![0_i32; EAC3_CARRIER_BYTES_PER_MS / 2];
    let inject_every = u64::try_from(args.inject_every).context("inject-every exceeds u64")?;
    let mut unit_index = 0_usize;
    let mut output_frames = 0_u64;
    let mut handled_expected_errors = 0_u64;

    for _ in 0..units.len().min(8) {
        output_frames = output_frames.saturating_add(feed_valid_unit(
            &mut runtime,
            &units[unit_index % units.len()],
            &mut period,
            &mut words,
        )? as u64);
        unit_index = unit_index.wrapping_add(1);
    }
    verify_channel_identity(&mut pcm_stage, &mut pcm_frame)?;

    // The PCM legs start at Aurora's canonical speaker boundary. They stress the
    // shared SpeakerPostProcessor, but do not claim raw eARC LPCM negotiation.
    for cycle in 0..args.switches {
        fill_pcm_profile(&mut pcm_frame, 6, cycle as u64);
        process_pcm_profile(&mut pcm_stage, &pcm_frame)?;
        fill_pcm_profile(&mut pcm_frame, 8, cycle as u64 + 1);
        process_pcm_profile(&mut pcm_stage, &pcm_frame)?;
        output_frames = output_frames.saturating_add(feed_valid_unit(
            &mut runtime,
            &units[unit_index % units.len()],
            &mut period,
            &mut words,
        )? as u64);
        unit_index = unit_index.wrapping_add(1);
        fill_pcm_profile(&mut pcm_frame, 12, cycle as u64 + 2);
        process_pcm_profile(&mut pcm_stage, &pcm_frame)?;
    }

    for _ in 0..args.pause_resumes {
        let idle = runtime.push_direct_s32_words(&idle_words)?;
        recycle_playback_batch(&mut runtime, idle)?;
        runtime.reset();
        output_frames = output_frames.saturating_add(feed_valid_unit(
            &mut runtime,
            &units[unit_index % units.len()],
            &mut period,
            &mut words,
        )? as u64);
        unit_index = unit_index.wrapping_add(1);
    }

    // Warm all reset/switch paths before taking the leak baseline.
    output_frames = output_frames.saturating_add(feed_valid_unit(
        &mut runtime,
        &units[unit_index % units.len()],
        &mut period,
        &mut words,
    )? as u64);
    unit_index = unit_index.wrapping_add(1);

    let baseline_rss = rss_bytes_linux();
    let mut max_rss = baseline_rss.unwrap_or(0);
    let soak_start = Instant::now();
    let soak_duration = Duration::from_secs(args.duration_seconds);
    let mut next_tick = Instant::now();
    let mut periods = 0_u64;
    let mut fault_sequence = 0_u64;

    while soak_start.elapsed() < soak_duration {
        let unit = &units[unit_index % units.len()];
        write_eac3_period(unit, &mut period)
            .map_err(|error| anyhow::anyhow!("failed building stress carrier: {error}"))?;
        carrier_to_reference_s32(&period, &mut words)?;

        let inject = inject_every != 0 && periods != 0 && periods % inject_every == 0;
        if inject {
            match fault_sequence % 5 {
                0 => {
                    period[0] ^= 0x01;
                    carrier_to_reference_s32(&period, &mut words)?;
                    let batch = runtime.push_direct_s32_words(&words)?;
                    recycle_playback_batch(&mut runtime, batch)?;
                }
                1 => {
                    let batch = runtime.push_direct_s32_words(&idle_words)?;
                    recycle_playback_batch(&mut runtime, batch)?;
                }
                2 => {
                    let batch = runtime.push_direct_s32_words(&idle_words)?;
                    recycle_playback_batch(&mut runtime, batch)?;
                    runtime.reset();
                }
                3 => {
                    let jitter = runtime.push_direct_s32_words(&jitter_words)?;
                    recycle_playback_batch(&mut runtime, jitter)?;
                    let batch = runtime.push_direct_s32_words(&words)?;
                    output_frames = output_frames
                        .saturating_add(recycle_playback_batch(&mut runtime, batch)? as u64);
                }
                _ => {
                    let cut_words = aligned_mid_payload_cut_words(unit.len(), words.len())?;
                    let prefix = runtime.push_direct_s32_words(&words[..cut_words])?;
                    recycle_playback_batch(&mut runtime, prefix)?;
                    match runtime.push_direct_s32_words(&words) {
                        Ok(batch) => {
                            let accepted = batch.bursts;
                            recycle_playback_batch(&mut runtime, batch)?;
                            bail!(
                                "cut-burst fault was silently accepted after resumption ({accepted} burst(s)); refusing silent corruption"
                            );
                        }
                        Err(_) => {
                            handled_expected_errors = handled_expected_errors.saturating_add(1);
                            runtime.reset();
                        }
                    }
                }
            }
            fault_sequence = fault_sequence.saturating_add(1);
        } else {
            let batch = runtime.push_direct_s32_words(&words)?;
            output_frames = output_frames
                .saturating_add(recycle_playback_batch(&mut runtime, batch)? as u64);
        }

        periods = periods.saturating_add(1);
        unit_index = unit_index.wrapping_add(1);

        // Out-of-band observer only; not part of the measured/realtime audio path.
        if periods % 128 == 0 {
            if let Some(rss) = rss_bytes_linux() {
                max_rss = max_rss.max(rss);
            }
        }

        if !args.unpaced {
            next_tick += EAC3_PERIOD;
            let now = Instant::now();
            if next_tick > now {
                thread::sleep(next_tick.duration_since(now));
            }
        }
    }

    if let Some(rss) = rss_bytes_linux() {
        max_rss = max_rss.max(rss);
    }

    handled_expected_errors = handled_expected_errors.saturating_add(
        verify_truncated_eof_is_rejected(&units[0])? as u64,
    );
    verify_channel_identity(&mut pcm_stage, &mut pcm_frame)?;

    let rss_growth = match baseline_rss {
        Some(baseline) if baseline > 0 => {
            (max_rss.saturating_sub(baseline) as f64 * 100.0) / baseline as f64
        }
        _ => 0.0,
    };
    let rss_available = baseline_rss.is_some();

    println!("Aurora stress report");
    println!("format_switch_cycles={}", args.switches);
    println!("pause_resume_cycles={}", args.pause_resumes);
    println!("soak_seconds={}", args.duration_seconds);
    println!("soak_periods={periods}");
    println!("fault_injections={fault_sequence}");
    println!("handled_expected_errors={handled_expected_errors}");
    println!("unhandled_errors=0");
    println!("speaker_output_frames={output_frames}");
    println!("channel_swap=0");
    println!("xruns=N/A(file-mode)");
    println!("capture_queue_starvations=N/A(file-mode)");
    println!("raw_lpcm_input_switch_proven=false");
    if rss_available {
        println!("rss_baseline_bytes={}", baseline_rss.unwrap_or(0));
        println!("rss_max_bytes={max_rss}");
        println!("rss_growth_percent={rss_growth:.3}");
    } else {
        println!("rss_growth_percent=N/A(non-Linux)");
    }

    if rss_available && rss_growth > args.memory_growth_percent {
        bail!(
            "RSS growth {rss_growth:.3}% exceeds {:.3}% acceptance threshold",
            args.memory_growth_percent
        );
    }
    println!(
        "PASS stress software_chain=true memory_limit_percent={:.3} xruns=NOT_MEASURED hardware_required=true",
        args.memory_growth_percent
    );
    Ok(())
}

fn playback_runtime() -> Result<AuroraPlaybackRuntime> {
    let runtime = AuroraPlaybackRuntime::new(
        EncodedInputConfig::DirectEarc {
            slots: 2,
            word_half: DIRECT_EARC_REFERENCE_WORD_HALF,
        },
        EngineConfig::default(),
        output_format(),
        OutputDspConfig::default(),
    )?;
    Ok(runtime)
}

fn feed_valid_unit(
    runtime: &mut AuroraPlaybackRuntime,
    unit: &[u8],
    period: &mut [u8; EAC3_BURST_PERIOD_BYTES],
    words: &mut [i32],
) -> Result<usize> {
    write_eac3_period(unit, period)
        .map_err(|error| anyhow::anyhow!("failed building E-AC-3 stress period: {error}"))?;
    carrier_to_reference_s32(period, words)?;
    let batch = runtime.push_direct_s32_words(words)?;
    recycle_playback_batch(runtime, batch)
}

fn recycle_playback_batch(
    runtime: &mut AuroraPlaybackRuntime,
    batch: PlaybackBatch,
) -> Result<usize> {
    let mut frames = 0_usize;
    for frame in batch.frames {
        if frame.interleaved_f32.len() != frame.frame_count * CHANNELS
            || frame.interleaved_f32.iter().any(|sample| !sample.is_finite())
        {
            bail!("runtime emitted malformed or non-finite speaker PCM");
        }
        runtime.recycle_output_frame(frame);
        frames = frames.saturating_add(1);
    }
    Ok(frames)
}

fn carrier_to_reference_s32(carrier: &[u8], words: &mut [i32]) -> Result<()> {
    if carrier.len() % 2 != 0 || words.len() != carrier.len() / 2 {
        bail!("carrier/S32 validation buffer geometry mismatch");
    }
    for (destination, source) in words.iter_mut().zip(carrier.chunks_exact(2)) {
        let word = u32::from(u16::from_le_bytes([source[0], source[1]]));
        *destination = (word << 16) as i32;
    }
    Ok(())
}

fn aligned_mid_payload_cut_words(payload_bytes: usize, maximum_words: usize) -> Result<usize> {
    let payload_half = payload_bytes.max(8) / 2;
    let mut cut_bytes = 8_usize
        .checked_add(payload_half)
        .context("cut-burst length overflow")?;
    cut_bytes -= cut_bytes % 4;
    cut_bytes = cut_bytes.max(12);
    let cut_words = cut_bytes / 2;
    if cut_words >= maximum_words || cut_words % 2 != 0 {
        bail!("cut-burst geometry is not a complete two-slot S32 capture frame");
    }
    Ok(cut_words)
}

fn verify_truncated_eof_is_rejected(unit: &[u8]) -> Result<usize> {
    let mut runtime = playback_runtime()?;
    let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];
    write_eac3_period(unit, &mut period)
        .map_err(|error| anyhow::anyhow!("failed building truncated-EOF period: {error}"))?;
    let mut words = vec![0_i32; EAC3_BURST_PERIOD_BYTES / 2];
    carrier_to_reference_s32(&period, &mut words)?;
    let cut_words = aligned_mid_payload_cut_words(unit.len(), words.len())?;
    let batch = runtime.push_direct_s32_words(&words[..cut_words])?;
    recycle_playback_batch(&mut runtime, batch)?;
    match runtime.finish() {
        Ok(_) => bail!("truncated EOF unexpectedly passed finite IEC61937 validation"),
        Err(_) => Ok(1),
    }
}

fn silent_frame(frame_count: usize) -> DecodedFrame {
    DecodedFrame {
        audio: AudioBlock {
            channels: (0..CHANNELS)
                .map(|_| vec![0.0_f32; frame_count])
                .collect(),
            frame_count,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        },
        objects: Vec::new(),
    }
}

fn fill_pcm_profile(frame: &mut DecodedFrame, active_channels: usize, seed: u64) {
    for channel in &mut frame.audio.channels {
        channel.fill(0.0);
    }
    let active_channels = active_channels.min(CHANNELS);
    for channel in 0..active_channels {
        let frequency = 600.0_f32 + 70.0 * channel as f32 + (seed % 5) as f32;
        for (sample_index, sample) in frame.audio.channels[channel].iter_mut().enumerate() {
            let phase = std::f32::consts::TAU * frequency * sample_index as f32
                / SAMPLE_RATE as f32;
            *sample = 0.05 * phase.sin();
        }
    }
}

fn process_pcm_profile(stage: &mut SpeakerOutputStage, frame: &DecodedFrame) -> Result<()> {
    let output = stage.process_decoded_frame_ref(frame)?;
    if output.interleaved_f32.len() != output.frame_count * CHANNELS
        || output.interleaved_f32.iter().any(|sample| !sample.is_finite())
    {
        bail!("PCM profile produced malformed canonical speaker output");
    }
    stage.recycle_output_frame(output);
    Ok(())
}

fn verify_channel_identity(stage: &mut SpeakerOutputStage, frame: &mut DecodedFrame) -> Result<()> {
    let mut energy = [0.0_f64; CHANNELS];
    for target in 0..CHANNELS {
        for channel in &mut frame.audio.channels {
            channel.fill(0.0);
        }
        let frequency = CHANNEL_ID_FREQUENCIES_HZ[target];
        for (index, sample) in frame.audio.channels[target].iter_mut().enumerate() {
            let phase =
                std::f32::consts::TAU * frequency * index as f32 / SAMPLE_RATE as f32;
            *sample = 0.20 * phase.sin();
        }
        stage.reset();
        let output = stage.process_decoded_frame_ref(frame)?;
        energy.fill(0.0);
        for samples in output.interleaved_f32.chunks_exact(CHANNELS) {
            for channel in 0..CHANNELS {
                let value = f64::from(samples[channel]);
                energy[channel] += value * value;
            }
        }
        let dominant = energy
            .iter()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(right.1))
            .map(|(index, _)| index)
            .context("channel-ID output contained no channels")?;
        stage.recycle_output_frame(output);
        if dominant != target {
            bail!(
                "channel swap detected: input {} ({}) dominated output {} ({})",
                target,
                StandardLayout::SevenOneFour.canonical_roles()[target],
                dominant,
                StandardLayout::SevenOneFour.canonical_roles()[dominant]
            );
        }
    }
    Ok(())
}

fn load_access_units(
    input: Option<&Path>,
    seconds: f64,
    bitrate_kbps: u32,
) -> Result<Vec<Vec<u8>>> {
    match input {
        Some(path) => {
            let mut bytes = Vec::new();
            File::open(path)
                .with_context(|| format!("failed opening {}", path.display()))?
                .read_to_end(&mut bytes)
                .with_context(|| format!("failed reading {}", path.display()))?;
            frame_access_units(&bytes)
        }
        None => generate_access_units(seconds, bitrate_kbps),
    }
}

fn generate_access_units(seconds: f64, bitrate_kbps: u32) -> Result<Vec<Vec<u8>>> {
    if !seconds.is_finite() || seconds <= 0.0 {
        bail!("generated E-AC-3 seconds must be finite and greater than zero");
    }
    if bitrate_kbps == 0 {
        bail!("bitrate-kbps must be greater than zero");
    }
    let duration = format!("{seconds:.6}");
    let bitrate = format!("{bitrate_kbps}k");
    let generated = ProcessCommand::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=r=48000:cl=5.1",
            "-t",
            &duration,
            "-c:a",
            "eac3",
            "-b:a",
            &bitrate,
            "-ar",
            "48000",
            "-ac",
            "6",
            "-f",
            "eac3",
            "pipe:1",
        ])
        .output()
        .context("failed to launch FFmpeg E-AC-3 generator")?;
    if !generated.status.success() {
        bail!(
            "FFmpeg E-AC-3 generation failed: {}",
            String::from_utf8_lossy(&generated.stderr)
        );
    }
    frame_access_units(&generated.stdout)
}

fn frame_access_units(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    if bytes.is_empty() {
        bail!("E-AC-3 source is empty");
    }
    let mut framer = Eac3AccessUnitFramer::new();
    let mut units = framer
        .push(bytes)
        .map_err(|error| anyhow::anyhow!("E-AC-3 AU framing failed: {error}"))?;
    units.extend(
        framer
            .finish()
            .map_err(|error| anyhow::anyhow!("finite E-AC-3 framing failed: {error}"))?,
    );
    if units.is_empty() {
        bail!("E-AC-3 source produced no complete access units");
    }
    Ok(units)
}

fn rss_bytes_linux() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
        let kib = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
        kib.checked_mul(1_024)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_mask_covers_exact_canonical_roles() {
        let expected = StandardLayout::SevenOneFour
            .canonical_roles()
            .iter()
            .map(|role| role.wav_channel_mask_bit().unwrap())
            .fold(0_u32, |mask, bit| mask | bit);
        assert_eq!(canonical_channel_mask().unwrap(), expected);
        assert_eq!(
            StandardLayout::SevenOneFour.canonical_roles().len(),
            CHANNELS
        );
    }

    #[test]
    fn wave_slots_preserve_canonical_role_semantics() {
        assert_eq!(
            canonical_to_wave_slots().unwrap(),
            [0, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11]
        );
    }

    #[test]
    fn channel_id_frequencies_are_unique() {
        for left in 0..CHANNELS {
            for right in left + 1..CHANNELS {
                assert_ne!(
                    CHANNEL_ID_FREQUENCIES_HZ[left],
                    CHANNEL_ID_FREQUENCIES_HZ[right]
                );
            }
        }
    }

    #[test]
    fn reference_high_half_s32_conversion_round_trips_canonical_carrier() {
        let payload = [0x0B_u8, 0x77, 0x12, 0x34, 0x56, 0x78];
        let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];
        write_eac3_period(&payload, &mut period).unwrap();
        let mut words = vec![0_i32; EAC3_BURST_PERIOD_BYTES / 2];
        carrier_to_reference_s32(&period, &mut words).unwrap();
        let mut normalizer =
            S32LeCarrierNormalizer::new(2, DIRECT_EARC_REFERENCE_WORD_HALF).unwrap();
        let normalized = normalizer.push_s32_words(&words).unwrap();
        assert_eq!(normalized, period);
    }

    #[test]
    fn mid_payload_cut_preserves_two_slot_alignment() {
        let cut_words =
            aligned_mid_payload_cut_words(1_536, EAC3_BURST_PERIOD_BYTES / 2).unwrap();
        assert!(cut_words < EAC3_BURST_PERIOD_BYTES / 2);
        assert_eq!(cut_words % 2, 0);
    }

    #[test]
    fn ordinary_ffmpeg_eac3_does_not_promote_transport_type_to_joc() {
        let units = generate_access_units(0.128, DEFAULT_BITRATE_KBPS).unwrap();
        let mut engine = AuroraDecoderEngine::new(EngineConfig::default());
        engine.configure(output_format()).unwrap();
        for unit in units {
            let mut frame = engine.decode_complete_eac3_access_unit(&unit).unwrap();
            while let Some(decoded) = frame {
                engine.recycle_decoded_frame(decoded);
                frame = engine.decode_chunk(&[]).unwrap();
            }
        }
        engine.flush_pending().unwrap();
        while let Some(decoded) = engine.decode_chunk(&[]).unwrap() {
            engine.recycle_decoded_frame(decoded);
        }
        assert!(
            !engine.joc_health().codec_classified_joc,
            "ordinary FFmpeg E-AC-3 must not be promoted to JOC from IEC61937 type 0x15"
        );
    }

    #[test]
    fn runtime_rejects_cut_burst_instead_of_emitting_silent_corruption() {
        let units = generate_access_units(0.128, DEFAULT_BITRATE_KBPS).unwrap();
        let unit = &units[0];
        let mut runtime = playback_runtime().unwrap();
        let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];
        write_eac3_period(unit, &mut period).unwrap();
        let mut words = vec![0_i32; EAC3_BURST_PERIOD_BYTES / 2];
        carrier_to_reference_s32(&period, &mut words).unwrap();
        let cut_words = aligned_mid_payload_cut_words(unit.len(), words.len()).unwrap();

        let prefix = runtime.push_direct_s32_words(&words[..cut_words]).unwrap();
        assert_eq!(prefix.bursts, 0);
        recycle_playback_batch(&mut runtime, prefix).unwrap();
        assert!(
            runtime.push_direct_s32_words(&words).is_err(),
            "mid-payload cut followed by a new period must not be accepted as valid audio"
        );
    }

    #[test]
    fn runtime_rejects_truncated_finite_eof() {
        let units = generate_access_units(0.128, DEFAULT_BITRATE_KBPS).unwrap();
        assert_eq!(verify_truncated_eof_is_rejected(&units[0]).unwrap(), 1);
    }
}
