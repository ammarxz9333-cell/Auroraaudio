//! Native open AC-3 / E-AC-3 backend built on the MIT OxideAV codec core.
//!
//! Aurora owns the adapter, timing, float conversion, routing and object-scene
//! boundary. No proprietary runtime library or binary blob is loaded.

use aurora_core::{AudioBlock, AudioFormat, SampleType};
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use oxideav_ac3::decoder::{make_decoder, make_eac3_decoder, make_eac3_decoder_with_joc};
use oxideav_core::{CodecId, CodecParameters, Decoder as OxideDecoder, Frame, Packet, TimeBase};

use crate::sniff::CodecKind;

const AURORA_SEVEN_ONE_FOUR_CHANNELS: usize = 12;
const MAX_RECYCLED_PLANAR_BLOCKS: usize = 32;
const MAX_RECYCLED_PLANAR_FRAMES: usize = 2_048;

/// JOC policy for the native E-AC-3 backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JocPresentation {
    /// Preserve the channel-based E-AC-3 presentation. Aurora's object
    /// extractor/renderer can consume metadata on a separate path.
    Bed,
    /// Use OxideAV's open JOC reconstruction stereo speaker renderer as a
    /// reference/check path. This is not used for the 7.1.4 product path.
    StereoReference,
}

pub struct NativeAc3Decoder {
    codec: CodecKind,
    output_format: Option<AudioFormat>,
    inner: Option<Box<dyn OxideDecoder>>,
    recycled_planar: Vec<Vec<Vec<f32>>>,
    emitted_frames: u64,
    discontinuity: bool,
    joc_presentation: JocPresentation,
}

impl NativeAc3Decoder {
    pub fn new(codec: CodecKind, joc_presentation: JocPresentation) -> Result<Self, DecoderError> {
        if !matches!(codec, CodecKind::Ac3 | CodecKind::Eac3 | CodecKind::Eac3Joc) {
            return Err(DecoderError::UnsupportedInput(
                "native AC-3 backend only accepts AC-3/E-AC-3/JOC",
            ));
        }
        Ok(Self {
            codec,
            output_format: None,
            inner: None,
            recycled_planar: Vec::with_capacity(MAX_RECYCLED_PLANAR_BLOCKS),
            emitted_frames: 0,
            discontinuity: true,
            joc_presentation,
        })
    }

    fn build_inner(&self, output: AudioFormat) -> Result<Box<dyn OxideDecoder>, DecoderError> {
        let codec_id = match self.codec {
            CodecKind::Ac3 => "ac3",
            CodecKind::Eac3 | CodecKind::Eac3Joc => "eac3",
            _ => unreachable!(),
        };
        let mut params = CodecParameters::audio(CodecId::new(codec_id));
        params.sample_rate = Some(output.sample_rate);
        // OxideAV treats Some(1/2) as an explicit compatibility downmix.
        // For immersive/multichannel Aurora targets request native channel
        // presentation by leaving the count unspecified. Aurora then performs
        // only semantic zero-extension for proven mono/stereo/5.1 beds.
        if output.channel_count <= 2 {
            params.channels = Some(output.channel_count as u16);
        }

        let result = match (self.codec, self.joc_presentation, output.channel_count) {
            (CodecKind::Eac3Joc, JocPresentation::StereoReference, 2) => {
                make_eac3_decoder_with_joc(&params)
            }
            (CodecKind::Eac3 | CodecKind::Eac3Joc, _, _) => make_eac3_decoder(&params),
            (CodecKind::Ac3, _, _) => make_decoder(&params),
            _ => unreachable!(),
        };
        result.map_err(|e| DecoderError::ExternalProcess(format!("oxideav decoder init: {e}")))
    }

    fn take_canonical_planar_storage(&mut self, samples: usize) -> Vec<Vec<f32>> {
        if let Some(index) = self.recycled_planar.iter().position(|planar| {
            planar.len() == AURORA_SEVEN_ONE_FOUR_CHANNELS
                && planar.iter().all(|channel| channel.capacity() >= samples)
        }) {
            let mut planar = self.recycled_planar.swap_remove(index);
            for channel in &mut planar {
                channel.clear();
                channel.resize(samples, 0.0);
            }
            return planar;
        }
        (0..AURORA_SEVEN_ONE_FOUR_CHANNELS)
            .map(|_| vec![0.0_f32; samples])
            .collect()
    }

    /// Return a consumed canonical bed frame to bounded decoder-owned storage.
    /// Other shapes are dropped rather than retained because this pool is used
    /// only by Aurora's fixed 7.1.4 product path.
    pub fn recycle_frame(&mut self, frame: DecodedFrame) {
        if !frame.objects.is_empty()
            || self.output_format.map(|format| format.channel_count)
                != Some(AURORA_SEVEN_ONE_FOUR_CHANNELS)
            || frame.audio.channels.len() != AURORA_SEVEN_ONE_FOUR_CHANNELS
            || frame.audio.frame_count == 0
            || frame.audio.frame_count > MAX_RECYCLED_PLANAR_FRAMES
            || frame
                .audio
                .channels
                .iter()
                .any(|channel| channel.len() != frame.audio.frame_count)
            || self.recycled_planar.len() >= MAX_RECYCLED_PLANAR_BLOCKS
        {
            return;
        }
        let mut planar = frame.audio.channels;
        for channel in &mut planar {
            channel.clear();
        }
        self.recycled_planar.push(planar);
    }

    fn convert_audio_frame(
        &mut self,
        frame: oxideav_core::AudioFrame,
        input: &[u8],
    ) -> Result<DecodedFrame, DecoderError> {
        if frame.data.len() != 1 {
            return Err(DecoderError::UnsupportedInput(
                "oxideav AC-3 output was not interleaved S16",
            ));
        }
        let samples = frame.samples as usize;
        if samples == 0 {
            return Ok(DecodedFrame {
                audio: AudioBlock {
                    channels: Vec::new(),
                    frame_count: 0,
                    presentation_time_seconds: 0.0,
                    discontinuity: self.discontinuity,
                },
                objects: Vec::new(),
            });
        }
        let bytes = &frame.data[0];
        let denom = samples
            .checked_mul(2)
            .ok_or(DecoderError::UnsupportedInput("decoded frame size overflow"))?;
        if denom == 0 || bytes.len() % denom != 0 {
            return Err(DecoderError::UnsupportedInput(
                "decoded S16 payload has inconsistent frame/channel size",
            ));
        }
        let channels = bytes.len() / denom;
        if channels == 0 || channels > 64 {
            return Err(DecoderError::UnsupportedInput(
                "decoded channel count outside Aurora limits",
            ));
        }

        let sample_rate = crate::sample_rate_hint(input)
            .or_else(|| self.output_format.map(|f| f.sample_rate))
            .unwrap_or(48_000);
        let output_channels = self
            .output_format
            .map(|format| format.channel_count)
            .unwrap_or(channels);

        let planar = if output_channels == AURORA_SEVEN_ONE_FOUR_CHANNELS {
            let target_map: &[usize] = match channels {
                // AC-3/E-AC-3 1/0 is front centre.
                1 => &[2],
                2 => &[0, 1],
                // OxideAV 0.0.11 emits 3/2+LFE as
                // FL, FR, FC, LFE, Ls, Rs after its WAVE-order reorder.
                6 => &[0, 1, 2, 3, 4, 5],
                _ => {
                    return Err(DecoderError::UnsupportedInput(
                        "native E-AC-3 bed width has no proven mapping to Aurora canonical 7.1.4",
                    ));
                }
            };
            let mut output = self.take_canonical_planar_storage(samples);
            for (frame_index, frame_bytes) in bytes.chunks_exact(channels * 2).enumerate() {
                for (source_channel, &target_channel) in target_map.iter().enumerate() {
                    let at = source_channel * 2;
                    let sample = i16::from_le_bytes([frame_bytes[at], frame_bytes[at + 1]]);
                    output[target_channel][frame_index] = f32::from(sample) / 32768.0;
                }
            }
            output
        } else {
            let mut source = (0..channels)
                .map(|_| Vec::with_capacity(samples))
                .collect::<Vec<_>>();
            for frame_bytes in bytes.chunks_exact(channels * 2) {
                for (channel, dst) in source.iter_mut().enumerate() {
                    let at = channel * 2;
                    let sample = i16::from_le_bytes([frame_bytes[at], frame_bytes[at + 1]]);
                    dst.push(f32::from(sample) / 32768.0);
                }
            }
            normalize_bed_for_output(source, output_channels, samples)?
        };

        let pts = self.emitted_frames as f64 / f64::from(sample_rate);
        self.emitted_frames = self.emitted_frames.saturating_add(samples as u64);
        let discontinuity = std::mem::replace(&mut self.discontinuity, false);
        Ok(DecodedFrame {
            audio: AudioBlock {
                channels: planar,
                frame_count: samples,
                presentation_time_seconds: pts,
                discontinuity,
            },
            // JOC objects are populated by Aurora's metadata bridge rather than
            // silently discarded here. Until that bridge admits a frame, this
            // vector remains empty instead of inventing object coordinates.
            objects: Vec::new(),
        })
    }
}

/// Preserve a proven channel bed inside Aurora's canonical 7.1.4 speaker bus.
///
/// OxideAV 0.0.11 emits non-extended mono/stereo/5.1 in WAVE/SMPTE order.
/// Aurora's first six canonical 7.1.4 roles are exactly
/// FL, FR, FC, LFE, SL, SR, so those signals can be copied losslessly and all
/// absent back/height roles remain digital zero. This is channel preservation,
/// not an upmix and not an Atmos/JOC claim.
///
/// OxideAV currently documents that dependent-substream-extended E-AC-3 (for
/// example 7.1) can remain in a mixed bitstream/appended order. Aurora therefore
/// rejects other source widths when the product requests twelve canonical
/// channels instead of guessing speaker semantics.
fn normalize_bed_for_output(
    planar: Vec<Vec<f32>>,
    output_channels: usize,
    samples: usize,
) -> Result<Vec<Vec<f32>>, DecoderError> {
    let source_channels = planar.len();
    if output_channels != AURORA_SEVEN_ONE_FOUR_CHANNELS {
        return Ok(planar);
    }

    let mut output = (0..AURORA_SEVEN_ONE_FOUR_CHANNELS)
        .map(|_| vec![0.0; samples])
        .collect::<Vec<_>>();
    match source_channels {
        1 => {
            output[2] = planar.into_iter().next().expect("one source channel");
        }
        2 => {
            for (destination, source) in output.iter_mut().take(2).zip(planar) {
                *destination = source;
            }
        }
        6 => {
            for (destination, source) in output.iter_mut().take(6).zip(planar) {
                *destination = source;
            }
        }
        _ => {
            return Err(DecoderError::UnsupportedInput(
                "native E-AC-3 bed width has no proven mapping to Aurora canonical 7.1.4",
            ));
        }
    }
    Ok(output)
}

impl Decoder for NativeAc3Decoder {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "Aurora native open AC-3/E-AC-3 backend (OxideAV core)",
            production_ready: false,
            maturity: "open-native-integration",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        if output_format.sample_type != SampleType::F32
            || output_format.sample_rate == 0
            || output_format.channel_count == 0
            || output_format.channel_count > 64
        {
            return Err(DecoderError::UnsupportedInput(
                "Aurora open decoder requires finite F32 output format with 1..=64 channels",
            ));
        }
        self.inner = Some(self.build_inner(output_format)?);
        self.output_format = Some(output_format);
        self.emitted_frames = 0;
        self.discontinuity = true;
        Ok(())
    }

    fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        if input.is_empty() {
            return Ok(None);
        }
        let output = self
            .output_format
            .ok_or(DecoderError::Unavailable("decoder is not configured"))?;
        if self.inner.is_none() {
            self.inner = Some(self.build_inner(output)?);
        }
        let packet = Packet::new(
            0,
            TimeBase::new(1, i64::from(output.sample_rate)),
            input.to_vec(),
        );
        let inner = self.inner.as_mut().expect("decoder initialized above");
        inner
            .send_packet(&packet)
            .map_err(|e| DecoderError::ExternalProcess(format!("oxideav send_packet: {e}")))?;
        let decoded = inner
            .receive_frame()
            .map_err(|e| DecoderError::ExternalProcess(format!("oxideav receive_frame: {e}")))?;
        match decoded {
            Frame::Audio(audio) => self.convert_audio_frame(audio, input).map(Some),
            _ => Err(DecoderError::UnsupportedInput(
                "audio decoder returned a non-audio frame",
            )),
        }
    }

    fn reset(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            let _ = inner.reset();
        }
        self.emitted_frames = 0;
        self.discontinuity = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn immersive_format() -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: 12,
            sample_type: SampleType::F32,
            block_size: 40,
        }
    }

    #[test]
    fn rejects_unrelated_codec() {
        assert!(NativeAc3Decoder::new(CodecKind::Flac, JocPresentation::Bed).is_err());
    }

    #[test]
    fn accepts_immersive_output_contract() {
        let mut decoder = NativeAc3Decoder::new(CodecKind::Eac3, JocPresentation::Bed).unwrap();
        decoder.configure(immersive_format()).unwrap();
        assert_eq!(decoder.output_format.unwrap().block_size, 40);
    }

    #[test]
    fn five_one_bed_is_zero_extended_without_synthetic_channels() {
        let source = (0..6)
            .map(|channel| vec![channel as f32 + 1.0; 4])
            .collect::<Vec<_>>();
        let output = normalize_bed_for_output(source, 12, 4).unwrap();
        assert_eq!(output.len(), 12);
        for (channel, values) in output.iter().take(6).enumerate() {
            assert_eq!(values, &vec![channel as f32 + 1.0; 4]);
        }
        for values in &output[6..] {
            assert_eq!(values, &vec![0.0; 4]);
        }
    }

    #[test]
    fn stereo_bed_populates_front_pair_only() {
        let source = vec![vec![1.0; 3], vec![2.0; 3]];
        let output = normalize_bed_for_output(source, 12, 3).unwrap();
        assert_eq!(output[0], vec![1.0; 3]);
        assert_eq!(output[1], vec![2.0; 3]);
        for values in &output[2..] {
            assert_eq!(values, &vec![0.0; 3]);
        }
    }

    #[test]
    fn canonical_planar_pool_reuses_channel_allocations() {
        let mut decoder = NativeAc3Decoder::new(CodecKind::Eac3, JocPresentation::Bed).unwrap();
        decoder.configure(immersive_format()).unwrap();
        let frame = DecodedFrame {
            audio: AudioBlock {
                channels: (0..12).map(|_| vec![0.0; 1536]).collect(),
                frame_count: 1536,
                presentation_time_seconds: 0.0,
                discontinuity: false,
            },
            objects: Vec::new(),
        };
        let first_ptr = frame.audio.channels[0].as_ptr();
        decoder.recycle_frame(frame);

        let recycled = decoder.take_canonical_planar_storage(1536);
        assert_eq!(recycled[0].as_ptr(), first_ptr);
        assert_eq!(recycled.len(), 12);
        assert!(recycled.iter().all(|channel| channel.len() == 1536));
    }

    #[test]
    fn unproven_extended_bed_mapping_fails_closed() {
        let source = (0..8).map(|_| vec![0.0; 2]).collect::<Vec<_>>();
        let error = normalize_bed_for_output(source, 12, 2).unwrap_err();
        assert!(error.to_string().contains("no proven mapping"));
    }
}
