use std::collections::VecDeque;

use aurora_core::{AudioBlock, AudioFormat};
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use iamf_codecs::DefaultFactory;
use iamf_dec::layout::SoundSystem;
use iamf_dec::stream::{OutputSampleType, StreamDecoder, StreamSettings};
use iamf_obu::{ByteReader, Error as IamfObuError, Obu, ObuType};

const MAX_DESCRIPTOR_BOOTSTRAP_BYTES: usize = 4 * 1024 * 1024;
const PCM32_SCALE: f32 = 2_147_483_648.0;

/// Native IAMF v1.1 streaming decoder backed by the pinned `iamf-rs` runtime.
///
/// The backend deliberately renders only to unambiguous Aurora layouts that can
/// be inferred from the legacy `AudioFormat` channel count. Ambiguous counts are
/// rejected until Aurora's decoder API carries an explicit semantic layout.
pub struct IamfDecoderAdapter {
    configured_format: Option<AudioFormat>,
    decoder: Option<StreamDecoder>,
    bootstrap: Vec<u8>,
    pcm_fifo: Vec<VecDeque<f32>>,
    ready: VecDeque<DecodedFrame>,
    emitted_frames: u64,
    discontinuity: bool,
}

impl core::fmt::Debug for IamfDecoderAdapter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("IamfDecoderAdapter")
            .field("configured_format", &self.configured_format)
            .field("initialized", &self.decoder.is_some())
            .field("bootstrap_bytes", &self.bootstrap.len())
            .field("ready_blocks", &self.ready.len())
            .finish_non_exhaustive()
    }
}

impl Default for IamfDecoderAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl IamfDecoderAdapter {
    pub fn new() -> Self {
        Self {
            configured_format: None,
            decoder: None,
            bootstrap: Vec::new(),
            pcm_fifo: Vec::new(),
            ready: VecDeque::new(),
            emitted_frames: 0,
            discontinuity: true,
        }
    }

    fn configured(&self) -> Result<AudioFormat, DecoderError> {
        self.configured_format.ok_or(DecoderError::Unavailable(
            "IAMF adapter must be configured before decoding",
        ))
    }

    fn initialize_if_ready(&mut self) -> Result<(), DecoderError> {
        if self.decoder.is_some() {
            return Ok(());
        }
        let Some(split) = descriptor_split(&self.bootstrap)? else {
            if self.bootstrap.len() > MAX_DESCRIPTOR_BOOTSTRAP_BYTES {
                return Err(DecoderError::Decode(
                    "IAMF descriptor bootstrap exceeded the bounded input window".into(),
                ));
            }
            return Ok(());
        };
        if split == 0 || split > self.bootstrap.len() {
            return Err(DecoderError::Decode(
                "IAMF stream did not provide a valid descriptor prefix".into(),
            ));
        }

        let format = self.configured()?;
        let layout = sound_system_for_format(format)?;
        let mut settings = StreamSettings::default();
        settings.layout = layout;
        settings.sample_type = Some(OutputSampleType::Int32LittleEndian);
        settings.enable_limiter = false;
        settings.loudness_target_db = None;

        let descriptors = self.bootstrap[..split].to_vec();
        let media = self.bootstrap[split..].to_vec();
        let mut decoder = StreamDecoder::new_from_descriptors(&descriptors, settings, &DefaultFactory)
            .map_err(|error| DecoderError::Decode(format!("IAMF descriptor decode failed: {error}")))?;
        if decoder.num_output_channels() != format.channel_count {
            return Err(DecoderError::Decode(format!(
                "IAMF renderer output geometry mismatch: requested={}, got={}",
                format.channel_count,
                decoder.num_output_channels()
            )));
        }
        self.pcm_fifo = (0..format.channel_count).map(|_| VecDeque::new()).collect();
        self.bootstrap.clear();
        if !media.is_empty() {
            decoder
                .decode(&media)
                .map_err(|error| DecoderError::Decode(format!("IAMF media decode failed: {error}")))?;
        }
        self.decoder = Some(decoder);
        self.drain_decoder()
    }

    fn drain_decoder(&mut self) -> Result<(), DecoderError> {
        loop {
            let unit = {
                let Some(decoder) = self.decoder.as_mut() else {
                    return Ok(());
                };
                decoder
                    .get_output_temporal_unit()
                    .map_err(|error| DecoderError::Decode(format!("IAMF render failed: {error}")))?
            };
            let Some(unit) = unit else {
                break;
            };
            self.push_pcm32_unit(&unit)?;
        }
        self.emit_blocks()
    }

    fn push_pcm32_unit(&mut self, bytes: &[u8]) -> Result<(), DecoderError> {
        let format = self.configured()?;
        let channels = format.channel_count;
        let stride = channels
            .checked_mul(4)
            .ok_or_else(|| DecoderError::Decode("IAMF PCM stride overflow".into()))?;
        if stride == 0 || bytes.len() % stride != 0 {
            return Err(DecoderError::Decode(
                "IAMF renderer returned malformed interleaved s32 PCM".into(),
            ));
        }
        if self.pcm_fifo.len() != channels {
            return Err(DecoderError::Decode(
                "IAMF PCM FIFO geometry is not initialized".into(),
            ));
        }
        for frame in bytes.chunks_exact(stride) {
            for channel in 0..channels {
                let offset = channel * 4;
                let sample = i32::from_le_bytes([
                    frame[offset],
                    frame[offset + 1],
                    frame[offset + 2],
                    frame[offset + 3],
                ]);
                self.pcm_fifo[channel].push_back(sample as f32 / PCM32_SCALE);
            }
        }
        Ok(())
    }

    fn emit_blocks(&mut self) -> Result<(), DecoderError> {
        let format = self.configured()?;
        let block_size = format.block_size;
        if block_size == 0 {
            return Err(DecoderError::UnsupportedInput(
                "IAMF output block size must be non-zero",
            ));
        }
        loop {
            if self
                .pcm_fifo
                .iter()
                .any(|channel| channel.len() < block_size)
            {
                break;
            }
            let sample_rate = self
                .decoder
                .as_ref()
                .map(StreamDecoder::sample_rate)
                .unwrap_or(0);
            if sample_rate == 0 {
                return Err(DecoderError::Decode(
                    "IAMF decoder has not resolved a sample rate".into(),
                ));
            }
            if sample_rate != format.sample_rate {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF sample-rate conversion is not wired into the native adapter yet",
                ));
            }
            let mut channels = Vec::with_capacity(format.channel_count);
            for fifo in &mut self.pcm_fifo {
                let mut plane = Vec::with_capacity(block_size);
                for _ in 0..block_size {
                    plane.push(fifo.pop_front().expect("length checked above"));
                }
                channels.push(plane);
            }
            let pts = self.emitted_frames as f64 / f64::from(sample_rate);
            self.emitted_frames = self.emitted_frames.saturating_add(block_size as u64);
            let discontinuity = std::mem::replace(&mut self.discontinuity, false);
            self.ready.push_back(DecodedFrame {
                audio: AudioBlock {
                    channels,
                    frame_count: block_size,
                    presentation_time_seconds: pts,
                    discontinuity,
                },
                objects: Vec::new(),
            });
        }
        Ok(())
    }
}

impl Decoder for IamfDecoderAdapter {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "Aurora native iamf-rs decoder",
            production_ready: false,
            maturity: "native-iamf-rs-streaming-rendered-v1",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        let _ = sound_system_for_format(output_format)?;
        if output_format.sample_rate == 0 || output_format.block_size == 0 {
            return Err(DecoderError::UnsupportedInput(
                "IAMF output format requires non-zero sample rate and block size",
            ));
        }
        self.configured_format = Some(output_format);
        self.decoder = None;
        self.bootstrap.clear();
        self.pcm_fifo.clear();
        self.ready.clear();
        self.emitted_frames = 0;
        self.discontinuity = true;
        Ok(())
    }

    fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        if let Some(frame) = self.ready.pop_front() {
            if !input.is_empty() {
                if let Some(decoder) = self.decoder.as_mut() {
                    decoder
                        .decode(input)
                        .map_err(|error| DecoderError::Decode(format!("IAMF decode failed: {error}")))?;
                    self.drain_decoder()?;
                } else {
                    self.bootstrap.extend_from_slice(input);
                    self.initialize_if_ready()?;
                }
            }
            return Ok(Some(frame));
        }

        if input.is_empty() {
            return Ok(None);
        }
        if let Some(decoder) = self.decoder.as_mut() {
            decoder
                .decode(input)
                .map_err(|error| DecoderError::Decode(format!("IAMF decode failed: {error}")))?;
            self.drain_decoder()?;
        } else {
            self.bootstrap.extend_from_slice(input);
            self.initialize_if_ready()?;
        }
        Ok(self.ready.pop_front())
    }

    fn reset(&mut self) {
        let configured = self.configured_format;
        *self = Self::new();
        self.configured_format = configured;
        if let Some(format) = configured {
            self.pcm_fifo = (0..format.channel_count).map(|_| VecDeque::new()).collect();
        }
    }
}

fn sound_system_for_format(format: AudioFormat) -> Result<SoundSystem, DecoderError> {
    match format.channel_count {
        1 => Ok(SoundSystem::Mono),
        2 => Ok(SoundSystem::A),
        6 => Ok(SoundSystem::B),
        12 => Ok(SoundSystem::J),
        _ => Err(DecoderError::UnsupportedInput(
            "IAMF native adapter currently admits only unambiguous 1.0, 2.0, 5.1, and 7.1.4 output layouts",
        )),
    }
}

/// Returns the byte offset of the first temporal OBU once the complete
/// descriptor prefix and at least one non-descriptor OBU are available.
/// Reaching the end immediately after a descriptor means more bytes may still
/// contain descriptor OBUs, so the adapter waits rather than initializing with
/// an incomplete descriptor set.
fn descriptor_split(data: &[u8]) -> Result<Option<usize>, DecoderError> {
    let mut reader = ByteReader::new(data);
    let mut end = 0usize;
    loop {
        let position = reader.position();
        let obu = match Obu::parse(&mut reader) {
            Ok(obu) => obu,
            Err(IamfObuError::UnexpectedEof { .. }) => return Ok(None),
            Err(error) => {
                return Err(DecoderError::Decode(format!(
                    "IAMF OBU bootstrap parse failed: {error}"
                )))
            }
        };
        match obu.header.obu_type {
            ObuType::SequenceHeader
            | ObuType::CodecConfig
            | ObuType::AudioElement
            | ObuType::MixPresentation => {
                end = reader.position();
                if end >= data.len() {
                    return Ok(None);
                }
            }
            _ => {
                if end == 0 {
                    return Err(DecoderError::Decode(
                        "IAMF temporal data arrived before descriptor OBUs".into(),
                    ));
                }
                return Ok(Some(position.max(end)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::SampleType;

    fn format(channels: usize) -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: channels,
            sample_type: SampleType::F32,
            block_size: 40,
        }
    }

    #[test]
    fn target_layouts_are_fail_closed_when_channel_count_is_ambiguous() {
        assert_eq!(sound_system_for_format(format(12)).unwrap(), SoundSystem::J);
        assert!(sound_system_for_format(format(8)).is_err());
        assert!(sound_system_for_format(format(10)).is_err());
    }

    #[test]
    fn pcm32_full_scale_maps_inside_f32_contract() {
        assert_eq!(i32::MIN as f32 / PCM32_SCALE, -1.0);
        assert!((i32::MAX as f32 / PCM32_SCALE) <= 1.0);
    }
}
