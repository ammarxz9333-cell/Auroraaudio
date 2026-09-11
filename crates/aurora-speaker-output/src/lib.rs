//! Layout-driven decoded-speaker PCM -> Aurora output-DSP boundary.
//!
//! This crate deliberately knows nothing about HDMI, codecs or Atmos/JOC. A
//! caller supplies an explicit semantic output-layout contract and already
//! rendered planar speaker PCM. The stage validates that identity, interleaves
//! without reordering, applies Aurora's speaker DSP and exposes caller-owned
//! output buffers that can be recycled after a hardware sink consumes them.

#![forbid(unsafe_code)]

use aurora_core::{AudioFormat, SampleType};
use aurora_decoder_api::DecodedFrame;
use aurora_dsp_basic::output::{
    OutputDspConfig, OutputShapeError, SpeakerPostProcessor, SAMPLE_RATE as OUTPUT_SAMPLE_RATE,
};
use aurora_dsp_basic::output_layout::OutputLayoutContract;
use thiserror::Error;

const MAX_RECYCLED_BLOCKS: usize = 32;
const MAX_RECYCLED_FRAMES_PER_BLOCK: usize = 2_048;

#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerOutputFrame {
    pub interleaved_f32: Vec<f32>,
    pub frame_count: usize,
    pub channel_count: usize,
    pub presentation_time_seconds: f64,
    pub discontinuity: bool,
}

pub struct SpeakerOutputStage {
    layout: OutputLayoutContract,
    post: SpeakerPostProcessor,
    recycled_interleaved: Vec<Vec<f32>>,
    max_recycled_samples: usize,
}

impl SpeakerOutputStage {
    pub fn new(
        output_format: AudioFormat,
        layout: OutputLayoutContract,
        config: OutputDspConfig,
    ) -> Result<Self, SpeakerOutputError> {
        validate_output_format(output_format, &layout)?;
        let max_recycled_samples = layout
            .channel_count()
            .checked_mul(MAX_RECYCLED_FRAMES_PER_BLOCK)
            .ok_or(SpeakerOutputError::BlockSizeOverflow)?;
        let post = SpeakerPostProcessor::new_for_layout(config, layout.clone())
            .map_err(|error| SpeakerOutputError::Setup(error.to_string()))?;
        Ok(Self {
            layout,
            post,
            recycled_interleaved: Vec::with_capacity(MAX_RECYCLED_BLOCKS),
            max_recycled_samples,
        })
    }

    pub fn layout(&self) -> &OutputLayoutContract {
        &self.layout
    }

    pub fn channel_count(&self) -> usize {
        self.layout.channel_count()
    }

    fn validate_decoded_frame(&self, frame: &DecodedFrame) -> Result<(), SpeakerOutputError> {
        frame
            .audio
            .validate()
            .map_err(|error| SpeakerOutputError::InvalidAudio(error.to_string()))?;
        if !frame.objects.is_empty() {
            return Err(SpeakerOutputError::ObjectSignalBindingsRequired {
                object_count: frame.objects.len(),
            });
        }
        let actual = frame.audio.channels.len();
        let expected = self.channel_count();
        if actual != expected {
            return Err(SpeakerOutputError::ChannelCount { expected, actual });
        }
        Ok(())
    }

    fn take_interleaved_storage(&mut self, required: usize) -> Vec<f32> {
        if let Some(index) = self
            .recycled_interleaved
            .iter()
            .position(|buffer| buffer.capacity() >= required)
        {
            return self.recycled_interleaved.swap_remove(index);
        }
        Vec::with_capacity(required)
    }

    fn recycle_interleaved_storage(&mut self, mut storage: Vec<f32>) {
        storage.clear();
        if storage.capacity() > self.max_recycled_samples
            || self.recycled_interleaved.len() >= MAX_RECYCLED_BLOCKS
        {
            return;
        }
        self.recycled_interleaved.push(storage);
    }

    pub fn process_decoded_frame_ref(
        &mut self,
        frame: &DecodedFrame,
    ) -> Result<SpeakerOutputFrame, SpeakerOutputError> {
        self.validate_decoded_frame(frame)?;
        if frame.audio.discontinuity {
            self.post.reset();
        }

        let channel_count = self.channel_count();
        let required = frame
            .audio
            .frame_count
            .checked_mul(channel_count)
            .ok_or(SpeakerOutputError::BlockSizeOverflow)?;
        let mut interleaved = self.take_interleaved_storage(required);
        for frame_index in 0..frame.audio.frame_count {
            for channel in &frame.audio.channels {
                interleaved.push(channel[frame_index]);
            }
        }
        if let Err(error) = self.post.process_block(&mut interleaved) {
            self.recycle_interleaved_storage(interleaved);
            return Err(SpeakerOutputError::Dsp(error));
        }

        Ok(SpeakerOutputFrame {
            interleaved_f32: interleaved,
            frame_count: frame.audio.frame_count,
            channel_count,
            presentation_time_seconds: frame.audio.presentation_time_seconds,
            discontinuity: frame.audio.discontinuity,
        })
    }

    pub fn process_decoded_frame(
        &mut self,
        frame: DecodedFrame,
    ) -> Result<SpeakerOutputFrame, SpeakerOutputError> {
        self.process_decoded_frame_ref(&frame)
    }

    pub fn recycle_output_frame(&mut self, frame: SpeakerOutputFrame) {
        if frame.channel_count == self.channel_count() {
            self.recycle_interleaved_storage(frame.interleaved_f32);
        }
    }

    pub fn reset(&mut self) {
        self.post.reset();
    }
}

fn validate_output_format(
    format: AudioFormat,
    layout: &OutputLayoutContract,
) -> Result<(), SpeakerOutputError> {
    if format.sample_rate != OUTPUT_SAMPLE_RATE {
        return Err(SpeakerOutputError::SampleRate {
            expected: OUTPUT_SAMPLE_RATE,
            actual: format.sample_rate,
        });
    }
    if format.sample_type != SampleType::F32 {
        return Err(SpeakerOutputError::SampleType);
    }
    if format.channel_count != layout.channel_count() {
        return Err(SpeakerOutputError::ChannelCount {
            expected: layout.channel_count(),
            actual: format.channel_count,
        });
    }
    if format.block_size == 0 {
        return Err(SpeakerOutputError::ZeroBlockSize);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum SpeakerOutputError {
    #[error("invalid decoded audio block: {0}")]
    InvalidAudio(String),
    #[error("speaker output setup failed: {0}")]
    Setup(String),
    #[error("speaker output DSP failed: {0}")]
    Dsp(OutputShapeError),
    #[error("speaker output requires {expected} channels, got {actual}")]
    ChannelCount { expected: usize, actual: usize },
    #[error("speaker output requires {expected} Hz, got {actual} Hz")]
    SampleRate { expected: u32, actual: u32 },
    #[error("speaker output requires F32 decoded PCM")]
    SampleType,
    #[error("speaker output block size must be greater than zero")]
    ZeroBlockSize,
    #[error("speaker output block size overflow")]
    BlockSizeOverflow,
    #[error(
        "decoder returned {object_count} generic object metadata records without object-signal PCM bindings"
    )]
    ObjectSignalBindingsRequired { object_count: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::{AudioBlock, ChannelRole, StandardLayout};
    use aurora_decoder_api::DecodedFrame;
    use aurora_dsp_basic::output_layout::{OutputChannelClass, OutputChannelSpec};

    fn format(channels: usize) -> AudioFormat {
        AudioFormat {
            sample_rate: OUTPUT_SAMPLE_RATE,
            channel_count: channels,
            sample_type: SampleType::F32,
            block_size: 40,
        }
    }

    fn decoded_frame(channels: usize) -> DecodedFrame {
        let mut planar = Vec::with_capacity(channels);
        for channel in 0..channels {
            planar.push(vec![(channel + 1) as f32 / 64.0; 40]);
        }
        DecodedFrame {
            audio: AudioBlock {
                channels: planar,
                frame_count: 40,
                presentation_time_seconds: 1.0,
                discontinuity: true,
            },
            objects: Vec::new(),
        }
    }

    fn custom_sixteen() -> OutputLayoutContract {
        let channels = (0..16)
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
        OutputLayoutContract::custom("custom-16", channels).unwrap()
    }

    #[test]
    fn canonical_seven_one_four_remains_supported() {
        let layout = OutputLayoutContract::for_standard(StandardLayout::SevenOneFour).unwrap();
        let mut stage =
            SpeakerOutputStage::new(format(12), layout, OutputDspConfig::default()).unwrap();
        let output = stage.process_decoded_frame(decoded_frame(12)).unwrap();
        assert_eq!(output.channel_count, 12);
        assert_eq!(output.frame_count, 40);
        assert_eq!(output.interleaved_f32.len(), 480);
        assert!(output.interleaved_f32.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn custom_sixteen_channel_stage_processes_without_width_inference() {
        let mut stage = SpeakerOutputStage::new(
            format(16),
            custom_sixteen(),
            OutputDspConfig::default(),
        )
        .unwrap();
        let output = stage.process_decoded_frame(decoded_frame(16)).unwrap();
        assert_eq!(stage.layout().name(), "custom-16");
        assert_eq!(output.channel_count, 16);
        assert_eq!(output.interleaved_f32.len(), 640);
        assert!(output.interleaved_f32.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn stage_rejects_layout_width_mismatch() {
        let error = SpeakerOutputStage::new(
            format(12),
            custom_sixteen(),
            OutputDspConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            SpeakerOutputError::ChannelCount {
                expected: 16,
                actual: 12
            }
        ));
    }

    #[test]
    fn recycled_dynamic_output_storage_is_reused() {
        let mut stage = SpeakerOutputStage::new(
            format(16),
            custom_sixteen(),
            OutputDspConfig::default(),
        )
        .unwrap();
        let first = stage.process_decoded_frame(decoded_frame(16)).unwrap();
        let allocation = first.interleaved_f32.as_ptr();
        stage.recycle_output_frame(first);
        let second = stage.process_decoded_frame(decoded_frame(16)).unwrap();
        assert_eq!(second.interleaved_f32.as_ptr(), allocation);
    }
}
