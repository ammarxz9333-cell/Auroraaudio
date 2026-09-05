use aurora_core::StandardLayout;
use aurora_dsp_basic::output::*;
use aurora_test_alloc::{count_allocations, CountingAllocator};
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn calibration() -> SpeakerCalibration {
    SpeakerCalibration {
        schema_version: 1,
        sample_rate: SAMPLE_RATE,
        channels: StandardLayout::SevenOneFour
            .canonical_roles()
            .iter()
            .map(|role| ChannelCalibration {
                role: role.clone(),
                trim_db: 0.0,
                delay_frames: 0,
                invert_polarity: false,
                peq: vec![],
            })
            .collect(),
    }
}

#[test]
fn worst_case_calibrated_processing_and_controls_allocate_nothing() {
    let mut post = SpeakerPostProcessor::new(OutputDspConfig::default()).unwrap();
    let mut config = calibration();
    for channel in &mut config.channels {
        channel.delay_frames = 4800;
        channel.peq = vec![
            PeqBand {
                frequency_hz: 1000.0,
                q: 0.7,
                gain_db: -1.0
            };
            8
        ];
    }
    post.configure_calibration(&config).unwrap();
    let mut block = [0.05; CHANNELS * 40];
    let allocations = count_allocations(|| {
        for index in 0..2000 {
            post.set_master_gain_mdb(-3000);
            post.set_lipsync_frames(index % 24000);
            post.set_mute(index % 7 == 0);
            post.set_standby(index % 11 == 0);
            post.process_block(&mut block).unwrap();
        }
        post.reset();
    });
    assert_eq!(allocations, 0);
    assert!(block.iter().all(|sample| sample.is_finite()));
}

#[test]
fn malformed_configuration_is_rejected_without_replacing_active_state() {
    let mut post = SpeakerPostProcessor::new(OutputDspConfig::default()).unwrap();
    let mut config = calibration();
    config.channels.swap(4, 6);
    assert!(post.configure_calibration(&config).is_err());
    config = calibration();
    config.channels[0].peq.push(PeqBand {
        frequency_hz: f32::NAN,
        q: 1.0,
        gain_db: 0.0,
    });
    assert!(post.configure_calibration(&config).is_err());
    config = calibration();
    config.channels[0].delay_frames = usize::MAX;
    assert!(post.configure_calibration(&config).is_err());
    assert!(SpeakerPostProcessor::new(OutputDspConfig {
        headroom_db: f32::NAN,
        ..Default::default()
    })
    .is_err());
}

#[test]
fn mute_is_not_postponed_by_half_second_lipsync() {
    let mut post = SpeakerPostProcessor::new(OutputDspConfig {
        lipsync_frames: 24000,
        ..Default::default()
    })
    .unwrap();
    let mut block = [0.0; CHANNELS * 40];
    for offset in 0..1200 {
        for (index, frame) in block.chunks_exact_mut(CHANNELS).enumerate() {
            frame.fill(0.0);
            frame[0] = 0.1
                * (std::f32::consts::TAU * 1000.0 * (offset * 40 + index) as f32 / 48000.0).sin();
        }
        post.process_block(&mut block).unwrap();
    }
    assert!(block.iter().any(|v| v.abs() > 0.01));
    post.set_mute(true);
    for _ in 0..60 {
        block.fill(0.1);
        post.process_block(&mut block).unwrap();
    }
    assert!(
        block.iter().all(|v| v.abs() < 0.0001),
        "mute must settle within 50 ms despite buffered audio"
    );
}

#[test]
fn corrupt_samples_do_not_poison_subsequent_audio() {
    let mut post = SpeakerPostProcessor::new(OutputDspConfig::default()).unwrap();
    let mut bad = [f32::NAN; CHANNELS * 40];
    bad[0] = f32::INFINITY;
    post.process_block(&mut bad).unwrap();
    assert!(bad.iter().all(|v| v.is_finite()));
    let mut block = [0.0; CHANNELS * 40];
    let mut peak = 0.0_f32;
    for index in 0..100 {
        for (frame_index, frame) in block.chunks_exact_mut(CHANNELS).enumerate() {
            frame.fill(0.0);
            frame[0] = 0.1 * ((index * 40 + frame_index) as f32 * 0.13).sin();
        }
        post.process_block(&mut block).unwrap();
        peak = block.iter().fold(peak, |peak, v| peak.max(v.abs()));
    }
    assert!(peak > 0.01);
    let mut malformed = [1.0; 13];
    assert!(post.process_block(&mut malformed).is_err());
    assert_eq!(malformed, [0.0; 13]);
}

#[test]
fn calibration_polarity_applies_to_the_correct_speaker() {
    let mut reference = SpeakerPostProcessor::new(OutputDspConfig::default()).unwrap();
    let mut calibrated = SpeakerPostProcessor::new(OutputDspConfig::default()).unwrap();
    let mut config = calibration();
    config.channels[10].invert_polarity = true;
    calibrated.configure_calibration(&config).unwrap();
    for index in 0..1000 {
        let mut a = [0.0; CHANNELS];
        a[10] = 0.1 * (index as f32 * 0.2).sin();
        let mut b = a;
        reference.process_block(&mut a).unwrap();
        calibrated.process_block(&mut b).unwrap();
        assert!((a[10] + b[10]).abs() < 1e-6);
        assert!((a[3] - b[3]).abs() < 1e-6);
    }
}
