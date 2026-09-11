use aurora_alsa_output::{encode_f32_to_s32_padded, f32_to_s32};
use aurora_core::ChannelRole;
use aurora_dsp_basic::output::{OutputDspConfig, SpeakerPostProcessor};
use aurora_dsp_basic::output_layout::{
    OutputChannelClass, OutputChannelSpec, OutputLayoutContract,
};

const CHANNELS: usize = 16;
const FRAMES: usize = 40;

fn sixteen_channel_layout() -> OutputLayoutContract {
    let channels = (0..CHANNELS)
        .map(|index| OutputChannelSpec {
            role: ChannelRole::Custom(format!("lane-{index}")),
            class: if index == 3 {
                OutputChannelClass::Lfe
            } else if index >= 12 {
                OutputChannelClass::Height
            } else {
                OutputChannelClass::Bed
            },
        })
        .collect();
    OutputLayoutContract::custom("16ch-validation", channels).unwrap()
}

#[test]
fn sixteen_channel_dsp_reaches_tdm16_without_lane_loss() {
    let layout = sixteen_channel_layout();
    let mut post = SpeakerPostProcessor::new_for_layout(OutputDspConfig::default(), layout).unwrap();
    let mut block = Vec::with_capacity(CHANNELS * FRAMES);
    for frame in 0..FRAMES {
        for channel in 0..CHANNELS {
            let phase = (frame * CHANNELS + channel) as f32 * 0.017;
            block.push(phase.sin() * 0.25);
        }
    }

    post.process_block(&mut block).unwrap();
    assert_eq!(block.len(), CHANNELS * FRAMES);
    assert!(block.iter().all(|sample| sample.is_finite()));

    let encoded = encode_f32_to_s32_padded(&block, CHANNELS, CHANNELS).unwrap();
    assert_eq!(encoded.len(), CHANNELS * FRAMES);
    for (source, encoded_sample) in block.iter().copied().zip(encoded.iter().copied()) {
        assert_eq!(encoded_sample, f32_to_s32(source).unwrap());
    }
}

#[test]
fn sixteen_channel_output_can_pad_into_wider_tdm_without_reordering() {
    let logical = (0..CHANNELS)
        .map(|index| (index + 1) as f32 / 32.0)
        .collect::<Vec<_>>();
    let encoded = encode_f32_to_s32_padded(&logical, CHANNELS, 20).unwrap();
    assert_eq!(encoded.len(), 20);
    for index in 0..CHANNELS {
        assert_eq!(encoded[index], f32_to_s32(logical[index]).unwrap());
    }
    assert_eq!(&encoded[CHANNELS..], &[0_i32; 4]);
}
