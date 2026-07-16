use std::path::PathBuf;

use aurora_core::Vector3;
use aurora_renderer_api::{RenderObject, Renderer, RendererScratch, SpeakerGain};
use aurora_renderer_vbap::VbapRenderer;
use aurora_scene::{load_render_scene, RenderScene};

fn fixture(name: &str) -> RenderScene {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scenes")
        .join(name);
    load_render_scene(path).unwrap()
}

fn configured_renderer(scene: &RenderScene) -> (VbapRenderer, RendererScratch, Vec<SpeakerGain>) {
    let speakers = scene.ordered_speakers().unwrap();
    let mut renderer = VbapRenderer::new();
    renderer
        .configure(speakers, 48_000, scene.block_size, 1)
        .unwrap();
    let scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
    let gains = vec![SpeakerGain::default(); renderer.output_channel_count()];
    (renderer, scratch, gains)
}

fn render(
    renderer: &mut VbapRenderer,
    scratch: &mut RendererScratch,
    gains: &mut [SpeakerGain],
    scene: &RenderScene,
    position: Vector3,
) {
    renderer
        .render_gains(
            &scene.listener,
            &[RenderObject {
                position,
                gain: 1.0,
            }],
            gains,
            scratch,
        )
        .unwrap();
}

#[test]
fn standard_fixtures_keep_canonical_output_indices_and_finite_values() {
    for name in ["circle.json", "circle_7_1.json"] {
        let scene = fixture(name);
        let (mut renderer, mut scratch, mut gains) = configured_renderer(&scene);
        render(
            &mut renderer,
            &mut scratch,
            &mut gains,
            &scene,
            scene.trajectory.position_at_time(0.125),
        );

        assert_eq!(gains.len(), scene.layout.canonical_roles().len());
        assert!(gains
            .iter()
            .enumerate()
            .all(|(index, gain)| gain.speaker_index == index));
        assert!(gains.iter().all(|gain| {
            gain.gain.is_finite()
                && gain.distance_meters.is_finite()
                && gain.delay_samples.is_finite()
        }));
    }
}

#[test]
fn scene_vector_order_does_not_change_vbap_output() {
    let scene = fixture("circle_7_1.json");
    let mut reversed = scene.clone();
    reversed.speakers.reverse();

    let (mut first_renderer, mut first_scratch, mut first_gains) = configured_renderer(&scene);
    let (mut second_renderer, mut second_scratch, mut second_gains) =
        configured_renderer(&reversed);
    let position = scene.trajectory.position_at_time(0.375);
    render(
        &mut first_renderer,
        &mut first_scratch,
        &mut first_gains,
        &scene,
        position,
    );
    render(
        &mut second_renderer,
        &mut second_scratch,
        &mut second_gains,
        &reversed,
        position,
    );

    assert_eq!(first_gains, second_gains);
}

#[test]
fn full_circle_is_deterministic_finite_and_continuous() {
    fn render_circle() -> (Vec<Vec<f32>>, f32) {
        let scene = fixture("circle_7_1.json");
        let (mut renderer, mut scratch, mut gains) = configured_renderer(&scene);
        let mut snapshots = Vec::with_capacity(1_441);
        let mut maximum_step = 0.0_f32;
        let mut previous = None::<Vec<f32>>;

        for step in 0..=1_440 {
            let time = step as f64 / 1_440.0 / 0.5;
            render(
                &mut renderer,
                &mut scratch,
                &mut gains,
                &scene,
                scene.trajectory.position_at_time(time),
            );
            let snapshot = gains.iter().map(|gain| gain.gain).collect::<Vec<_>>();
            assert!(snapshot.iter().all(|gain| gain.is_finite()));
            if let Some(previous) = &previous {
                let step_size = previous
                    .iter()
                    .zip(&snapshot)
                    .map(|(left, right)| (left - right).powi(2))
                    .sum::<f32>()
                    .sqrt();
                maximum_step = maximum_step.max(step_size);
            }
            previous = Some(snapshot.clone());
            snapshots.push(snapshot);
        }

        (snapshots, maximum_step)
    }

    let first = render_circle();
    let second = render_circle();
    assert_eq!(first.0, second.0);
    assert_eq!(first.1, second.1);
    assert!(first.1 < 0.02, "maximum gain-vector step was {}", first.1);
}
