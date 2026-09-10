use aurora_core::{AudioBlock, AudioFormat, SampleType};
use aurora_decoder_api::DecodedFrame;
use aurora_dsp_basic::output::{OutputDspConfig, CHANNELS, SAMPLE_RATE};
use aurora_encoded_runtime::SpeakerOutputStage;

#[test]
fn speaker_output_accepts_short_joc_retirement_tail() {
    let format = AudioFormat {
        sample_rate: SAMPLE_RATE,
        channel_count: CHANNELS,
        sample_type: SampleType::F32,
        block_size: 40,
    };
    let mut output = SpeakerOutputStage::new(format, OutputDspConfig::default()).unwrap();

    // A 1536-sample E-AC-3/JOC access unit leaves a 16-frame remainder when
    // reblocked into Aurora's 40-frame realtime cadence. Renderer retirement
    // must preserve that real PCM instead of padding or dropping it.
    let tail_frames = 16;
    let frame = DecodedFrame {
        audio: AudioBlock {
            channels: (0..CHANNELS)
                .map(|channel| vec![(channel + 1) as f32 * 0.001; tail_frames])
                .collect(),
            frame_count: tail_frames,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        },
        objects: Vec::new(),
    };

    let processed = output.process_decoded_frame(frame).unwrap();
    assert_eq!(processed.frame_count, tail_frames);
    assert_eq!(processed.interleaved_f32.len(), tail_frames * CHANNELS);
    assert!(processed.interleaved_f32.iter().all(|sample| sample.is_finite()));
}
