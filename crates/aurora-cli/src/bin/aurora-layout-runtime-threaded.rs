//! Bounded native ALSA capture worker for `aurora-layout-runtime`.
//!
//! The producer owns the ALSA PCM handle and moves complete S32_LE period
//! buffers through a bounded channel. The consumer recycles those allocations
//! after the explicit-layout decoder/DSP path consumes them. This keeps live
//! wider-layout capture off the playback/output thread without inventing a new
//! transport contract.

#[cfg(target_os = "linux")]
use std::collections::VecDeque;
#[cfg(target_os = "linux")]
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};

#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::{bail, Result};
#[cfg(target_os = "linux")]
use aurora_alsa_input::{AlsaInputConfig, NativeAlsaCapture, OwnedCaptureBlock};
#[cfg(target_os = "linux")]
use aurora_layout_playback_runtime::{LayoutPlaybackBatch, LayoutPlaybackRuntime};

#[cfg(target_os = "linux")]
use super::{Args, SpeakerSink, WordHalfArg};

#[cfg(target_os = "linux")]
const MIN_QUEUE_DEPTH: usize = 2;
#[cfg(target_os = "linux")]
const MAX_QUEUE_DEPTH: usize = 256;
#[cfg(target_os = "linux")]
const REFERENCE_CARRIER_RATE_HZ: u32 = 192_000;
#[cfg(target_os = "linux")]
const REFERENCE_CARRIER_SLOTS: usize = 2;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct NativeCaptureStats {
    pub xruns: u64,
    pub recoveries: u64,
    pub discontinuities: u64,
    pub queue_starvations: u64,
    pub transport_discontinuities: u64,
}

#[cfg(target_os = "linux")]
fn validate_reference_capture_geometry(
    sample_rate: u32,
    slots: usize,
    word_half: WordHalfArg,
) -> Result<()> {
    if sample_rate != REFERENCE_CARRIER_RATE_HZ {
        bail!(
            "native explicit-layout direct-eARC capture requires the proven {REFERENCE_CARRIER_RATE_HZ} Hz carrier rate; got {sample_rate} Hz"
        );
    }
    if slots != REFERENCE_CARRIER_SLOTS {
        bail!(
            "native explicit-layout direct-eARC capture requires the proven {REFERENCE_CARRIER_SLOTS}-slot carrier; got {slots} slots"
        );
    }
    if word_half != WordHalfArg::High {
        bail!(
            "native explicit-layout direct-eARC capture requires proven high-half S32 packing (IEC61937 word in bits 31..16)"
        );
    }
    Ok(())
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
    if !(MIN_QUEUE_DEPTH..=MAX_QUEUE_DEPTH).contains(&queue_depth) {
        bail!(
            "native capture queue depth must be between {MIN_QUEUE_DEPTH} and {MAX_QUEUE_DEPTH}; got {queue_depth}"
        );
    }

    let (filled_tx, filled_rx) = sync_channel::<CaptureMessage>(queue_depth);
    let (recycle_tx, recycle_rx) = sync_channel::<Vec<i32>>(queue_depth);
    let worker = std::thread::Builder::new()
        .name("aurora-layout-earc-capture".to_owned())
        .spawn(move || {
            let run = || -> Result<()> {
                let mut capture = NativeAlsaCapture::open(config)
                    .context("failed to open explicit-layout native direct-eARC capture")?;
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
                        .context("explicit-layout native direct-eARC capture failed")?;
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

            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    let _ = filled_tx.send(CaptureMessage::Error(format!("{error:#}")));
                }
                Err(error) => {
                    let message = if let Some(text) = error.downcast_ref::<&str>() {
                        (*text).to_owned()
                    } else if let Some(text) = error.downcast_ref::<String>() {
                        text.clone()
                    } else {
                        "unknown capture-thread panic".to_owned()
                    };
                    let _ = filled_tx.send(CaptureMessage::Error(format!(
                        "explicit-layout direct-eARC capture thread panicked: {message}"
                    )));
                }
            }
        })
        .context("failed spawning explicit-layout direct-eARC capture thread")?;

    Ok((filled_rx, recycle_tx, worker))
}

#[cfg(target_os = "linux")]
fn consume_batch_without_duplicate_reset<S: SpeakerSink>(
    mut batch: LayoutPlaybackBatch,
    runtime: &mut LayoutPlaybackRuntime,
    sink: &mut S,
    transport_already_reset: bool,
) -> Result<()> {
    if batch.discontinuity && !transport_already_reset {
        sink.reset_for_transport_discontinuity()?;
    }
    for frame in batch.frames.drain(..) {
        let write_result = sink.write_frame(&frame.interleaved_f32, frame.discontinuity);
        runtime.recycle_output_frame(frame);
        write_result?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn run_direct_native_alsa<S: SpeakerSink>(
    args: &Args,
    device: &str,
    runtime: &mut LayoutPlaybackRuntime,
    sink: &mut S,
) -> Result<NativeCaptureStats> {
    validate_reference_capture_geometry(args.carrier_rate, args.slots, args.word_half)?;
    let capture_config = AlsaInputConfig {
        device: device.to_owned(),
        sample_rate: args.carrier_rate,
        channels: args.slots,
        period_frames: args.input_period_frames,
        buffer_frames: args.input_buffer_frames,
    };
    let (captured_rx, recycle_tx, capture_thread) =
        spawn_capture_thread(capture_config, args.input_queue_depth)?;

    let run_result = (|| -> Result<NativeCaptureStats> {
        let mut stats = NativeCaptureStats::default();
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
                stats.transport_discontinuities = stats.transport_discontinuities.saturating_add(1);
            }

            let batch = runtime
                .push_direct_s32_words(&packet.block.interleaved_s32)
                .context("native explicit-layout direct-eARC S32-word ingest failed")?;
            consume_batch_without_duplicate_reset(batch, runtime, sink, capture_discontinuity)?;

            stats.xruns = packet.counters.xruns;
            stats.recoveries = packet.counters.recoveries;
            stats.discontinuities = packet.counters.discontinuities;
            stats.queue_starvations = packet.counters.queue_starvations;

            recycle_tx
                .send(packet.block.interleaved_s32)
                .context("explicit-layout capture worker stopped while recycling period buffer")?;
        }
        Ok(stats)
    })();

    drop(captured_rx);
    drop(recycle_tx);
    match run_result {
        Ok(stats) => {
            if capture_thread.join().is_err() {
                bail!("explicit-layout capture thread terminated with an unreported panic");
            }
            Ok(stats)
        }
        Err(error) => {
            // A producer may still be blocked in an ALSA read. Detach it on a
            // downstream fail-closed error so shutdown cannot turn into a hang.
            drop(capture_thread);
            Err(error)
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn run_direct_native_alsa<S: super::SpeakerSink>(
    _args: &super::Args,
    _device: &str,
    _runtime: &mut aurora_layout_playback_runtime::LayoutPlaybackRuntime,
    _sink: &mut S,
) -> Result<NativeCaptureStats> {
    bail!("native ALSA direct-eARC capture requires Linux")
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn native_layout_capture_accepts_only_reference_geometry() {
        validate_reference_capture_geometry(
            REFERENCE_CARRIER_RATE_HZ,
            REFERENCE_CARRIER_SLOTS,
            WordHalfArg::High,
        )
        .unwrap();

        for rate in [0, 48_000, 96_000, 384_000] {
            assert!(validate_reference_capture_geometry(
                rate,
                REFERENCE_CARRIER_SLOTS,
                WordHalfArg::High
            )
            .is_err());
        }
        for slots in [0, 1, 4, 8, 16] {
            assert!(validate_reference_capture_geometry(
                REFERENCE_CARRIER_RATE_HZ,
                slots,
                WordHalfArg::High
            )
            .is_err());
        }
        assert!(validate_reference_capture_geometry(
            REFERENCE_CARRIER_RATE_HZ,
            REFERENCE_CARRIER_SLOTS,
            WordHalfArg::Low
        )
        .is_err());
    }
}
