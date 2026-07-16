use std::hint::black_box;
use std::path::PathBuf;

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};
use aurora_renderer_vbap::{HorizontalSpread, SpreadRenderObject, VbapRenderer};
use aurora_scene::{load_render_scene, RenderScene};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

fn fixture(name: &str) -> RenderScene {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scenes")
        .join(name);
    load_render_scene(path).expect("valid benchmark fixture")
}

fn bench_renderer<R: Renderer>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    implementation: &str,
    layout: &str,
    scene: &RenderScene,
    mut renderer: R,
) {
    renderer
        .configure(
            scene.ordered_speakers().expect("valid speaker layout"),
            48_000,
            256,
            1,
        )
        .expect("valid renderer configuration");
    let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
    let mut gains = vec![SpeakerGain::default(); renderer.output_channel_count()];
    let object = RenderObject {
        position: scene.trajectory.position_at_time(0.125),
        gain: 1.0,
    };

    group.throughput(Throughput::Elements(gains.len() as u64));
    group.bench_function(BenchmarkId::new(implementation, layout), |b| {
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

fn phase_3a_renderer_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase_3a_renderer_gains_48khz_256");
    for (layout, fixture_name) in [("5.1", "circle.json"), ("7.1", "circle_7_1.json")] {
        let scene = fixture(fixture_name);
        bench_renderer(&mut group, "vbap", layout, &scene, VbapRenderer::new());
        bench_renderer(
            &mut group,
            "basic-inverse-distance",
            layout,
            &scene,
            BasicRenderer::new(BasicRendererMode::InverseDistance),
        );
    }
    group.finish();
}

fn bench_spread(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    scenario: &str,
    layout: Vec<Speaker>,
    listener: Listener,
    position: Vector3,
    spread: HorizontalSpread,
) {
    let mut renderer = VbapRenderer::new();
    renderer
        .configure(layout, 48_000, 256, 1)
        .expect("valid renderer configuration");
    let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
    let mut gains = vec![SpeakerGain::default(); renderer.output_channel_count()];
    let object = SpreadRenderObject {
        object: RenderObject {
            position,
            gain: 1.0,
        },
        spread,
    };

    group.throughput(Throughput::Elements(gains.len() as u64));
    group.bench_function(scenario, |b| {
        b.iter(|| {
            renderer
                .render_spread_gains(
                    black_box(&listener),
                    black_box(std::slice::from_ref(&object)),
                    black_box(&mut gains),
                    black_box(&mut scratch),
                )
                .unwrap();
        });
    });
}

fn dense_layout(count: usize) -> Vec<Speaker> {
    (0..count)
        .map(|index| {
            let angle = std::f32::consts::TAU * index as f32 / count as f32;
            Speaker {
                id: format!("dense-{index:02}"),
                label: format!("Dense {index:02}"),
                channel_role: ChannelRole::Custom(format!("dense-{index:02}")),
                position: Vector3::new(angle.cos(), angle.sin(), 0.0),
                orientation: Vector3::ZERO,
                gain_db: 0.0,
                delay_samples: 0.0,
                enabled: true,
            }
        })
        .collect()
}

fn phase_3b_spread_benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase_3b_spread_gains_48khz_256");
    for (layout, fixture_name) in [("5.1", "circle.json"), ("7.1", "circle_7_1.json")] {
        let scene = fixture(fixture_name);
        let speakers = scene.ordered_speakers().expect("valid speaker layout");
        let position = scene.trajectory.position_at_time(0.125);
        bench_spread(
            &mut group,
            &format!("point/{layout}"),
            speakers.clone(),
            scene.listener,
            position,
            HorizontalSpread::POINT,
        );
        bench_spread(
            &mut group,
            &format!("intermediate/{layout}"),
            speakers,
            scene.listener,
            position,
            HorizontalSpread::new(0.5).unwrap(),
        );
    }

    let irregular = fixture("irregular_horizontal_10.json");
    bench_spread(
        &mut group,
        "maximum/irregular-10",
        irregular.ordered_speakers().unwrap(),
        irregular.listener,
        Vector3::new(-0.4, 0.9, 0.0),
        HorizontalSpread::MAXIMUM,
    );
    bench_spread(
        &mut group,
        "intermediate/dense-16",
        dense_layout(16),
        Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 1.2,
        },
        Vector3::new(0.4, -0.9, 0.0),
        HorizontalSpread::new(0.65).unwrap(),
    );
    group.finish();
}

criterion_group!(
    benches,
    phase_3a_renderer_comparison,
    phase_3b_spread_benchmarks
);
criterion_main!(benches);
