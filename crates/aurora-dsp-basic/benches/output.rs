use aurora_core::StandardLayout;
use aurora_dsp_basic::output::{
    ChannelCalibration, OutputDspConfig, PeqBand, SpeakerCalibration, SpeakerPostProcessor,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark(c: &mut Criterion) {
    for bands in [0, 8] {
        let mut processor = SpeakerPostProcessor::new(OutputDspConfig::default()).unwrap();
        let config = SpeakerCalibration {
            schema_version: 1,
            sample_rate: 48000,
            channels: StandardLayout::SevenOneFour
                .canonical_roles()
                .iter()
                .map(|role| ChannelCalibration {
                    role: role.clone(),
                    trim_db: 0.0,
                    delay_frames: 48,
                    invert_polarity: false,
                    peq: vec![
                        PeqBand {
                            frequency_hz: 1000.0,
                            q: 0.7,
                            gain_db: -1.0
                        };
                        bands
                    ],
                })
                .collect(),
        };
        processor.configure_calibration(&config).unwrap();
        c.bench_function(&format!("output_714_40_frames_{bands}_peq_bands"), |b| {
            b.iter(|| {
                let mut audio = [0.1; 480];
                processor.process_block(black_box(&mut audio)).unwrap();
                black_box(audio);
            });
        });
    }
}
criterion_group!(benches, benchmark);
criterion_main!(benches);
