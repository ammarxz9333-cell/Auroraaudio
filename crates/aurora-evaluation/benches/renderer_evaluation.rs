use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_evaluation::{evaluate_renderer, EvaluationConfig, EvaluationThresholds};
use aurora_renderer_api::Renderer;
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};
use aurora_scene::Trajectory;
use criterion::{criterion_group, criterion_main, Criterion};

fn benchmark(c: &mut Criterion) {
    let input = (0..48_000)
        .map(|frame| (frame as f32 * std::f32::consts::TAU * 440.0 / 48_000.0).sin() * 0.1)
        .collect::<Vec<_>>();
    c.bench_function("evaluation/geometric-binaural/48khz-256/1s", |b| {
        b.iter(|| {
            let mut renderer = renderer();
            evaluate_renderer(&mut renderer, &listener(), &trajectory(), &input, &config()).unwrap()
        })
    });
}

fn renderer() -> BasicRenderer {
    let mut renderer = BasicRenderer::new(BasicRendererMode::GeometricBinaural).with_smoothing(1.0);
    renderer.configure(speakers(), 48_000, 256, 1).unwrap();
    renderer
}

fn config() -> EvaluationConfig {
    EvaluationConfig {
        renderer_id: "geometric-binaural".to_owned(),
        scenario_id: "benchmark-circle".to_owned(),
        sample_rate: 48_000,
        block_size: 256,
        max_delay_samples: 1_024.0,
        apply_delays: true,
        thresholds: EvaluationThresholds {
            max_renderer_p99_ns: u64::MAX,
            ..EvaluationThresholds::default()
        },
        commit_sha: "benchmark".to_owned(),
        command: "cargo bench -p aurora-evaluation".to_owned(),
        probes: Vec::new(),
        hooks: Vec::new(),
        steady_state_allocations: None,
    }
}

fn listener() -> Listener {
    Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 1.2,
    }
}

fn speakers() -> Vec<Speaker> {
    [
        ("left", ChannelRole::FrontLeft, -0.0875),
        ("right", ChannelRole::FrontRight, 0.0875),
    ]
    .into_iter()
    .map(|(id, channel_role, x)| Speaker {
        id: id.to_owned(),
        label: id.to_owned(),
        channel_role,
        position: Vector3::new(x, 0.0, 1.2),
        orientation: Vector3::ZERO,
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    })
    .collect()
}

fn trajectory() -> Trajectory {
    Trajectory::Circle {
        center: Vector3::ZERO,
        radius: 2.0,
        z: 1.2,
        start_degrees: 0.0,
        revolutions_per_second: 0.25,
    }
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
