//! Bounded threaded native ALSA capture used by `aurora-encoded-runtime`.
//!
//! The capture PCM handle is owned by a dedicated producer thread. Captured
//! period buffers move by ownership through a bounded channel and are recycled
//! back to the producer, avoiding carrier copies and steady-state Vec allocation.

#[cfg(target_os = "linux")]
use std::collections::VecDeque;
#[cfg(target_os = "linux")]
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

use anyhow::{bail, Context, Result};
#[cfg(target_os = "linux")]
use aurora_alsa_input::{AlsaInputConfig, NativeAlsaCapture, OwnedCaptureBlock};
#[cfg(target_os = "linux")]
use aurora_encoded_runtime::health::HealthReporter;
#[cfg(target_os = "linux")]
use aurora_encoded_runtime::{AuroraPlaybackRuntime, PlaybackBatch};

#[cfg(target_os = "linux")]
use super::{Args, RuntimeStats, SpeakerSink};

#[cfg(target_os = "linux")]
const MIN_QUEUE_DEPTH: usize = 2;
#[cfg(target_os = "linux")]
const MAX_QUEUE_DEPTH: usize = 256;

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
    if !(MIN_QUEUE_DEPTH..=MAX_QUEUE_DEPTH).contains(&queue_depth) {
        bail!(
            "native capture queue depth must be between {MIN_QUEUE_DEPTH} and {MAX_QUEUE_DEPTH}; got {queue_depth}"
        );
    }

    let (filled_tx, filled_rx) = sync_channel::<CaptureMessage>(queue_depth);
    let (recycle_tx, recycle_rx) = sync_channel::<Vec<i32>>(queue_depth);
    let worker = std::thread::Builder::new()
        .name("aurora-earc-capture".to_owned())
        .spawn(move || {
            let run = || -> Result<()> {
                let mut capture = NativeAlsaCapture::open(config)
                    .context("failed to open native direct-eARC capture")?;
                let geometry = capture.telemetry();
                let sample_count = geometry
                    .period_frames
                    .checked_mul(geometry.channels)
                    .filter(|value| *value > 0)
                    .context("negotiated capture period/channel product is invalid")?;

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
                                Err(_) => return Ok(()),
                            }
                        }
                    };

                    let block = capture
                        .read_owned_block(replacement)
                        .context("native direct-eARC capture failed")?;
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
                        return Ok(());
                    }
                }
            };

            if let Err(error) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
                let message = if let Some(text) = error.downcast_ref::<&str>() {
                    (*text).to_owned()
                } else if let Some(text) = error.downcast_ref::<String>() {
                    text.clone()
                } else {
                    "unknown capture-thread panic".to_owned()
                };
                let _ = filled_tx.send(CaptureMessage::Error(format!(
                    "native direct-eARC capture thread panicked: {message}"
                )));
            } else if let Err(error) = run() {
                let _ = filled_tx.send(CaptureMessage::Error(format!("{error:#}")));
            }
        })
        .context("failed spawning Aurora direct-eARC capture thread")?;

    Ok((filled_rx, recycle_tx, worker))
}

#[cfg(target_os = "linux")]
fn consume_batch_without_duplicate_reset<S: SpeakerSink>(
    batch: PlaybackBatch,
    sink: &mut S,
    stats: &mut RuntimeStats,
    transport_already_reset: bool,
) -> Result<()> {
    stats.record_batch(&batch);
    if batch.discontinuity && !transport_already_reset {
        sink.reset_for_transport_discontinuity()?;
    }
    for frame in batch.frames {
        sink.write_frame(&frame)?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn run_direct_native_alsa<S: SpeakerSink>(
    args: &Args,
    device: &str,
    runtime: &mut AuroraPlaybackRuntime,
    sink: &mut S,
    reporter: &HealthReporter,
) -> Result<RuntimeStats> {
    let capture_config = AlsaInputConfig {
        device: device.to_owned(),
        sample_rate: args.carrier_rate,
        channels: args.slots,
        period_frames: args.input_period_frames,
        buffer_frames: args.input_buffer_frames,
    };
    let (captured_rx, recycle_tx, capture_thread) =
        spawn_capture_thread(capture_config, args.input_queue_depth)?;

    let run_result = (|| -> Result<RuntimeStats> {
        let mut stats = RuntimeStats::default();
        let mut last_starvations = 0_u64;

        loop {
            let packet = match captured_rx.recv() {
                Ok(CaptureMessage::Block(packet)) => packet,
                Ok(CaptureMessage::Error(error)) => bail!("{error}"),
                Err(_) => break,
            };

            let capture_discontinuity = packet.block.discontinuity;
            if capture_discontinuity {
                runtime.reset();
                sink.reset_for_transport_discontinuity()?;
                stats.transport_discontinuities =
                    stats.transport_discontinuities.saturating_add(1);
            }

            let batch = runtime
                .push_direct_s32_words(&packet.block.interleaved_s32)
                .context("native threaded direct-eARC S32-word ingest failed")?;
            consume_batch_without_duplicate_reset(
                batch,
                sink,
                &mut stats,
                capture_discontinuity,
            )?;

            stats.capture_xruns = packet.counters.xruns;
            stats.capture_recoveries = packet.counters.recoveries;
            stats.capture_discontinuities = packet.counters.discontinuities;
            if packet.counters.queue_starvations != last_starvations {
                eprintln!(
                    "aurora-runtime-warning: native_capture_queue_starvations={} (consumer is exhausting the bounded capture pool)",
                    packet.counters.queue_starvations
                );
                last_starvations = packet.counters.queue_starvations;
            }

            recycle_tx
                .send(packet.block.interleaved_s32)
                .context("native direct-eARC capture worker stopped while recycling period buffer")?;

            let decoder = runtime.encoded().decoder();
            reporter.publish(stats.snapshot_with_joc(
                decoder.transport_telemetry(),
                sink.output_health(),
                decoder.engine().joc_health(),
            ));
        }
        Ok(stats)
    })();

    drop(captured_rx);
    drop(recycle_tx);
    let join_result = capture_thread.join();
    if join_result.is_err() && run_result.is_ok() {
        bail!("native direct-eARC capture thread terminated with an unreported panic");
    }
    run_result
}

#[cfg(not(target_os = "linux"))]
pub(super) fn run_direct_native_alsa<S: super::SpeakerSink>(
    _args: &super::Args,
    _device: &str,
    _runtime: &mut aurora_encoded_runtime::AuroraPlaybackRuntime,
    _sink: &mut S,
    _reporter: &aurora_encoded_runtime::health::HealthReporter,
) -> Result<super::RuntimeStats> {
    bail!("native ALSA direct-eARC capture requires Linux")
}
