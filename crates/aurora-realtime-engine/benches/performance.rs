mod common;

use std::hint::black_box;

use aurora_core::{StandardLayout, Vector3};
use aurora_dsp_basic::DelayProcessor;
use aurora_realtime_engine::{
    AsynchronousResampler, RubatoAsrc, TestSignal, TransportKind, TransportPrototype,
};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn renderer_gains(c: &mut Criterion) {
    let mut group = c.benchmark_group("renderer_gains");
    for (name, layout) in [
        ("stereo", StandardLayout::Stereo),
        ("5.1", StandardLayout::FiveOne),
        ("7.1", StandardLayout::SevenOne),
        ("5.1.2", StandardLayout::FiveOneTwo),
    ] {
        let scene = common::standard_scene(layout, 256);
        bench_renderer(&mut group, name, scene);
    }
    bench_renderer(&mut group, "custom-12", common::channel_scene(12, 256));
    group.finish();
}

fn bench_renderer(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    scene: aurora_scene::RenderScene,
) {
    let speakers = scene.ordered_speakers().expect("valid benchmark scene");
    let mut renderer = BasicRenderer::new(BasicRendererMode::InverseDistance).with_smoothing(0.35);
    renderer
        .configure(speakers, 48_000, 256, 1)
        .expect("valid renderer configuration");
    let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
    let mut gains = vec![SpeakerGain::default(); renderer.output_channel_count()];
    let object = RenderObject {
        position: Vector3::new(0.5, 0.25, 0.0),
        gain: 1.0,
    };
    group.bench_function(name, |b| {
        b.iter(|| {
            renderer
                .render_gains(
                    black_box(&scene.listener),
                    black_box(std::slice::from_ref(&object)),
                    black_box(&mut gains),
                    black_box(&mut scratch),
                )
                .unwrap();
        });
    });
}

fn full_block_processing(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_block_48khz");
    for frames in [64, 128, 256, 512] {
        for channels in [2, 6, 8, 12] {
            let mut engine = common::engine(channels, frames, TestSignal::Sine, true);
            let mut output = vec![0.0; frames * channels];
            group.throughput(Throughput::Elements((frames * channels) as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("{channels}ch"), frames),
                &frames,
                |b, _| {
                    b.iter(|| {
                        black_box(engine.process_interleaved(None, black_box(&mut output)));
                    });
                },
            );
        }
    }
    group.finish();
}

fn geometric_delay(c: &mut Criterion) {
    let mut group = c.benchmark_group("geometric_fractional_delay");
    for channels in [2, 6, 8, 12] {
        let frames = 256;
        let input = vec![vec![0.1; frames]; channels];
        let mut output = vec![vec![0.0; frames]; channels];
        let mut delay = DelayProcessor::new(channels, 256.0);
        delay
            .set_delays((0..channels).map(|index| index as f32 * 1.25).collect())
            .unwrap();
        group.bench_function(BenchmarkId::from_parameter(channels), |b| {
            b.iter(|| {
                delay
                    .process_block_into(
                        black_box(&input),
                        black_box(&mut output),
                        black_box(frames),
                    )
                    .unwrap();
            });
        });
    }
    group.finish();
}

fn basic_dsp_kernel(c: &mut Criterion) {
    let mut samples = vec![0.1_f32; 256 * 8];
    let mut filter_state = [0.0_f32; 8];
    c.bench_function("basic_dsp_gain_delay_filter_kernel_8ch_256", |b| {
        b.iter(|| {
            for frame in black_box(&mut samples).chunks_exact_mut(8) {
                for (sample, state) in frame.iter_mut().zip(filter_state.iter_mut()) {
                    let gained = *sample * 0.707_945_76;
                    *state = state.mul_add(0.95, gained * 0.05);
                    *sample = *state;
                }
            }
        });
    });
}

fn rotating_source(c: &mut Criterion) {
    let mut engine = common::engine(8, 256, TestSignal::RotatingSine, false);
    let mut output = vec![0.0; 256 * 8];
    c.bench_function("rotating_source_update_8ch_256", |b| {
        b.iter(|| {
            black_box(engine.process_interleaved(None, black_box(&mut output)));
        });
    });
}

fn adaptive_asrc(c: &mut Criterion) {
    let mut group = c.benchmark_group("adaptive_asrc_48khz_256");
    for channels in [2, 6, 8, 12] {
        let mut resampler = RubatoAsrc::default();
        resampler.configure(48_000, 48_000, channels, 256).unwrap();
        let mut input = vec![0.1_f32; 512 * channels];
        let mut output = vec![0.0_f32; 256 * channels];
        group.throughput(Throughput::Elements((256 * channels) as u64));
        group.bench_function(BenchmarkId::from_parameter(channels), |b| {
            b.iter(|| {
                let required = resampler.required_input_frames() * channels;
                black_box(
                    resampler
                        .process(black_box(&input[..required]), black_box(&mut output))
                        .unwrap(),
                );
                input[0] = output[0];
            });
        });
    }
    group.finish();
}

fn deterministic_convolution(c: &mut Criterion) {
    const CHANNELS: usize = 12;
    const FRAMES: usize = 256;
    const TAPS: usize = 128;

    let mut impulse = vec![0.0_f32; TAPS];
    for (index, tap) in impulse.iter_mut().enumerate() {
        let decay = 1.0 - index as f32 / TAPS as f32;
        *tap = decay * ((index as f32 + 1.0) * 0.071).sin() * 0.02;
    }
    impulse[0] += 1.0;

    let input_frames = FRAMES + TAPS - 1;
    let mut input = vec![0.0_f32; CHANNELS * input_frames];
    for channel in 0..CHANNELS {
        for frame in 0..input_frames {
            input[channel * input_frames + frame] =
                ((frame + channel * 17) as f32 * 0.013).sin() * 0.25;
        }
    }
    let mut output = vec![0.0_f32; CHANNELS * FRAMES];

    c.bench_function("deterministic_convolution_12ch_256_128tap", |b| {
        b.iter(|| {
            for channel in 0..CHANNELS {
                let input_base = channel * input_frames;
                let output_base = channel * FRAMES;
                for frame in 0..FRAMES {
                    let mut sum = 0.0_f32;
                    for (tap_index, tap) in impulse.iter().enumerate() {
                        sum = input[input_base + frame + TAPS - 1 - tap_index].mul_add(*tap, sum);
                    }
                    output[output_base + frame] = sum;
                }
            }
            black_box(&output);
        });
    });
}

fn transport_round_trip(c: &mut Criterion) {
    const CHANNELS: usize = 12;
    const FRAMES: usize = 256;
    let samples = CHANNELS * FRAMES;
    let transport = TransportPrototype::new(TransportKind::ContiguousFrameRing, samples, 8)
        .expect("valid transport benchmark configuration");
    let input = vec![0.25_f32; samples];
    let mut output = vec![0.0_f32; samples];

    c.bench_function("transport_round_trip_contiguous_12ch_256", |b| {
        b.iter(|| {
            assert!(transport.try_push_block(black_box(&input)));
            assert!(transport.try_pop_block(black_box(&mut output)));
            black_box(&output);
        });
    });
}

criterion_group!(
    benches,
    renderer_gains,
    full_block_processing,
    geometric_delay,
    basic_dsp_kernel,
    rotating_source,
    adaptive_asrc,
    deterministic_convolution,
    transport_round_trip
);
criterion_main!(benches);
