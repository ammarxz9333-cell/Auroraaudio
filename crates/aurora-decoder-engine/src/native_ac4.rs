use std::collections::VecDeque;

use aurora_core::{AudioBlock, AudioFormat};
use aurora_decoder_api::{DecodedFrame, DecoderError};
use oxideav_ac4::decoder::Ac4Decoder;
use oxideav_ac4::sync::{parse_sync_frame_at_start, SYNC_WORD_CRC, SYNC_WORD_PLAIN};
use oxideav_core::{CodecId, CodecParameters, Decoder as OxideDecoder, Frame, Packet, TimeBase};

const MAX_COMPRESSED_BUFFER: usize = 8 * 1024 * 1024;

pub fn looks_like_ac4_sync(input: &[u8]) -> bool {
    if input.len() < 2 {
        return false;
    }
    let sync = u16::from_be_bytes([input[0], input[1]]);
    sync == SYNC_WORD_PLAIN || sync == SYNC_WORD_CRC
}

pub struct NativeAc4Decoder {
    decoder: Ac4Decoder,
    output: Option<AudioFormat>,
    compressed: Vec<u8>,
    pcm: Vec<VecDeque<f32>>,
    emitted_frames: u64,
    discontinuity: bool,
}

impl NativeAc4Decoder {
    pub fn new() -> Self {
        let params = CodecParameters::audio(CodecId::new("ac4"));
        Self {
            decoder: Ac4Decoder::new(&params),
            output: None,
            compressed: Vec::new(),
            pcm: Vec::new(),
            emitted_frames: 0,
            discontinuity: true,
        }
    }

    pub fn configure(&mut self, output: AudioFormat) {
        self.output = Some(output);
        self.ensure_pcm_channels();
    }

    pub fn push(
        &mut self,
        input: &[u8],
        packetized_raw: bool,
    ) -> Result<Option<DecodedFrame>, DecoderError> {
        if input.is_empty() {
            return self.take_block(false);
        }

        if packetized_raw && !looks_like_ac4_sync(input) {
            self.decode_packet(input.to_vec())?;
        } else {
            self.compressed.extend_from_slice(input);
            self.process_sync_stream()?;
        }
        self.take_block(false)
    }

    pub fn poll(&mut self) -> Result<Option<DecodedFrame>, DecoderError> {
        self.take_block(false)
    }

    pub fn reset(&mut self) {
        let output = self.output;
        *self = Self::new();
        if let Some(output) = output {
            self.configure(output);
        }
    }

    fn ensure_pcm_channels(&mut self) {
        let channels = self.output.map(|f| f.channel_count).unwrap_or(0);
        if self.pcm.len() != channels {
            self.pcm = (0..channels).map(|_| VecDeque::new()).collect();
        }
    }

    fn process_sync_stream(&mut self) -> Result<(), DecoderError> {
        loop {
            if self.compressed.len() < 2 {
                break;
            }

            if !looks_like_ac4_sync(&self.compressed) {
                if let Some(offset) = find_next_sync_prefix(&self.compressed) {
                    self.compressed.drain(..offset);
                    continue;
                }
                let keep_trailing_ac = self.compressed.last() == Some(&0xAC);
                self.compressed.clear();
                if keep_trailing_ac {
                    self.compressed.push(0xAC);
                }
                break;
            }

            let Some(frame) = parse_sync_frame_at_start(&self.compressed) else {
                if self.compressed.len() > MAX_COMPRESSED_BUFFER {
                    return Err(DecoderError::Decode(
                        "AC-4 sync frame exceeded bounded compressed buffer".into(),
                    ));
                }
                break;
            };

            if frame.crc_valid == Some(false) {
                return Err(DecoderError::Decode(
                    "AC-4 Annex G CRC verification failed".into(),
                ));
            }
            let total_len = frame.total_len;
            let packet = self.compressed[..total_len].to_vec();
            self.compressed.drain(..total_len);
            self.decode_packet(packet)?;
        }
        Ok(())
    }

    fn decode_packet(&mut self, packet: Vec<u8>) -> Result<(), DecoderError> {
        let output = self
            .output
            .ok_or(DecoderError::Unavailable("AC-4 decoder is not configured"))?;
        let pkt = Packet::new(0, TimeBase::new(1, i64::from(output.sample_rate)), packet);
        self.decoder
            .send_packet(&pkt)
            .map_err(|e| DecoderError::Decode(format!("AC-4 packet rejected: {e}")))?;
        let frame = self
            .decoder
            .receive_frame()
            .map_err(|e| DecoderError::Decode(format!("AC-4 frame decode failed: {e}")))?;
        let Frame::Audio(audio) = frame else {
            return Err(DecoderError::Decode(
                "AC-4 backend returned a non-audio frame".into(),
            ));
        };

        let samples = audio.samples as usize;
        if samples == 0 {
            return Ok(());
        }
        let source_rate = self
            .decoder
            .last_info
            .as_ref()
            .map(|info| info.sample_rate)
            .unwrap_or(output.sample_rate);
        if source_rate != output.sample_rate {
            return Err(DecoderError::UnsupportedInput(
                "native AC-4 sample-rate conversion is not wired yet",
            ));
        }

        let Some(interleaved) = audio.data.first() else {
            return Err(DecoderError::Decode("AC-4 frame has no PCM plane".into()));
        };
        let bytes_per_sample = 2usize;
        let frame_bytes = samples
            .checked_mul(bytes_per_sample)
            .ok_or(DecoderError::Decode("AC-4 frame size overflow".into()))?;
        if frame_bytes == 0 || interleaved.len() % frame_bytes != 0 {
            return Err(DecoderError::Decode(
                "AC-4 S16 output has inconsistent frame geometry".into(),
            ));
        }
        let source_channels = interleaved.len() / frame_bytes;
        if source_channels == 0 {
            return Err(DecoderError::Decode("AC-4 decoded zero channels".into()));
        }
        if source_channels > output.channel_count {
            return Err(DecoderError::UnsupportedInput(
                "AC-4 source has more channels than configured Aurora output; layout renderer required",
            ));
        }

        self.ensure_pcm_channels();
        for frame_index in 0..samples {
            for channel in 0..output.channel_count {
                let sample = if channel < source_channels {
                    let offset = (frame_index * source_channels + channel) * 2;
                    let value = i16::from_le_bytes([interleaved[offset], interleaved[offset + 1]]);
                    f32::from(value) / 32768.0
                } else {
                    0.0
                };
                self.pcm[channel].push_back(sample);
            }
        }
        Ok(())
    }

    fn take_block(&mut self, allow_short: bool) -> Result<Option<DecodedFrame>, DecoderError> {
        let output = self
            .output
            .ok_or(DecoderError::Unavailable("AC-4 decoder is not configured"))?;
        if self.pcm.is_empty() {
            return Ok(None);
        }
        let available = self.pcm.iter().map(VecDeque::len).min().unwrap_or(0);
        let wanted = output.block_size.max(1);
        if available < wanted && !allow_short {
            return Ok(None);
        }
        let frame_count = available.min(wanted);
        if frame_count == 0 {
            return Ok(None);
        }

        let mut channels = Vec::with_capacity(output.channel_count);
        for queue in &mut self.pcm {
            let mut channel = Vec::with_capacity(frame_count);
            for _ in 0..frame_count {
                channel.push(queue.pop_front().unwrap_or(0.0));
            }
            channels.push(channel);
        }
        let pts = self.emitted_frames as f64 / f64::from(output.sample_rate);
        self.emitted_frames = self.emitted_frames.saturating_add(frame_count as u64);
        let discontinuity = std::mem::replace(&mut self.discontinuity, false);
        Ok(Some(DecodedFrame {
            audio: AudioBlock {
                channels,
                frame_count,
                presentation_time_seconds: pts,
                discontinuity,
            },
            objects: Vec::new(),
        }))
    }
}

impl Default for NativeAc4Decoder {
    fn default() -> Self {
        Self::new()
    }
}

fn find_next_sync_prefix(data: &[u8]) -> Option<usize> {
    data.windows(2).position(|pair| {
        pair == SYNC_WORD_PLAIN.to_be_bytes() || pair == SYNC_WORD_CRC.to_be_bytes()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::SampleType;

    #[test]
    fn recognizes_both_annex_g_sync_words() {
        assert!(looks_like_ac4_sync(&[0xAC, 0x40, 0, 0]));
        assert!(looks_like_ac4_sync(&[0xAC, 0x41, 0, 0]));
        assert!(!looks_like_ac4_sync(&[0x0B, 0x77, 0, 0]));
    }

    #[test]
    fn adapter_accepts_aurora_40_frame_configuration() {
        let mut decoder = NativeAc4Decoder::new();
        decoder.configure(AudioFormat {
            sample_rate: 48_000,
            channel_count: 12,
            sample_type: SampleType::F32,
            block_size: 40,
        });
        assert!(decoder.poll().unwrap().is_none());
    }

    #[test]
    fn resynchronizer_finds_ac4_after_noise() {
        assert_eq!(find_next_sync_prefix(&[1, 2, 3, 0xAC, 0x40, 0]), Some(3));
    }
}
