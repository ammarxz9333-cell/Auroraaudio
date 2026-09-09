//! Single-process Aurora encoded-input, decode, render-boundary and output-DSP runtime.
//!
//! Direct eARC and the legacy STM32/USB fallback converge before IEC61937
//! parsing and decoding. Decoder frames that are already speaker-rendered may
//! continue through Aurora's canonical 48 kHz / 7.1.4 output DSP in the same
//! runtime. Generic object metadata is never guessed into PCM lanes: object-
//! preserving frames must use Aurora's `SpatialDecodedFrame` contract and the
//! existing 3D VBAP spatial runtime.
//!
//! OpenJOC is intentionally not rendered twice. Its admitted speaker-mode path
//! already returns speaker-layout PCM with no generic `DecodedFrame.objects`;
//! that PCM enters only the downstream output DSP stage.

#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;

use aurora_core::{AudioFormat, CoreError, SampleType, StandardLayout};
use aurora_decoder_api::{DecodedFrame, DecoderError};
use aurora_decoder_engine::{spatial_ir::SpatialDecodedFrame, EngineConfig};
use aurora_direct_earc_decoder::DirectEarcDecoder;
use aurora_dsp_basic::output::{
    OutputDspConfig, OutputShapeError, SpeakerPostProcessor, CHANNELS as OUTPUT_CHANNELS,
    SAMPLE_RATE as OUTPUT_SAMPLE_RATE,
};
use aurora_encoded_input::{
    EncodedInput, EncodedInputConfig, EncodedInputError, EncodedInputKind,
};
use aurora_spatial_runtime::{SpatialRuntimeConfig, SpatialRuntimeError, VbapSpatialRuntime};

/// Decoder output produced by one source-ingest call.
#[derive(Debug, Default)]
pub struct RuntimeBatch {
    pub frames: Vec<DecodedFrame>,
    pub bursts: usize,
    pub format_changes: usize,
    pub discontinuity: bool,
    pub pts_48k: Option<u64>,
}

/// Final speaker-domain block after Aurora's canonical output DSP.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerOutputFrame {
    /// Interleaved F32 samples in Aurora canonical 7.1.4 channel order.
    pub interleaved_f32: Vec<f32>,
    pub frame_count: usize,
    pub presentation_time_seconds: f64,
    pub discontinuity: bool,
}

/// Output produced by one encoded-source ingest call after decode and DSP.
#[derive(Debug, Default)]
pub struct PlaybackBatch {
    pub frames: Vec<SpeakerOutputFrame>,
    pub bursts: usize,
    pub format_changes: usize,
    pub discontinuity: bool,
    pub pts_48k: Option<u64>,
}

/// One runtime instance is bound to exactly one input family until recreated.
/// This fail-closed rule prevents an accidental automatic switch between the
/// direct eARC receiver and the legacy STM32/USB fallback.
pub struct AuroraEncodedRuntime {
    input: EncodedInput,
    decoder: DirectEarcDecoder,
}

impl AuroraEncodedRuntime {
    pub fn new(
        input_config: EncodedInputConfig,
        engine_config: EngineConfig,
        output_format: AudioFormat,
    ) -> Result<Self, RuntimeError> {
        let input = EncodedInput::new(input_config).map_err(RuntimeError::Input)?;
        let mut decoder = DirectEarcDecoder::new(engine_config);
        decoder
            .configure(output_format)
            .map_err(RuntimeError::Decoder)?;
        Ok(Self { input, decoder })
    }

    pub const fn input_kind(&self) -> EncodedInputKind {
        self.input.kind()
    }

    /// Feeds raw S32_LE serial-audio capture bytes from the direct eARC path.
    pub fn push_direct_s32(&mut self, bytes: &[u8]) -> Result<RuntimeBatch, RuntimeError> {
        let Some(carrier) = self
            .input
            .push_direct_s32(bytes)
            .map_err(RuntimeError::Input)?
        else {
            return Ok(RuntimeBatch::default());
        };
        self.decode_carrier(carrier.carrier, carrier.discontinuity, carrier.pts_48k)
    }

    /// Feeds complete native ALSA S32 slot samples from direct eARC capture.
    ///
    /// This path avoids the redundant intermediate i32 -> byte staging vector
    /// while preserving the exact signed 32-bit slot bit pattern.
    pub fn push_direct_s32_words(
        &mut self,
        samples: &[i32],
    ) -> Result<RuntimeBatch, RuntimeError> {
        let Some(carrier) = self
            .input
            .push_direct_s32_words(samples)
            .map_err(RuntimeError::Input)?
        else {
            return Ok(RuntimeBatch::default());
        };
        self.decode_carrier(carrier.carrier, carrier.discontinuity, carrier.pts_48k)
    }

    /// Feeds one complete Aurora USB v1 packet from the legacy STM32 bridge.
    /// Control/clock/output packets intentionally produce an empty batch.
    pub fn push_legacy_usb_packet(
        &mut self,
        packet: &[u8],
    ) -> Result<RuntimeBatch, RuntimeError> {
        let Some(carrier) = self
            .input
            .push_legacy_usb_packet(packet)
            .map_err(RuntimeError::Input)?
        else {
            return Ok(RuntimeBatch::default());
        };
        self.decode_carrier(carrier.carrier, carrier.discontinuity, carrier.pts_48k)
    }

    fn decode_carrier(
        &mut self,
        carrier: Vec<u8>,
        discontinuity: bool,
        pts_48k: Option<u64>,
    ) -> Result<RuntimeBatch, RuntimeError> {
        let decoded = self
            .decoder
            .push_carrier(&carrier, discontinuity)
            .map_err(RuntimeError::Decoder)?;
        Ok(RuntimeBatch {
            frames: decoded.frames,
            bursts: decoded.bursts,
            format_changes: decoded.format_changes,
            discontinuity: decoded.discontinuity,
            pts_48k,
        })
    }

    /// Clears both source-local partial state and decoder/parser state after an
    /// out-of-band physical reset.
    pub fn reset(&mut self) {
        self.input.reset();
        self.decoder.reset();
    }

    /// Checks source-local end-of-stream invariants.
    pub fn finish(&self) -> Result<(), RuntimeError> {
        self.input.finish().map_err(RuntimeError::Input)
    }

    pub fn decoder(&self) -> &DirectEarcDecoder {
        &self.decoder
    }

    pub fn decoder_mut(&mut self) -> &mut DirectEarcDecoder {
        &mut self.decoder
    }
}

/// Canonical speaker-output stage shared by already-rendered decoder PCM and
/// Aurora's object-preserving spatial renderer.
pub struct SpeakerOutputStage {
    post: SpeakerPostProcessor,
}

impl SpeakerOutputStage {
    pub fn new(output_format: AudioFormat, config: OutputDspConfig) -> Result<Self, RuntimeError> {
        validate_output_format(output_format)?;
        let post = SpeakerPostProcessor::new(config)
            .map_err(|error| RuntimeError::OutputSetup(error.to_string()))?;
        Ok(Self { post })
    }

    fn validate_decoded_frame(&self, frame: &DecodedFrame) -> Result<(), RuntimeError> {
        frame.audio.validate().map_err(RuntimeError::InvalidAudioBlock)?;
        if !frame.objects.is_empty() {
            return Err(RuntimeError::ObjectSignalBindingsRequired {
                object_count: frame.objects.len(),
            });
        }
        if frame.audio.channels.len() != OUTPUT_CHANNELS {
            return Err(RuntimeError::OutputChannelCount {
                expected: OUTPUT_CHANNELS,
                actual: frame.audio.channels.len(),
            });
        }
        Ok(())
    }

    /// Converts planar canonical speaker PCM to the hardware-facing interleaved
    /// domain and applies bass management, calibration, limiter and lip-sync.
    pub fn process_decoded_frame(
        &mut self,
        frame: DecodedFrame,
    ) -> Result<SpeakerOutputFrame, RuntimeError> {
        self.validate_decoded_frame(&frame)?;
        if frame.audio.discontinuity {
            self.post.reset();
        }

        let mut interleaved = Vec::with_capacity(frame.audio.frame_count * OUTPUT_CHANNELS);
        for frame_index in 0..frame.audio.frame_count {
            for channel in &frame.audio.channels {
                interleaved.push(channel[frame_index]);
            }
        }
        self.post
            .process_block(&mut interleaved)
            .map_err(RuntimeError::OutputDsp)?;

        Ok(SpeakerOutputFrame {
            interleaved_f32: interleaved,
            frame_count: frame.audio.frame_count,
            presentation_time_seconds: frame.audio.presentation_time_seconds,
            discontinuity: frame.audio.discontinuity,
        })
    }

    pub fn reset(&mut self) {
        self.post.reset();
    }
}

/// End-to-end encoded-source runtime through Aurora's canonical speaker DSP.
///
/// Both direct eARC and legacy USB use this exact object. Frames with generic
/// object metadata fail closed because that metadata lacks object-signal PCM
/// bindings. Valid speaker-rendered output, including OpenJOC speaker-mode PCM,
/// is not spatially rendered a second time.
pub struct AuroraPlaybackRuntime {
    encoded: AuroraEncodedRuntime,
    output: SpeakerOutputStage,
}

impl AuroraPlaybackRuntime {
    pub fn new(
        input_config: EncodedInputConfig,
        engine_config: EngineConfig,
        output_format: AudioFormat,
        output_dsp: OutputDspConfig,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            encoded: AuroraEncodedRuntime::new(input_config, engine_config, output_format)?,
            output: SpeakerOutputStage::new(output_format, output_dsp)?,
        })
    }

    pub const fn input_kind(&self) -> EncodedInputKind {
        self.encoded.input_kind()
    }

    pub fn push_direct_s32(&mut self, bytes: &[u8]) -> Result<PlaybackBatch, RuntimeError> {
        let batch = self.encoded.push_direct_s32(bytes)?;
        self.process_batch(batch)
    }

    /// Feeds native ALSA S32 slot words without a byte-staging allocation.
    pub fn push_direct_s32_words(
        &mut self,
        samples: &[i32],
    ) -> Result<PlaybackBatch, RuntimeError> {
        let batch = self.encoded.push_direct_s32_words(samples)?;
        self.process_batch(batch)
    }

    pub fn push_legacy_usb_packet(
        &mut self,
        packet: &[u8],
    ) -> Result<PlaybackBatch, RuntimeError> {
        let batch = self.encoded.push_legacy_usb_packet(packet)?;
        self.process_batch(batch)
    }

    fn process_batch(&mut self, batch: RuntimeBatch) -> Result<PlaybackBatch, RuntimeError> {
        // Validate the whole batch before mutating DSP state, so an unsupported
        // generic object-bearing frame cannot partially advance the output chain.
        for frame in &batch.frames {
            self.output.validate_decoded_frame(frame)?;
        }
        if batch.discontinuity {
            self.output.reset();
        }

        let mut frames = Vec::with_capacity(batch.frames.len());
        for frame in batch.frames {
            frames.push(self.output.process_decoded_frame(frame)?);
        }
        Ok(PlaybackBatch {
            frames,
            bursts: batch.bursts,
            format_changes: batch.format_changes,
            discontinuity: batch.discontinuity,
            pts_48k: batch.pts_48k,
        })
    }

    pub fn reset(&mut self) {
        self.encoded.reset();
        self.output.reset();
    }

    pub fn finish(&self) -> Result<(), RuntimeError> {
        self.encoded.finish()
    }

    pub fn encoded(&self) -> &AuroraEncodedRuntime {
        &self.encoded
    }

    pub fn encoded_mut(&mut self) -> &mut AuroraEncodedRuntime {
        &mut self.encoded
    }
}

/// Explicit object-preserving renderer + output-DSP route.
///
/// This is deliberately separate from generic `DecodedFrame.objects`: a
/// `SpatialDecodedFrame` carries the PCM-lane bindings required to render real
/// object signals. It is the reusable target for decoder backends that expose
/// Aurora Spatial IR rather than already-rendered speaker PCM.
pub struct SpatialSpeakerRuntime {
    spatial: VbapSpatialRuntime,
    output: SpeakerOutputStage,
}

impl SpatialSpeakerRuntime {
    pub fn new(
        config: SpatialRuntimeConfig,
        output_dsp: OutputDspConfig,
    ) -> Result<Self, RuntimeError> {
        validate_spatial_output_layout(&config)?;
        let output_format = AudioFormat {
            sample_rate: config.sample_rate,
            channel_count: OUTPUT_CHANNELS,
            sample_type: SampleType::F32,
            block_size: config.renderer_block_size,
        };
        let spatial = VbapSpatialRuntime::new(config).map_err(RuntimeError::Spatial)?;
        let output = SpeakerOutputStage::new(output_format, output_dsp)?;
        Ok(Self { spatial, output })
    }

    pub fn render_frame(
        &mut self,
        frame: &SpatialDecodedFrame,
    ) -> Result<SpeakerOutputFrame, RuntimeError> {
        let audio = self
            .spatial
            .render_frame(frame)
            .map_err(RuntimeError::Spatial)?;
        self.output.process_decoded_frame(DecodedFrame {
            audio,
            objects: Vec::new(),
        })
    }

    pub fn reset(&mut self) {
        self.spatial.reset();
        self.output.reset();
    }
}

fn validate_output_format(format: AudioFormat) -> Result<(), RuntimeError> {
    if format.sample_rate != OUTPUT_SAMPLE_RATE {
        return Err(RuntimeError::OutputSampleRate {
            expected: OUTPUT_SAMPLE_RATE,
            actual: format.sample_rate,
        });
    }
    if format.channel_count != OUTPUT_CHANNELS {
        return Err(RuntimeError::OutputChannelCount {
            expected: OUTPUT_CHANNELS,
            actual: format.channel_count,
        });
    }
    Ok(())
}

fn validate_spatial_output_layout(config: &SpatialRuntimeConfig) -> Result<(), RuntimeError> {
    let enabled = config
        .speakers
        .iter()
        .filter(|speaker| speaker.enabled)
        .collect::<Vec<_>>();
    let canonical = StandardLayout::SevenOneFour.canonical_roles();
    if enabled.len() != canonical.len()
        || enabled
            .iter()
            .zip(canonical.iter())
            .any(|(speaker, role)| speaker.channel_role != *role)
    {
        return Err(RuntimeError::OutputLayout(
            "spatial output must expose enabled speakers in Aurora canonical 7.1.4 order"
                .to_owned(),
        ));
    }
    if config.sample_rate != OUTPUT_SAMPLE_RATE {
        return Err(RuntimeError::OutputSampleRate {
            expected: OUTPUT_SAMPLE_RATE,
            actual: config.sample_rate,
        });
    }
    Ok(())
}

#[derive(Debug)]
pub enum RuntimeError {
    Input(EncodedInputError),
    Decoder(DecoderError),
    InvalidAudioBlock(CoreError),
    OutputSetup(String),
    OutputDsp(OutputShapeError),
    Spatial(SpatialRuntimeError),
    OutputSampleRate { expected: u32, actual: u32 },
    OutputChannelCount { expected: usize, actual: usize },
    OutputLayout(String),
    ObjectSignalBindingsRequired { object_count: usize },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input(error) => write!(f, "encoded input failed: {error}"),
            Self::Decoder(error) => write!(f, "decoder failed: {error}"),
            Self::InvalidAudioBlock(error) => write!(f, "invalid decoded audio block: {error}"),
            Self::OutputSetup(error) => write!(f, "speaker output DSP setup failed: {error}"),
            Self::OutputDsp(error) => write!(f, "speaker output DSP failed: {error}"),
            Self::Spatial(error) => write!(f, "spatial renderer failed: {error}"),
            Self::OutputSampleRate { expected, actual } => write!(
                f,
                "speaker output requires {expected} Hz, got {actual} Hz"
            ),
            Self::OutputChannelCount { expected, actual } => write!(
                f,
                "speaker output requires {expected} canonical channels, got {actual}"
            ),
            Self::OutputLayout(error) => write!(f, "speaker output layout is invalid: {error}"),
            Self::ObjectSignalBindingsRequired { object_count } => write!(
                f,
                "decoder returned {object_count} generic object metadata records without object-signal PCM bindings; SpatialDecodedFrame is required and Aurora refuses to guess or drop them"
            ),
        }
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Decoder(error) => Some(error),
            Self::InvalidAudioBlock(error) => Some(error),
            Self::OutputDsp(error) => Some(error),
            Self::Spatial(error) => Some(error),
            Self::OutputSetup(_)
            | Self::OutputSampleRate { .. }
            | Self::OutputChannelCount { .. }
            | Self::OutputLayout(_)
            | Self::ObjectSignalBindingsRequired { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::{AudioBlock, AudioObject, SampleType, Vector3};
    use aurora_encoded_input::EncodedInputConfig;
    use aurora_iec61937::CarrierWordHalf;
    use aurora_usb_protocol::{Header, Kind, FLAG_DISCONTINUITY};

    fn format() -> AudioFormat {
        AudioFormat {
            sample_rate: OUTPUT_SAMPLE_RATE,
            channel_count: OUTPUT_CHANNELS,
            sample_type: SampleType::F32,
            block_size: 40,
        }
    }

    fn usb_packet(kind: Kind, flags: u32, payload: &[u8]) -> Vec<u8> {
        let mut header = Header::new(kind, payload.len() as u32);
        header.flags = flags;
        let mut packet = header.encode().to_vec();
        packet.extend_from_slice(payload);
        packet
    }

    fn decoded_frame(channels: usize, objects: Vec<AudioObject>) -> DecodedFrame {
        DecodedFrame {
            audio: AudioBlock {
                channels: vec![vec![0.0; 40]; channels],
                frame_count: 40,
                presentation_time_seconds: 1.25,
                discontinuity: true,
            },
            objects,
        }
    }

    #[test]
    fn direct_runtime_buffers_partial_serial_audio_without_fabricating_audio() {
        let mut runtime = AuroraEncodedRuntime::new(
            EncodedInputConfig::DirectEarc {
                slots: 2,
                word_half: CarrierWordHalf::High,
            },
            EngineConfig::default(),
            format(),
        )
        .unwrap();

        let batch = runtime.push_direct_s32(&[1, 2, 3]).unwrap();
        assert_eq!(runtime.input_kind(), EncodedInputKind::DirectEarc);
        assert_eq!(batch.bursts, 0);
        assert!(batch.frames.is_empty());
        runtime.reset();
        runtime.finish().unwrap();
    }

    #[test]
    fn native_s32_word_path_enters_same_transport_parser() {
        let mut runtime = AuroraEncodedRuntime::new(
            EncodedInputConfig::DirectEarc {
                slots: 2,
                word_half: CarrierWordHalf::High,
            },
            EngineConfig::default(),
            format(),
        )
        .unwrap();
        let samples = [
            (u32::from(0xF872_u16) << 16) as i32,
            (u32::from(0x4E1F_u16) << 16) as i32,
        ];

        let batch = runtime.push_direct_s32_words(&samples).unwrap();

        // Preamble-only input is not a complete IEC61937 burst, but it proves
        // the native-word path reaches the same parser without fabricating PCM.
        assert_eq!(batch.bursts, 0);
        assert!(batch.frames.is_empty());
        assert_eq!(runtime.decoder().pending_carrier_bytes(), 4);
    }

    #[test]
    fn legacy_control_packets_do_not_enter_decoder() {
        let mut runtime = AuroraEncodedRuntime::new(
            EncodedInputConfig::LegacyUsb,
            EngineConfig::default(),
            format(),
        )
        .unwrap();
        let packet = usb_packet(Kind::ClockReport, 0, &[0_u8; 24]);
        let batch = runtime.push_legacy_usb_packet(&packet).unwrap();
        assert_eq!(runtime.input_kind(), EncodedInputKind::LegacyUsb);
        assert_eq!(batch.bursts, 0);
        assert!(batch.frames.is_empty());
    }

    #[test]
    fn legacy_discontinuity_reaches_decoder_boundary_without_fake_output() {
        let mut runtime = AuroraEncodedRuntime::new(
            EncodedInputConfig::LegacyUsb,
            EngineConfig::default(),
            format(),
        )
        .unwrap();
        let packet = usb_packet(
            Kind::EncodedIec61937,
            FLAG_DISCONTINUITY,
            &[0x72, 0xF8],
        );
        let batch = runtime.push_legacy_usb_packet(&packet).unwrap();
        assert!(batch.discontinuity);
        assert_eq!(batch.bursts, 0);
        assert!(batch.frames.is_empty());
    }

    #[test]
    fn wrong_source_calls_fail_closed() {
        let mut runtime = AuroraEncodedRuntime::new(
            EncodedInputConfig::LegacyUsb,
            EngineConfig::default(),
            format(),
        )
        .unwrap();
        assert!(matches!(
            runtime.push_direct_s32(&[0_u8; 8]),
            Err(RuntimeError::Input(EncodedInputError::WrongSource { .. }))
        ));
        assert!(matches!(
            runtime.push_direct_s32_words(&[0_i32; 2]),
            Err(RuntimeError::Input(EncodedInputError::WrongSource { .. }))
        ));
    }

    #[test]
    fn canonical_speaker_pcm_runs_through_output_dsp() {
        let mut output =
            SpeakerOutputStage::new(format(), OutputDspConfig::default()).unwrap();
        let processed = output
            .process_decoded_frame(decoded_frame(OUTPUT_CHANNELS, Vec::new()))
            .unwrap();
        assert_eq!(processed.frame_count, 40);
        assert_eq!(processed.interleaved_f32.len(), 40 * OUTPUT_CHANNELS);
        assert!(processed.interleaved_f32.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn generic_object_metadata_without_signal_bindings_fails_closed() {
        let mut output =
            SpeakerOutputStage::new(format(), OutputDspConfig::default()).unwrap();
        let object = AudioObject {
            id: "object-0".to_owned(),
            position: Vector3::ZERO,
            velocity: Vector3::ZERO,
            gain_db: 0.0,
            spread: 0.0,
            start_time_seconds: None,
            end_time_seconds: None,
        };
        assert!(matches!(
            output.process_decoded_frame(decoded_frame(OUTPUT_CHANNELS, vec![object])),
            Err(RuntimeError::ObjectSignalBindingsRequired { object_count: 1 })
        ));
    }

    #[test]
    fn noncanonical_channel_count_fails_before_output_dsp() {
        let mut output =
            SpeakerOutputStage::new(format(), OutputDspConfig::default()).unwrap();
        assert!(matches!(
            output.process_decoded_frame(decoded_frame(8, Vec::new())),
            Err(RuntimeError::OutputChannelCount {
                expected: OUTPUT_CHANNELS,
                actual: 8
            })
        ));
    }

    #[test]
    fn playback_runtime_keeps_partial_direct_input_silent() {
        let mut runtime = AuroraPlaybackRuntime::new(
            EncodedInputConfig::DirectEarc {
                slots: 2,
                word_half: CarrierWordHalf::High,
            },
            EngineConfig::default(),
            format(),
            OutputDspConfig::default(),
        )
        .unwrap();
        let batch = runtime.push_direct_s32(&[1, 2, 3]).unwrap();
        assert!(batch.frames.is_empty());
        assert_eq!(batch.bursts, 0);
    }

    #[test]
    fn playback_runtime_accepts_native_direct_words() {
        let mut runtime = AuroraPlaybackRuntime::new(
            EncodedInputConfig::DirectEarc {
                slots: 2,
                word_half: CarrierWordHalf::High,
            },
            EngineConfig::default(),
            format(),
            OutputDspConfig::default(),
        )
        .unwrap();
        let batch = runtime.push_direct_s32_words(&[0_i32, 0_i32]).unwrap();
        assert!(batch.frames.is_empty());
        assert_eq!(batch.bursts, 0);
    }

    #[test]
    fn integrated_output_stage_rejects_non_48k_configuration() {
        let mut invalid = format();
        invalid.sample_rate = 96_000;
        assert!(matches!(
            SpeakerOutputStage::new(invalid, OutputDspConfig::default()),
            Err(RuntimeError::OutputSampleRate {
                expected: OUTPUT_SAMPLE_RATE,
                actual: 96_000
            })
        ));
    }
}
