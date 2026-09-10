//! Threaded direct-eARC realtime proof for Aurora.
//!
//! Capture owns its ALSA handle on a dedicated thread and transfers period
//! buffers by ownership through a bounded queue. Buffers are recycled back to
//! the producer, so the steady-state capture path neither copies carrier words
//! nor allocates one vector per ALSA period. Decode, speaker DSP and playback
//! remain on the consumer thread.

#[cfg(target_os = "linux")]
use std::collections::VecDeque;
#[cfg(target_os = "linux")]
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
#[cfg(target_os = "linux")]
use aurora_alsa_input::{AlsaInputConfig, NativeAlsaCapture, OwnedCaptureBlock};
#[cfg(target_os = "linux")]
use aurora_alsa_output::{AlsaOutputConfig, NativeAlsaPlayback};
#[cfg(target_os = "linux")]
use aurora_core::{AudioFormat, SampleType};
#[cfg(target_os = "linux")]
use aurora_decoder_engine::EngineConfig;
#[cfg(target_os = "linux")]
use aurora_dsp_basic::output::{
    OutputDspConfig, CHANNELS as OUTPUT_CHANNELS, SAMPLE_RATE as OUTPUT_SAMPLE_RATE,
};
#[cfg(target_os = "linux")]
use aurora_encoded_input::EncodedInputConfig;
#[cfg(target_os = "linux")]
use aurora_encoded_runtime::AuroraPlaybackRuntime;
#[cfg(target_os = "linux")]
use aurora_iec61937::CarrierWordHalf;
use clap::Parser;

#[cfg(target_os = "linux")]
const CARRIER_RATE: u32 = 192_000;
#[cfg(target_os = "linux")]
const CARRIER_SLOTS: usize = 2;
#[cfg(target_os = "linux")]
const CAPTURE_PERIOD_FRAMES: usize = 1_024;
#[cfg(target_os = "linux")]
const CAPTURE_BUFFER_FRAMES: usize = 8_192;
#[cfg(target_os = "linux")]
const PLAYBACK_PERIOD_FRAMES: usize = 256;
#[cfg(target_os = "linux")]
const PLAYBACK_BUFFER_FRAMES: usize = 1_024;
#[cfg(target_os = "linux")]
const DSP_BLOCK_FRAMES: usize = 40;
#[cfg(target_os = "linux")]
const MAX_QUEUE_DEPTH: usize = 256;

#[derive(Debug, Parser)]
#[command(
    name = "aurora-threaded-earc-runtime",
    about = "Run Aurora direct eARC with decoupled ALSA capture and bounded buffer recycling"
)]
struct Args {
    /// Native ALSA capture endpoint carrying recovered eARC S32_LE words.
    #[arg(long)]
    capture_device: String,
    /// Native ALSA/ASoC speaker/TDM playback endpoint.
    #[arg(long)]
    output_device: String,
    /// Physical output width. Aurora fills the first 12 canonical channels and
    /// zero-pads any wider TDM slots.
    #[arg(long, default_value_t = 12)]
    hardware_output_channels: usize,
    /// Number of owned capture periods allowed between producer and consumer.
    /// The same number of sample vectors is allocated once and then recycled.
    #[arg(long, default_value_t = 16)]
    queue_depth: usize,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy)]
struct CaptureCounters {
    xruns: u64,
    recoveries: u64,
    discontinuities: u64,
    queue_starvations: u64,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct CapturePacket {
    block: OwnedCaptureBlock,
    counters: CaptureCounters,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
enum CaptureMessage {
    Block(CapturePacket),
    Error(String),
}

#[cfg(target_os = "linux")]
fn spawn_capture_thread(
    config: AlsaInputConfig,
    queue_depth: usize,
) -> Result<(
    Receiver<CaptureMessage>,
    SyncSender<Vec<i32>>,
    std::thread::JoinHandle<()>,
)> {
    if !(2..=MAX_QUEUE_DEPTH).contains(&queue_depth) {
        bail!("queue depth must be between 2 and {MAX_QUEUE_DEPTH} capture periods");
    }

    let (filled_tx, filled_rx) = sync_channel::<CaptureMessage>(queue_depth);
    let (recycle_tx, recycle_rx) = sync_channel::<Vec<i32>>(queue_depth);
    let worker = std::thread::Builder::new()
        .name("aurora-earc-capture".to_owned())
        .spawn(move || {
            let mut capture = match NativeAlsaCapture::open(config) {
                Ok(capture) => capture,
                Err(error) => {
                    let _ = filled_tx.send(CaptureMessage::Error(format!(
                        "failed to open native direct-eARC capture: {error}"
                    )));
                    return;
                }
            };

            let geometry = capture.telemetry();
            let sample_count = match geometry.period_frames.checked_mul(geometry.channels) {
                Some(value) if value > 0 => value,
                _ => {
                    let _ = filled_tx.send(CaptureMessage::Error(
                        "negotiated capture period/channel product is invalid".to_owned(),
                    ));
                    return;
                }
            };

            let mut free = VecDeque::with_capacity(queue_depth);
            for _ in 0..queue_depth {
                free.push_back(vec![0_i32; sample_count]);
            }
            let mut queue_starvations = 0_u64;

            loop {
                while let Ok(buffer) = recycle_rx.try_recv() {
                    free.push_back(buffer);
                }
                let replacement = match free.pop_front() {
                    Some(buffer) => buffer,
                    None => {
                        queue_starvations = queue_starvations.saturating_add(1);
                        match recycle_rx.recv() {
                            Ok(buffer) => buffer,
                            Err(_) => return,
                        }
                    }
                };

                let block = match capture.read_owned_block(replacement) {
                    Ok(block) => block,
                    Err(error) => {
                        let _ = filled_tx.send(CaptureMessage::Error(format!(
                            "native direct-eARC capture failed: {error}"
                        )));
                        return;
                    }
                };
                let telemetry = capture.telemetry();
                let counters = CaptureCounters {
                    xruns: telemetry.xruns,
                    recoveries: telemetry.recoveries,
                    discontinuities: telemetry.discontinuities,
                    queue_starvations,
                };
                if filled_tx
                    .send(CaptureMessage::Block(CapturePacket { block, counters }))
                    .is_err()
                {
                    return;
                }
            }
        })
        .context("failed spawning Aurora eARC capture thread")?;

    Ok((filled_rx, recycle_tx, worker))
}

#[cfg(target_os = "linux")]
fn run(args: Args) -> Result<()> {
    if args.hardware_output_channels < OUTPUT_CHANNELS {
        bail!(
            "hardware output exposes {} channels but Aurora 7.1.4 requires at least {OUTPUT_CHANNELS}",
            args.hardware_output_channels
        );
    }

    let mut runtime = AuroraPlaybackRuntime::new(
        EncodedInputConfig::DirectEarc {
            slots: CARRIER_SLOTS,
            word_half: CarrierWordHalf::High,
        },
        EngineConfig::default(),
        AudioFormat {
            sample_rate: OUTPUT_SAMPLE_RATE,
            channel_count: OUTPUT_CHANNELS,
            sample_type: SampleType::F32,
            block_size: DSP_BLOCK_FRAMES,
        },
        OutputDspConfig::default(),
    )
    .context("failed initializing Aurora threaded direct-eARC runtime")?;

    // Bring the speaker sink to a prepared state before capture starts. This
    // prevents ALSA playback setup latency from consuming the bounded capture
    // queue or hardware headroom during startup.
    let mut playback = NativeAlsaPlayback::open(AlsaOutputConfig {
        device: args.output_device,
        sample_rate: OUTPUT_SAMPLE_RATE,
        logical_channels: OUTPUT_CHANNELS,
        hardware_channels: args.hardware_output_channels,
        period_frames: PLAYBACK_PERIOD_FRAMES,
        buffer_frames: PLAYBACK_BUFFER_FRAMES,
    })
    .context("failed opening Aurora native speaker output")?;

    let capture_config = AlsaInputConfig {
        device: args.capture_device,
        sample_rate: CARRIER_RATE,
        channels: CARRIER_SLOTS,
        period_frames: CAPTURE_PERIOD_FRAMES,
        buffer_frames: CAPTURE_BUFFER_FRAMES,
    };
    let (captured_rx, recycle_tx, capture_thread) =
        spawn_capture_thread(capture_config, args.queue_depth)?;

    let mut suppress_next_frame_discontinuity = false;
    let mut carrier_periods = 0_u64;
    let mut decoded_pcm_frames = 0_u64;
    let mut last_report = Instant::now();
    let mut last_capture = CaptureCounters {
        xruns: 0,
        recoveries: 0,
        discontinuities: 0,
        queue_starvations: 0,
    };

    loop {
        let packet = match captured_rx.recv() {
            Ok(CaptureMessage::Block(packet)) => packet,
            Ok(CaptureMessage::Error(error)) => bail!("{error}"),
            Err(_) => break,
        };
        carrier_periods = carrier_periods.saturating_add(1);
        last_capture = packet.counters;

        let capture_discontinuity = packet.block.discontinuity;
        if capture_discontinuity {
            runtime.reset();
            playback
                .reset_for_discontinuity()
                .context("failed resetting playback after capture discontinuity")?;
            suppress_next_frame_discontinuity = true;
        }

        let batch = runtime
            .push_direct_s32_words(&packet.block.interleaved_s32)
            .context("threaded direct-eARC carrier ingest failed")?;

        // A parser/codec/source transition can create a transport discontinuity
        // even when ALSA capture itself stayed healthy. Drop queued pre-transition
        // speaker PCM in that case too, but do not reset twice when the same
        // period already carried an ALSA-recovery discontinuity.
        if batch.discontinuity && !capture_discontinuity {
            playback
                .reset_for_discontinuity()
                .context("failed resetting playback after decoded transport discontinuity")?;
            suppress_next_frame_discontinuity = true;
        }

        for frame in batch.frames {
            decoded_pcm_frames =
                decoded_pcm_frames.saturating_add(frame.frame_count as u64);
            let already_reset = std::mem::take(&mut suppress_next_frame_discontinuity);
            let discontinuity = frame.discontinuity && !already_reset;
            playback
                .write_interleaved_f32(&frame.interleaved_f32, discontinuity)
                .context("threaded native speaker write failed")?;
        }

        recycle_tx
            .send(packet.block.interleaved_s32)
            .context("capture worker stopped while recycling period buffer")?;

        if last_report.elapsed() >= Duration::from_secs(5) {
            let transport = runtime.encoded().decoder().transport_telemetry();
            let joc = runtime.encoded().decoder().engine().joc_health();
            let output = playback.telemetry();
            eprintln!(
                "aurora-threaded-earc: periods={} pcm_frames={} capture_xruns={} capture_recoveries={} capture_discontinuities={} capture_queue_starvations={} parser_pending={} bursts={} spacing={:?} joc={} joc_render={} joc_total_us={:?} joc_max_us={:?} output_xruns={} output_recoveries={}",
                carrier_periods,
                decoded_pcm_frames,
                last_capture.xruns,
                last_capture.recoveries,
                last_capture.discontinuities,
                last_capture.queue_starvations,
                transport.pending_carrier_bytes,
                transport.total_bursts,
                transport.last_burst_spacing_bytes,
                joc.codec_classified_joc,
                joc.speaker_render_active,
                joc.last_total_time_us,
                joc.max_total_time_us,
                output.xruns,
                output.recoveries,
            );
            last_report = Instant::now();
        }
    }

    drop(recycle_tx);
    capture_thread
        .join()
        .map_err(|_| anyhow::anyhow!("Aurora eARC capture thread panicked"))?;
    playback.drain().context("failed draining native speaker output")?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    #[cfg(target_os = "linux")]
    {
        return run(args);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        bail!("aurora-threaded-earc-runtime requires Linux ALSA")
    }
}
