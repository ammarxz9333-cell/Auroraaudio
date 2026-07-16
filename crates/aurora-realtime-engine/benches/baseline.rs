mod common;

use std::time::Instant;

use aurora_realtime_engine::TestSignal;

const SAMPLE_RATE: f64 = 48_000.0;
const ITERATIONS: usize = 2_000;

fn main() {
    println!("Aurora real-time release baseline (median/p95 per block)");
    for frames in [64, 128, 256, 512] {
        for channels in [2, 6, 8, 12] {
            run_case(frames, channels);
        }
    }
}

fn run_case(frames: usize, channels: usize) {
    let mut engine = common::engine(channels, frames, TestSignal::RotatingSine, true);
    let mut output = vec![0.0; frames * channels];
    for _ in 0..100 {
        let _ = engine.process_interleaved(None, &mut output);
    }
    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let started = Instant::now();
        let _ = engine.process_interleaved(None, &mut output);
        samples.push(started.elapsed().as_nanos() as u64);
    }
    samples.sort_unstable();
    let median = samples[ITERATIONS / 2];
    let p95 = samples[ITERATIONS * 95 / 100];
    let budget_ns = frames as f64 / SAMPLE_RATE * 1_000_000_000.0;
    let median_percent = median as f64 / budget_ns * 100.0;
    let p95_percent = p95 as f64 / budget_ns * 100.0;
    let sustainable_channels = if p95 == 0 {
        channels
    } else {
        ((channels as f64 * budget_ns / p95 as f64).floor() as usize).max(channels)
    };
    println!(
        "frames={frames} channels={channels} median_us={:.3} p95_us={:.3} median_budget_pct={median_percent:.3} p95_budget_pct={p95_percent:.3} estimated_max_channels={sustainable_channels}",
        median as f64 / 1_000.0,
        p95 as f64 / 1_000.0,
    );
}
