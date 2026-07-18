use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_evaluation::{
    evaluate_renderer, EvaluationConfig, EvaluationFixture, EvaluationThresholds, EvidenceStatus,
    HookCategory, HookEvidence, MAX_FRAMES,
};
use aurora_renderer_api::{
    RenderObject, Renderer, RendererError, RendererScratch, RendererScratchSize, SpeakerGain,
};
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};
use aurora_scene::Trajectory;

fn listener() -> Listener {
    Listener {
        position: Vector3::ZERO,
        orientation: Vector3::new(0.0, 1.0, 0.0),
        ear_height: 1.2,
    }
}

fn speakers() -> Vec<Speaker> {
    vec![
        Speaker {
            id: "left".to_owned(),
            label: "Left".to_owned(),
            channel_role: ChannelRole::FrontLeft,
            position: Vector3::new(-0.0875, 0.0, 1.2),
            orientation: Vector3::ZERO,
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        },
        Speaker {
            id: "right".to_owned(),
            label: "Right".to_owned(),
            channel_role: ChannelRole::FrontRight,
            position: Vector3::new(0.0875, 0.0, 1.2),
            orientation: Vector3::ZERO,
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        },
    ]
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

fn input() -> Vec<f32> {
    (0..4_096)
        .map(|frame| (frame as f32 * std::f32::consts::TAU * 440.0 / 48_000.0).sin() * 0.1)
        .collect()
}

fn config() -> EvaluationConfig {
    let fixture: EvaluationFixture = serde_json::from_str(include_str!(
        "../../../fixtures/evaluation/canonical_renderer_cases.json"
    ))
    .unwrap();
    EvaluationConfig {
        renderer_id: "geometric-binaural".to_owned(),
        scenario_id: "canonical-renderer-directions".to_owned(),
        sample_rate: 48_000,
        block_size: 256,
        max_delay_samples: 1_024.0,
        apply_delays: true,
        thresholds: EvaluationThresholds {
            max_renderer_p99_ns: u64::MAX,
            ..EvaluationThresholds::default()
        },
        commit_sha: "0123456789abcdef".to_owned(),
        command: "aurora evaluate-renderer --renderer geometric-binaural".to_owned(),
        probes: fixture.probes,
        hooks: Vec::new(),
        steady_state_allocations: Some(0),
    }
}

fn renderer() -> BasicRenderer {
    let mut renderer = BasicRenderer::new(BasicRendererMode::GeometricBinaural).with_smoothing(1.0);
    renderer.configure(speakers(), 48_000, 256, 1).unwrap();
    renderer
}

#[test]
fn deterministic_evidence_and_audio_repeat_exactly() {
    let input = input();
    let first = evaluate_renderer(
        &mut renderer(),
        &listener(),
        &trajectory(),
        &input,
        &config(),
    )
    .unwrap();
    let second = evaluate_renderer(
        &mut renderer(),
        &listener(),
        &trajectory(),
        &input,
        &config(),
    )
    .unwrap();

    assert_eq!(first.audio_channels, second.audio_channels);
    assert_eq!(first.report.renderer, second.report.renderer);
    assert_eq!(first.report.audio, second.report.audio);
    assert_eq!(first.report.latency, second.report.latency);
    assert_eq!(first.report.memory, second.report.memory);
    assert_eq!(first.report.discontinuity, second.report.discontinuity);
    assert_eq!(first.report.probes, second.report.probes);
    assert_eq!(first.report.trajectory, second.report.trajectory);
    assert_eq!(first.report.validation.status, EvidenceStatus::Pass);
}

#[test]
fn canonical_probes_and_circular_motion_are_finite() {
    let bundle = evaluate_renderer(
        &mut renderer(),
        &listener(),
        &trajectory(),
        &input(),
        &config(),
    )
    .unwrap();

    assert_eq!(
        bundle
            .report
            .probes
            .iter()
            .map(|probe| probe.id.as_str())
            .collect::<Vec<_>>(),
        ["front", "right", "rear", "left", "overhead"]
    );
    assert!(bundle
        .report
        .probes
        .iter()
        .flat_map(|probe| probe.gains.iter().chain(&probe.delays_samples))
        .all(|value| value.is_finite()));
    assert!(!bundle.report.audio.contains_non_finite);
}

#[test]
fn threshold_and_transport_hook_failures_fail_the_run() {
    let mut config = config();
    config.thresholds.max_gain_discontinuity = 0.0;
    config.hooks.push(HookEvidence {
        id: "transport-ring-ordering".to_owned(),
        category: HookCategory::Transport,
        status: EvidenceStatus::Fail,
        truth_source: "unit_test".to_owned(),
    });

    let bundle = evaluate_renderer(
        &mut renderer(),
        &listener(),
        &trajectory(),
        &input(),
        &config,
    )
    .unwrap();

    assert_eq!(bundle.report.validation.status, EvidenceStatus::Fail);
    assert!(bundle.report.validation.findings.iter().any(|finding| {
        finding.id == "hook:transport-ring-ordering" && finding.status == EvidenceStatus::Fail
    }));
}

#[test]
fn missing_allocation_observation_is_explicit_not_fabricated() {
    let mut config = config();
    config.steady_state_allocations = None;
    let bundle = evaluate_renderer(
        &mut renderer(),
        &listener(),
        &trajectory(),
        &input(),
        &config,
    )
    .unwrap();

    assert_eq!(
        bundle.report.allocations.status,
        EvidenceStatus::NotObserved
    );
    assert_eq!(bundle.report.allocations.steady_state_allocations, None);
}

#[test]
fn capacity_accounting_is_bounded_and_machine_readable() {
    let bundle = evaluate_renderer(
        &mut renderer(),
        &listener(),
        &trajectory(),
        &input(),
        &config(),
    )
    .unwrap();
    let json = bundle.report.to_json_pretty().unwrap();

    assert!(bundle.report.memory.bounded_working_set_bytes < 1_000_000);
    assert!(json.contains("\"truth_source\": \"deterministic_capacity_accounting\""));
    assert!(json.contains("\"renderer_p99_ns\""));
}

#[test]
fn frame_limit_is_rejected_without_truncation() {
    let oversized = vec![0.0; MAX_FRAMES + 1];
    let error = evaluate_renderer(
        &mut renderer(),
        &listener(),
        &trajectory(),
        &oversized,
        &config(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("frames count"));
}

#[test]
fn non_finite_probe_position_is_rejected_before_rendering() {
    let mut config = config();
    config.probes[0].position.x = f32::INFINITY;

    let error = evaluate_renderer(
        &mut renderer(),
        &listener(),
        &trajectory(),
        &input(),
        &config,
    )
    .unwrap_err();

    assert!(error.to_string().contains("finite coordinates"));
}

#[derive(Default)]
struct NonFiniteRenderer;

impl Renderer for NonFiniteRenderer {
    fn configure(
        &mut self,
        _layout: Vec<Speaker>,
        _sample_rate: u32,
        _block_size: usize,
        _max_objects: usize,
    ) -> Result<(), RendererError> {
        Ok(())
    }

    fn required_scratch_size(&self) -> Result<RendererScratchSize, RendererError> {
        Ok(RendererScratchSize { float_count: 0 })
    }

    fn render_gains(
        &mut self,
        _listener: &Listener,
        _objects: &[RenderObject],
        output_gains: &mut [SpeakerGain],
        _scratch: &mut RendererScratch,
    ) -> Result<(), RendererError> {
        output_gains[0] = SpeakerGain {
            speaker_index: 0,
            gain: f32::NAN,
            distance_meters: 1.0,
            delay_samples: 0.0,
        };
        Ok(())
    }

    fn reset(&mut self) {}

    fn latency_frames(&self) -> usize {
        0
    }

    fn output_channel_count(&self) -> usize {
        1
    }
}

#[test]
fn non_finite_renderer_output_fails_validation() {
    let mut config = config();
    config.renderer_id = "non-finite-test".to_owned();
    config.apply_delays = false;
    let bundle = evaluate_renderer(
        &mut NonFiniteRenderer,
        &listener(),
        &trajectory(),
        &input(),
        &config,
    )
    .unwrap();

    assert!(bundle.report.audio.contains_non_finite);
    assert_eq!(bundle.report.validation.status, EvidenceStatus::Fail);
}
