use std::{
    hint::black_box,
    sync::{Arc, Barrier},
    thread,
    time::Instant,
};

use aurora_realtime_engine::{
    correction_artifact_metrics, create_duplex_bridge_with_compensator, simulate_adaptive_drift,
    CorrectionTransition, DriftControllerConfig, DuplexBridgeConfig, DuplexFaultPolicy,
    ThresholdDriftCompensator, TransportKind, TransportPrototype,
};

const SAMPLE_RATE: f64 = 48_000.0;
const ITERATIONS: usize = 1_000;

fn main() {
    println!("Aurora duplex transport benchmark (48 kHz, times in microseconds)");
    println!("mode,transport,channels,frames,producer_p50,producer_p95,producer_max,consumer_p50,consumer_p95,consumer_max,throughput_msample_s,budget_p95_pct,minimum_atomics_per_frame,minimum_atomics_per_sample");
    for kind in [
        TransportKind::SampleArrayQueue,
        TransportKind::FixedBlockPool,
        TransportKind::ContiguousFrameRing,
    ] {
        for channels in [2, 6, 8, 12] {
            for frames in [64, 128, 256, 512] {
                sequential(kind, channels, frames);
                threaded(kind, channels, frames);
            }
        }
    }
    correction_artifacts();
    adaptive_drift_report();
}

fn sequential(kind: TransportKind, channels: usize, frames: usize) {
    let samples = channels * frames;
    let transport = TransportPrototype::new(kind, samples, 8).unwrap();
    let input = vec![0.25; samples];
    let mut output = vec![0.0; samples];
    let mut producer = Vec::with_capacity(ITERATIONS);
    let mut consumer = Vec::with_capacity(ITERATIONS);
    let started = Instant::now();
    for _ in 0..ITERATIONS {
        let operation = Instant::now();
        assert!(black_box(transport.try_push_block(black_box(&input))));
        producer.push(operation.elapsed().as_nanos() as u64);
        let operation = Instant::now();
        assert!(black_box(transport.try_pop_block(black_box(&mut output))));
        consumer.push(operation.elapsed().as_nanos() as u64);
    }
    print_result(
        "sequential",
        kind,
        channels,
        frames,
        producer,
        consumer,
        samples * ITERATIONS * 2,
        started.elapsed().as_nanos() as u64,
        transport.visible_atomics_per_round_trip(),
    );
}

fn threaded(kind: TransportKind, channels: usize, frames: usize) {
    let samples = channels * frames;
    let transport = TransportPrototype::new(kind, samples, 8).unwrap();
    let producer_transport = transport.clone();
    let consumer_transport = transport.clone();
    let barrier = Arc::new(Barrier::new(3));
    let producer_barrier = Arc::clone(&barrier);
    let consumer_barrier = Arc::clone(&barrier);
    let input = vec![0.25; samples];
    let producer = thread::spawn(move || {
        let mut timings = Vec::with_capacity(ITERATIONS);
        producer_barrier.wait();
        for _ in 0..ITERATIONS {
            let operation = Instant::now();
            while !producer_transport.try_push_block(black_box(&input)) {
                std::hint::spin_loop();
            }
            timings.push(operation.elapsed().as_nanos() as u64);
        }
        timings
    });
    let consumer = thread::spawn(move || {
        let mut timings = Vec::with_capacity(ITERATIONS);
        let mut output = vec![0.0; samples];
        consumer_barrier.wait();
        for _ in 0..ITERATIONS {
            let operation = Instant::now();
            while !consumer_transport.try_pop_block(black_box(&mut output)) {
                std::hint::spin_loop();
            }
            timings.push(operation.elapsed().as_nanos() as u64);
        }
        timings
    });
    barrier.wait();
    let started = Instant::now();
    let producer = producer.join().unwrap();
    let consumer = consumer.join().unwrap();
    print_result(
        "threaded-unpinned",
        kind,
        channels,
        frames,
        producer,
        consumer,
        samples * ITERATIONS * 2,
        started.elapsed().as_nanos() as u64,
        transport.visible_atomics_per_round_trip(),
    );
}

#[allow(clippy::too_many_arguments)]
fn print_result(
    mode: &str,
    kind: TransportKind,
    channels: usize,
    frames: usize,
    mut producer: Vec<u64>,
    mut consumer: Vec<u64>,
    transferred_samples: usize,
    elapsed_ns: u64,
    atomics: usize,
) {
    producer.sort_unstable();
    consumer.sort_unstable();
    let producer_p50 = percentile(&producer, 50);
    let producer_p95 = percentile(&producer, 95);
    let consumer_p50 = percentile(&consumer, 50);
    let consumer_p95 = percentile(&consumer, 95);
    let worst_p95 = producer_p95.max(consumer_p95) as f64;
    let block_budget_ns = frames as f64 / SAMPLE_RATE * 1_000_000_000.0;
    let throughput = transferred_samples as f64 / elapsed_ns as f64 * 1_000.0;
    println!(
        "{mode},{},{channels},{frames},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{throughput:.3},{:.4},{:.6},{:.6}",
        kind.label(),
        producer_p50 as f64 / 1_000.0,
        producer_p95 as f64 / 1_000.0,
        producer.last().copied().unwrap_or(0) as f64 / 1_000.0,
        consumer_p50 as f64 / 1_000.0,
        consumer_p95 as f64 / 1_000.0,
        consumer.last().copied().unwrap_or(0) as f64 / 1_000.0,
        worst_p95 / block_budget_ns * 100.0,
        atomics as f64 / frames as f64,
        atomics as f64 / (frames * channels) as f64,
    );
}

fn percentile(samples: &[u64], percentile: usize) -> u64 {
    samples[samples.len() * percentile / 100]
}

fn correction_artifacts() {
    println!("artifact,signal,transition,corrections,max_discontinuity,rms_error,spectral_artifact_energy");
    for signal in ["sine", "noise"] {
        for transition in [
            CorrectionTransition::Raw,
            CorrectionTransition::LinearInterpolation,
            CorrectionTransition::Crossfade,
            CorrectionTransition::ZeroCrossing,
        ] {
            let (reference, corrected, corrections) = correction_fixture(signal, transition);
            let metrics = correction_artifact_metrics(&reference, &corrected);
            println!(
                "artifact,{signal},{transition:?},{corrections},{:.9},{:.9},{:.9}",
                metrics.maximum_discontinuity, metrics.rms_error, metrics.spectral_artifact_energy,
            );
        }
    }
}

fn correction_fixture(signal: &str, transition: CorrectionTransition) -> (Vec<f32>, Vec<f32>, u64) {
    let channels = 2;
    let config = DuplexBridgeConfig {
        channels,
        capacity_frames: 512,
        target_fill_frames: 128,
        correction_threshold_frames: 8,
    };
    let (producer, mut consumer, status) = create_duplex_bridge_with_compensator(
        config,
        DuplexFaultPolicy {
            maximum_excursion_frames: 512,
            ..DuplexFaultPolicy::default()
        },
        Box::new(ThresholdDriftCompensator::new(transition, 16)),
    )
    .unwrap();
    let source = generated_signal(signal, 266);
    let mut initial = Vec::with_capacity(128 * channels);
    for sample in &source[..128] {
        initial.extend([*sample, -*sample]);
    }
    producer.push_interleaved(&initial, channels);
    let mut warmup = vec![0.0; 64 * channels];
    consumer.read_interleaved(&mut warmup, channels);
    let mut additional = Vec::with_capacity(138 * channels);
    for sample in &source[128..266] {
        additional.extend([*sample, -*sample]);
    }
    producer.push_interleaved(&additional, channels);
    let mut output = vec![0.0; 64 * channels];
    consumer.read_interleaved(&mut output, channels);

    let mut reference = Vec::with_capacity(65);
    reference.push(source[63]);
    reference.extend_from_slice(&source[64..128]);
    let mut corrected = Vec::with_capacity(65);
    corrected.push(warmup[(64 - 1) * channels]);
    corrected.extend(output.chunks_exact(channels).map(|frame| frame[0]));
    let snapshot = status.snapshot();
    (
        reference,
        corrected,
        snapshot.sample_slips_inserted + snapshot.sample_slips_removed,
    )
}

fn generated_signal(signal: &str, frames: usize) -> Vec<f32> {
    match signal {
        "sine" => (0..frames)
            .map(|frame| {
                (std::f32::consts::TAU * 750.0 * frame as f32 / SAMPLE_RATE as f32).sin() * 0.5
            })
            .collect(),
        _ => {
            let mut state = 0x1234_5678_u32;
            (0..frames)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.25
                })
                .collect()
        }
    }
}

fn adaptive_drift_report() {
    println!("adaptive_drift,ppm,duration_seconds,min_fill,max_fill,final_fill,final_correction_ppm,saturation,bounded");
    let config = DriftControllerConfig {
        target_fill_frames: 2_048,
        ..DriftControllerConfig::default()
    };
    for duration in [600, 3_600, 8 * 3_600] {
        for ppm in [
            -250.0, -100.0, -50.0, -25.0, -10.0, 10.0, 25.0, 50.0, 100.0, 250.0,
        ] {
            let report = simulate_adaptive_drift(ppm, duration, config, 8_192).unwrap();
            println!(
                "adaptive_drift,{ppm:.0},{duration},{:.3},{:.3},{:.3},{:.3},{},{}",
                report.minimum_fill_frames,
                report.maximum_fill_frames,
                report.final_fill_frames,
                report.final_correction_ppm,
                report.saturation_count,
                report.bounded,
            );
        }
    }
}
