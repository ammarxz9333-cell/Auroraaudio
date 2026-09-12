use aurora_core::{ChannelRole, Listener, Speaker, StandardLayout, Vector3};
use aurora_dsp_api::{RealtimeDelayProcessor, RealtimeDspFault};
use aurora_realtime_engine::{
    BasicRendererMode, PreparedDspError, ProcessStatus, RealTimeEngine, RealTimeEngineConfig,
    RealTimeEngineError, TestSignal,
};
use aurora_scene::{RenderScene, SceneObject, Trajectory};
use aurora_test_alloc::{count_allocations, CountingAllocator};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Debug)]
struct ExternalDelay {
    channels: usize,
    max_delay_samples: f32,
    delays: Vec<f32>,
}

impl ExternalDelay {
    fn new(channels: usize, max_delay_samples: f32) -> Self {
        Self {
            channels,
            max_delay_samples,
            delays: vec![0.0; channels],
        }
    }
}

impl RealtimeDelayProcessor for ExternalDelay {
    fn channel_count(&self) -> usize {
        self.channels
    }

    fn max_delay_samples(&self) -> f32 {
        self.max_delay_samples
    }

    fn set_delays(&mut self, delays_samples: &[f32]) -> Result<(), RealtimeDspFault> {
        if delays_samples.len() != self.channels {
            return Err(RealtimeDspFault::DelayShape);
        }
        if delays_samples
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0 || *value > self.max_delay_samples)
        {
            return Err(RealtimeDspFault::DelayValue);
        }
        self.delays.copy_from_slice(delays_samples);
        Ok(())
    }

    fn process_planar(
        &mut self,
        input: &[Vec<f32>],
        output: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), RealtimeDspFault> {
        if input.len() != self.channels
            || output.len() != self.channels
            || input.iter().any(|channel| channel.len() < frame_count)
            || output.iter().any(|channel| channel.len() < frame_count)
        {
            return Err(RealtimeDspFault::BufferShape);
        }
        for (source, target) in input.iter().zip(output.iter_mut()) {
            target[..frame_count].copy_from_slice(&source[..frame_count]);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.delays.fill(0.0);
    }

    fn latency_frames(&self) -> usize {
        self.delays.iter().copied().fold(0.0_f32, f32::max).ceil() as usize
    }
}

#[test]
fn external_prepared_delay_processor_can_drive_the_engine_without_callback_allocations() {
    let mut engine = RealTimeEngine::new_with_prepared_delay_processor(
        scene(),
        config(false),
        64,
        Box::new(ExternalDelay::new(2, 16.0)),
    )
    .unwrap();
    let mut output = vec![0.0; 128];
    assert_eq!(
        engine.process_interleaved(None, &mut output),
        ProcessStatus::Ok
    );
    let capacity = output.capacity();
    let allocations = count_allocations(|| {
        assert_eq!(
            engine.process_interleaved(None, &mut output),
            ProcessStatus::Ok
        );
    });
    assert_eq!(allocations, 0);
    assert_eq!(output.capacity(), capacity);
    assert!(output.iter().all(|sample| sample.is_finite()));
}

#[test]
fn prepared_delay_channel_mismatch_fails_before_activation() {
    let result = RealTimeEngine::new_with_prepared_delay_processor(
        scene(),
        config(false),
        64,
        Box::new(ExternalDelay::new(1, 16.0)),
    );
    assert!(matches!(
        result,
        Err(RealTimeEngineError::PreparedDsp(
            PreparedDspError::ChannelCount {
                expected: 2,
                actual: 1
            }
        ))
    ));
}

#[test]
fn prepared_delay_capacity_mismatch_fails_before_activation() {
    let result = RealTimeEngine::new_with_prepared_delay_processor(
        scene(),
        config(true),
        64,
        Box::new(ExternalDelay::new(2, 8.0)),
    );
    assert!(matches!(
        result,
        Err(RealTimeEngineError::PreparedDsp(
            PreparedDspError::DelayCapacity { required, actual }
        )) if required >= 1_024.0 && actual == 8.0
    ));
}

fn config(apply_geometric_delay: bool) -> RealTimeEngineConfig {
    RealTimeEngineConfig {
        sample_rate: 48_000,
        block_size: 64,
        input_channels: 0,
        apply_geometric_delay,
        speed_of_sound: 343.0,
        test_signal: TestSignal::Sine,
        renderer_mode: BasicRendererMode::InverseDistance,
    }
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
        block_size: 64,
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
