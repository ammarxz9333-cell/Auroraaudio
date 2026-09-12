use aurora_core::{ChannelRole, Listener, Speaker, StandardLayout, Vector3};
use aurora_realtime_engine::{BasicRendererMode, RealTimeEngine, RealTimeEngineConfig, TestSignal};
use aurora_scene::{RenderScene, SceneObject, Trajectory};

pub fn standard_scene(layout: StandardLayout, block_size: usize) -> RenderScene {
    let roles = layout.canonical_roles();
    scene_with_roles(layout, roles, block_size)
}

pub fn channel_scene(channel_count: usize, block_size: usize) -> RenderScene {
    match channel_count {
        2 => standard_scene(StandardLayout::Stereo, block_size),
        6 => standard_scene(StandardLayout::FiveOne, block_size),
        8 => standard_scene(StandardLayout::SevenOne, block_size),
        _ => {
            let roles = (0..channel_count)
                .map(|index| ChannelRole::Custom(format!("custom-{index}")))
                .collect::<Vec<_>>();
            scene_with_roles(StandardLayout::Custom, &roles, block_size)
        }
    }
}

pub fn engine(
    channel_count: usize,
    block_size: usize,
    signal: TestSignal,
    apply_delay: bool,
) -> RealTimeEngine {
    let scene = channel_scene(channel_count, block_size);
    let config = RealTimeEngineConfig {
        sample_rate: 48_000,
        block_size,
        input_channels: 0,
        apply_geometric_delay: apply_delay,
        speed_of_sound: 343.0,
        test_signal: signal,
        renderer_mode: BasicRendererMode::InverseDistance,
    };
    let requirements = RealTimeEngine::delay_requirements(&scene, &config)
        .expect("benchmark delay requirements must be valid");
    let delay = aurora_dsp_basic::DelayProcessor::new(
        requirements.channel_count(),
        requirements.max_delay_samples(),
    );
    RealTimeEngine::new_with_prepared_delay_processor(scene, config, block_size, Box::new(delay))
        .expect("benchmark scene and engine configuration must be valid")
}

fn scene_with_roles(
    layout: StandardLayout,
    roles: &[ChannelRole],
    block_size: usize,
) -> RenderScene {
    let count = roles.len();
    let speakers = roles
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, channel_role)| {
            let angle = index as f32 / count as f32 * std::f32::consts::TAU;
            Speaker {
                id: format!("speaker-{index}"),
                label: format!("Speaker {index}"),
                channel_role,
                position: Vector3::new(angle.cos() * 2.0, angle.sin() * 2.0, 0.0),
                orientation: Vector3::ZERO,
                gain_db: 0.0,
                delay_samples: 0.0,
                enabled: true,
            }
        })
        .collect();
    RenderScene {
        layout,
        listener: Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 1.2,
        },
        speakers,
        object: SceneObject {
            id: "benchmark-source".to_owned(),
            gain_db: 0.0,
            spread: 0.0,
        },
        trajectory: Trajectory::Circle {
            center: Vector3::ZERO,
            radius: 1.0,
            z: 0.0,
            start_degrees: 0.0,
            revolutions_per_second: 0.25,
        },
        block_size,
    }
}
