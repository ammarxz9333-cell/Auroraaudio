use aurora_core::{AudioBlock, AudioFormat, SampleType};
use aurora_decoder_api::DecodedFrame;
use aurora_dsp_basic::output::{
    OutputDspConfig, CHANNELS as OUTPUT_CHANNELS, SAMPLE_RATE as OUTPUT_SAMPLE_RATE,
};
use aurora_encoded_runtime::{SpeakerOutputFrame, SpeakerOutputStage};

#[global_allocator]
static ALLOCATOR: aurora_test_alloc::CountingAllocator = aurora_test_alloc::CountingAllocator;

fn format() -> AudioFormat {
    AudioFormat {
        sample_rate: OUTPUT_SAMPLE_RATE,
        channel_count: OUTPUT_CHANNELS,
        sample_type: SampleType::F32,
        block_size: 40,
    }
}

fn frame() -> DecodedFrame {
    DecodedFrame {
        audio: AudioBlock {
            channels: vec![vec![0.0; 40]; OUTPUT_CHANNELS],
            frame_count: 40,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        },
        objects: Vec::new(),
    }
}

#[test]
fn speaker_output_processing_allocates_nothing_after_recycle_warmup() {
    let mut stage = SpeakerOutputStage::new(format(), OutputDspConfig::default()).unwrap();
    let source = frame();

    let warm = stage.process_decoded_frame_ref(&source).unwrap();
    stage.recycle_output_frame(warm);

    let mut produced: Option<SpeakerOutputFrame> = None;
    let allocations = aurora_test_alloc::count_allocations(|| {
        produced = Some(stage.process_decoded_frame_ref(&source).unwrap());
    });

    assert_eq!(allocations, 0);
    let produced = produced.expect("steady-state output frame was not produced");
    assert_eq!(produced.interleaved_f32.len(), 40 * OUTPUT_CHANNELS);
    stage.recycle_output_frame(produced);
}
