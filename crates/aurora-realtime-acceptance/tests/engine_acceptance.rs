use aurora_core::{ChannelRole, Listener, Speaker, StandardLayout, Vector3};
use aurora_realtime_acceptance::{
    evaluate_realtime_acceptance, RealTimeAcceptancePolicy, RealTimeAcceptanceViolation,
};
use aurora_realtime_engine::{
    BasicRendererMode, ProcessStatus, RealTimeEngine, RealTimeEngineConfig, RealTimeFault, TestSignal,
};
use aurora_scene::{RenderScene, SceneObject, Trajectory};

#[test]
fn clean_engine_run_passes_strict_health_policy() {
    let mut engine = engine();
    let mut output = vec![0.0_f32; 1_024 * 2];
    for _ in 0..64 {
        assert_eq!(
            engine.process_interleaved(None, &mut output),
            ProcessStatus::Ok
        );
    }

    let report = evaluate_realtime_acceptance(
        engine.metrics(),
        RealTimeAcceptancePolicy {
            max_estimated_end_to_end_latency_frames: Some(4_096),
            ..RealTimeAcceptancePolicy::default()
        },
    );

    assert!(report.accepted, "violations: {:?}", report.violations);
    assert_eq!(engine.metrics().callback_count, 64);
    assert_eq!(engine.metrics().processed_blocks, 64);
    assert_eq!(engine.metrics().input_underruns, 0);
    assert_eq!(engine.metrics().output_underruns, 0);
    assert_eq!(engine.metrics().dropped_blocks, 0);
    assert_eq!(engine.metrics().fault, RealTimeFault::None);
}

#[test]
fn malformed_engine_callback_is_rejected_by_strict_health_policy() {
    let mut engine = engine();
    let mut malformed_output = vec![1.0_f32; 1_023];
    assert_eq!(
        engine.process_interleaved(None, &mut malformed_output),
        ProcessStatus::Fault(RealTimeFault::OutputBuffer)
    );

    let report =
        evaluate_realtime_acceptance(engine.metrics(), RealTimeAcceptancePolicy::default());
    assert!(!report.accepted);
    assert!(report
        .violations
        .contains(&RealTimeAcceptanceViolation::Fault(
            RealTimeFault::OutputBuffer
        )));
    assert!(report
        .violations
        .contains(&RealTimeAcceptanceViolation::OutputUnderruns {
            actual: 1,
            maximum: 0,
        }));
    assert!(report
        .violations
        .contains(&RealTimeAcceptanceViolation::DroppedBlocks {
            actual: 1,
            maximum: 0,
        }));
}

fn engine() -> RealTimeEngine {
    RealTimeEngine::new(
        scene(),
        RealTimeEngineConfig {
            sample_rate: 48_000,
            block_size: 1_024,
            input_channels: 0,
            apply_geometric_delay: false,
            speed_of_sound: 343.0,
            test_signal: TestSignal::Silence,
            renderer_mode: BasicRendererMode::InverseDistance,
        },
        128,
    )
    .expect("test realtime engine must configure")
}

fn scene() -> RenderScene {
    RenderScene {
        layout: StandardLayout::Stereo,
        listener: Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 1.2,
        },
        speakers: vec![
            speaker("left", ChannelRole::FrontLeft, -1.0),
            speaker("right", ChannelRole::FrontRight, 1.0),
        ],
        object: SceneObject {
            id: "source".to_owned(),
            gain_db: 0.0,
            spread: 0.0,
        },
        trajectory: Trajectory::Circle {
            center: Vector3::ZERO,
            radius: 0.5,
            z: 0.0,
            start_degrees: 0.0,
            revolutions_per_second: 0.25,
        },
        block_size: 1_024,
    }
}

fn speaker(id: &str, role: ChannelRole, x: f32) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: id.to_owned(),
        channel_role: role,
        position: Vector3::new(x, 0.0, 0.0),
        orientation: Vector3::ZERO,
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}
