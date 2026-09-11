//! Layout-driven encoded-source playback runtime.
//!
//! This is the additive migration path away from the canonical 7.1.4-only
//! `AuroraPlaybackRuntime`. It keeps Aurora's established encoded-input and
//! direct-eARC decoder boundaries, but the downstream speaker stage is selected
//! by an explicit [`OutputLayoutContract`] rather than channel-count inference.

#![forbid(unsafe_code)]

use aurora_core::{AudioFormat, StandardLayout};
use aurora_decoder_engine::EngineConfig;
use aurora_decoder_open::openjoc_native::{
    openjoc_preset_for_standard_layout, AURORA_ELEVEN_ONE_FOUR_REFERENCE_LAYOUT,
};
use aurora_dsp_basic::output::OutputDspConfig;
use aurora_dsp_basic::output_layout::{OutputLayoutContract, OutputLayoutContractError};
use aurora_encoded_input::{EncodedInputConfig, EncodedInputKind};
use aurora_encoded_runtime::{AuroraEncodedRuntime, RuntimeBatch, RuntimeError as EncodedRuntimeError};
use aurora_speaker_output::{SpeakerOutputError, SpeakerOutputFrame, SpeakerOutputStage};
use thiserror::Error;

#[derive(Debug, Default)]
pub struct LayoutPlaybackBatch {
    pub frames: Vec<SpeakerOutputFrame>,
    pub bursts: usize,
    pub format_changes: usize,
    pub discontinuity: bool,
    pub pts_48k: Option<u64>,
}

/// Encoded source -> decoder -> explicit-layout speaker DSP runtime.
pub struct LayoutPlaybackRuntime {
    encoded: AuroraEncodedRuntime,
    output: SpeakerOutputStage,
}

impl LayoutPlaybackRuntime {
    /// Internal constructor used only after a public typed constructor has bound
    /// the same layout identity at both the decoder and output-DSP boundaries.
    fn new(
        input_config: EncodedInputConfig,
        engine_config: EngineConfig,
        output_format: AudioFormat,
        output_layout: OutputLayoutContract,
        output_dsp: OutputDspConfig,
    ) -> Result<Self, LayoutPlaybackError> {
        let output = SpeakerOutputStage::new(output_format, output_layout, output_dsp)?;
        let encoded = AuroraEncodedRuntime::new(input_config, engine_config, output_format)?;
        Ok(Self { encoded, output })
    }

    /// Constructs the runtime for one typed fixed layout and binds the same
    /// identity into the OpenJOC adapter before any compressed input arrives.
    /// A conflicting legacy string hint is rejected rather than silently
    /// overwritten so the decoder and DSP boundaries cannot disagree.
    pub fn new_for_standard_layout(
        input_config: EncodedInputConfig,
        mut engine_config: EngineConfig,
        output_format: AudioFormat,
        layout: StandardLayout,
        output_dsp: OutputDspConfig,
    ) -> Result<Self, LayoutPlaybackError> {
        let expected = openjoc_preset_for_standard_layout(layout)
            .map_err(EncodedRuntimeError::Decoder)?;
        if let Some(existing) = engine_config.open_decoder.joc_layout_hint {
            if existing != expected {
                return Err(LayoutPlaybackError::DecoderLayoutConflict {
                    expected,
                    actual: existing,
                });
            }
        }

        let output_layout = OutputLayoutContract::for_standard(layout)?;
        engine_config.open_decoder = engine_config
            .open_decoder
            .with_standard_joc_layout(layout)
            .map_err(EncodedRuntimeError::Decoder)?;
        Self::new(
            input_config,
            engine_config,
            output_format,
            output_layout,
            output_dsp,
        )
    }

    /// Binds Aurora's explicit sixteen-lane 11.1.4 reference identity at both
    /// the decoder and speaker-DSP boundaries. No geometry is inferred from a
    /// bare channel count.
    pub fn new_for_aurora_eleven_one_four_reference(
        input_config: EncodedInputConfig,
        mut engine_config: EngineConfig,
        output_format: AudioFormat,
        output_dsp: OutputDspConfig,
    ) -> Result<Self, LayoutPlaybackError> {
        if let Some(existing) = engine_config.open_decoder.joc_layout_hint {
            if existing != AURORA_ELEVEN_ONE_FOUR_REFERENCE_LAYOUT {
                return Err(LayoutPlaybackError::DecoderLayoutConflict {
                    expected: AURORA_ELEVEN_ONE_FOUR_REFERENCE_LAYOUT,
                    actual: existing,
                });
            }
        }
        engine_config.open_decoder = engine_config
            .open_decoder
            .with_aurora_eleven_one_four_reference();
        let output_layout = OutputLayoutContract::aurora_eleven_one_four_reference()?;
        Self::new(
            input_config,
            engine_config,
            output_format,
            output_layout,
            output_dsp,
        )
    }

    pub const fn input_kind(&self) -> EncodedInputKind {
        self.encoded.input_kind()
    }

    pub fn output_layout(&self) -> &OutputLayoutContract {
        self.output.layout()
    }

    pub fn push_direct_s32(
        &mut self,
        bytes: &[u8],
    ) -> Result<LayoutPlaybackBatch, LayoutPlaybackError> {
        let batch = self.encoded.push_direct_s32(bytes)?;
        self.process_batch(batch)
    }

    pub fn push_direct_s32_words(
        &mut self,
        samples: &[i32],
    ) -> Result<LayoutPlaybackBatch, LayoutPlaybackError> {
        let batch = self.encoded.push_direct_s32_words(samples)?;
        self.process_batch(batch)
    }

    pub fn push_legacy_usb_packet(
        &mut self,
        packet: &[u8],
    ) -> Result<LayoutPlaybackBatch, LayoutPlaybackError> {
        let batch = self.encoded.push_legacy_usb_packet(packet)?;
        self.process_batch(batch)
    }

    fn recycle_decoded_frame(&mut self, frame: aurora_decoder_api::DecodedFrame) {
        self.encoded
            .decoder_mut()
            .engine_mut()
            .recycle_decoded_frame(frame);
    }

    fn recycle_decoded_frames(
        &mut self,
        frames: impl IntoIterator<Item = aurora_decoder_api::DecodedFrame>,
    ) {
        for frame in frames {
            self.recycle_decoded_frame(frame);
        }
    }

    fn process_batch(
        &mut self,
        batch: RuntimeBatch,
    ) -> Result<LayoutPlaybackBatch, LayoutPlaybackError> {
        if let Some(error) = batch
            .frames
            .iter()
            .find_map(|frame| self.output.validate_decoded_frame(frame).err())
        {
            self.recycle_decoded_frames(batch.frames);
            return Err(LayoutPlaybackError::Output(error));
        }

        if batch.discontinuity {
            self.output.reset();
        }

        let mut source_frames = batch.frames.into_iter();
        let mut frames = Vec::with_capacity(source_frames.len());
        while let Some(frame) = source_frames.next() {
            let processed = self.output.process_decoded_frame_ref(&frame);
            self.recycle_decoded_frame(frame);
            match processed {
                Ok(frame) => frames.push(frame),
                Err(error) => {
                    self.recycle_decoded_frames(source_frames);
                    for output in frames.drain(..) {
                        self.output.recycle_output_frame(output);
                    }
                    return Err(LayoutPlaybackError::Output(error));
                }
            }
        }

        Ok(LayoutPlaybackBatch {
            frames,
            bursts: batch.bursts,
            format_changes: batch.format_changes,
            discontinuity: batch.discontinuity,
            pts_48k: batch.pts_48k,
        })
    }

    pub fn recycle_output_frame(&mut self, frame: SpeakerOutputFrame) {
        self.output.recycle_output_frame(frame);
    }

    pub fn reset(&mut self) {
        self.encoded.reset();
        self.output.reset();
    }

    pub fn finish(&mut self) -> Result<LayoutPlaybackBatch, LayoutPlaybackError> {
        let batch = self.encoded.finish()?;
        self.process_batch(batch)
    }

    pub fn encoded(&self) -> &AuroraEncodedRuntime {
        &self.encoded
    }

    pub fn encoded_mut(&mut self) -> &mut AuroraEncodedRuntime {
        &mut self.encoded
    }
}

#[derive(Debug, Error)]
pub enum LayoutPlaybackError {
    #[error(transparent)]
    Encoded(#[from] EncodedRuntimeError),
    #[error(transparent)]
    Output(#[from] SpeakerOutputError),
    #[error(transparent)]
    Layout(#[from] OutputLayoutContractError),
    #[error("decoder JOC layout conflict: expected {expected}, got {actual}")]
    DecoderLayoutConflict {
        expected: &'static str,
        actual: &'static str,
    },
}
