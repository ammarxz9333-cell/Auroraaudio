use std::hint::black_box;
use std::path::PathBuf;

use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};
use aurora_renderer_vbap::VbapRenderer;
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

criterion_group!(benches, phase_3a_renderer_comparison);
criterion_main!(benches);
