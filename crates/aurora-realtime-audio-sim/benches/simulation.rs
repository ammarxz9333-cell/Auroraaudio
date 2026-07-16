use aurora_realtime_audio_sim::{builtin_profile, run_duplex_simulation, DuplexSimulationConfig};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn simulation_benchmark(criterion: &mut Criterion) {
    let config = DuplexSimulationConfig {
        profile: builtin_profile("usb-7-1").unwrap(),
        duration_seconds: 3_600,
        seed: 12345,
        input_ppm: None,
        output_ppm: None,
        callback_jitter_frames: None,
        device_latency_frames: None,
        sample_rate: None,
        block_size: 256,
        channels: None,
        faults: vec![],
    };
    criterion.bench_function("virtual_usb_7_1_one_hour", |bencher| {
        bencher.iter(|| black_box(run_duplex_simulation(black_box(&config)).unwrap()))
    });
}

criterion_group!(benches, simulation_benchmark);
criterion_main!(benches);
