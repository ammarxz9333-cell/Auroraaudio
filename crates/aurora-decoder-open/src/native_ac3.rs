//! Native open AC-3 / E-AC-3 backend built on the MIT OxideAV codec core.
//!
//! Aurora owns the adapter, timing, float conversion, routing and object-scene
//! boundary. No proprietary runtime library or binary blob is loaded.

use aurora_core::{AudioBlock, AudioFormat, SampleType};
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use oxideav_ac3::decoder::{make_decoder, make_eac3_decoder, make_eac3_decoder_with_joc};
use oxideav_core::{CodecId, CodecParameters, Decoder as OxideDecoder, Frame, Packet, TimeBase};

use crate::sniff::CodecKind;

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
        // presentation by leaving the count unspecified.
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
        let mut planar = (0..channels)
            .map(|_| Vec::with_capacity(samples))
            .collect::<Vec<_>>();
        for frame_bytes in bytes.chunks_exact(channels * 2) {
            for (channel, dst) in planar.iter_mut().enumerate() {
                let at = channel * 2;
                let sample = i16::from_le_bytes([frame_bytes[at], frame_bytes[at + 1]]);
                dst.push(f32::from(sample) / 32768.0);
            }
        }

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

    #[test]
    fn rejects_unrelated_codec() {
        assert!(NativeAc3Decoder::new(CodecKind::Flac, JocPresentation::Bed).is_err());
    }

    #[test]
    fn accepts_immersive_output_contract() {
        let mut decoder = NativeAc3Decoder::new(CodecKind::Eac3, JocPresentation::Bed).unwrap();
        decoder
            .configure(AudioFormat {
                sample_rate: 48_000,
                channel_count: 12,
                sample_type: SampleType::F32,
                block_size: 40,
            })
            .unwrap();
        assert_eq!(decoder.output_format.unwrap().block_size, 40);
    }
}
