use std::collections::VecDeque;

use aurora_core::{AudioBlock, AudioFormat};
use aurora_decoder_api::{DecodedFrame, DecoderError};
use oxideav_ac4::decoder::Ac4Decoder;
use oxideav_ac4::sync::{parse_sync_frame_at_start, SYNC_WORD_CRC, SYNC_WORD_PLAIN};
use oxideav_ac4::toc::Ac4FrameInfo;
use oxideav_core::{CodecId, CodecParameters, Decoder as OxideDecoder, Frame, Packet, TimeBase};

const MAX_COMPRESSED_BUFFER: usize = 8 * 1024 * 1024;
const MAX_QUEUE_BLOCKS: usize = 4;
const MAX_PCM_QUEUE_FRAMES: usize = 8_192;

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
    dropped_bytes: u64,
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
            dropped_bytes: 0,
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
            return self.poll();
        }
        if input.len() > MAX_COMPRESSED_BUFFER {
            return Err(DecoderError::Decode(
                "AC-4 input packet exceeded bounded compressed size".into(),
            ));
        }

        if packetized_raw && !looks_like_ac4_sync(input) {
            self.decode_packet(input.to_vec())?;
        } else {
            self.compressed.extend_from_slice(input);
            if self.compressed.len() > MAX_COMPRESSED_BUFFER {
                return Err(DecoderError::Decode(
                    "AC-4 compressed input exceeded bounded buffer".into(),
                ));
            }
            self.process_sync_stream()?;
        }
        self.take_block(false)
    }

    pub fn poll(&mut self) -> Result<Option<DecodedFrame>, DecoderError> {
        // A prior push may have stopped at the PCM high-water mark while
        // retaining complete compressed frames. Once the consumer drains a
        // block, polling must resume decode from that retained backlog.
        self.process_sync_stream()?;
        self.take_block(false)
    }

    pub fn reset(&mut self) {
        let output = self.output;
        *self = Self::new();
        if let Some(output) = output {
            self.configure(output);
        }
    }

    pub fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
    }

    fn ensure_pcm_channels(&mut self) {
        let channels = self.output.map(|f| f.channel_count).unwrap_or(0);
        if self.pcm.len() != channels {
            self.pcm = (0..channels).map(|_| VecDeque::new()).collect();
        }
    }

    fn queued_frames(&self) -> usize {
        self.pcm.iter().map(VecDeque::len).min().unwrap_or(0)
    }

    fn queue_high_watermark(&self) -> usize {
        self.output
            .map(|format| format.block_size.max(1).saturating_mul(MAX_QUEUE_BLOCKS))
            .unwrap_or(160)
    }

    fn process_sync_stream(&mut self) -> Result<(), DecoderError> {
        loop {
            if self.queued_frames() >= self.queue_high_watermark() {
                break;
            }
            if self.compressed.len() < 2 {
                break;
            }

            if !looks_like_ac4_sync(&self.compressed) {
                if let Some(offset) = find_next_sync_prefix(&self.compressed) {
                    self.dropped_bytes = self.dropped_bytes.saturating_add(offset as u64);
                    self.discontinuity = true;
                    self.compressed.drain(..offset);
                    continue;
                }
                let keep_trailing_ac = self.compressed.last() == Some(&0xAC);
                let keep = usize::from(keep_trailing_ac);
                let drop = self.compressed.len().saturating_sub(keep);
                self.dropped_bytes = self.dropped_bytes.saturating_add(drop as u64);
                if drop > 0 {
                    self.discontinuity = true;
                }
                self.compressed.clear();
                if keep_trailing_ac {
                    self.compressed.push(0xAC);
                }
                break;
            }

            let Some(frame) = parse_sync_frame_at_start(&self.compressed) else {
                // Sync is plausible but the complete frame has not arrived yet.
                // The outer input bound prevents an attacker from growing this
                // indefinitely while advertising an impossible frame.
                break;
            };

            if frame.crc_valid == Some(false) {
                return Err(DecoderError::Decode(
                    "AC-4 Annex G CRC verification failed".into(),
                ));
            }
            let total_len = frame.total_len;
            if total_len == 0 || total_len > MAX_COMPRESSED_BUFFER {
                return Err(DecoderError::Decode(
                    "AC-4 sync frame declared an invalid bounded size".into(),
                ));
            }
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
        if self.queued_frames().saturating_add(samples) > MAX_PCM_QUEUE_FRAMES {
            return Err(DecoderError::Decode(
                "AC-4 decoded PCM exceeded bounded queue".into(),
            ));
        }
        let info = self
            .decoder
            .last_info
            .as_ref()
            .ok_or_else(|| DecoderError::Decode("AC-4 frame has no parsed TOC info".into()))?;
        let source_rate = info.sample_rate;
        if source_rate != output.sample_rate {
            return Err(DecoderError::UnsupportedInput(
                "native AC-4 sample-rate conversion is not wired yet",
            ));
        }

        // OxideAV's A-JOC path currently emits reconstructed object signals as
        // PCM lanes. Those are NOT speaker channels and must never be wired
        // directly to Aurora outputs. Until the object coordinates/OAMD side
        // channel is exported into Aurora's scene model, fail closed.
        if !info.ajoc_substreams.is_empty() || !info.obj_substreams.is_empty() {
            return Err(DecoderError::UnsupportedInput(
                "AC-4 object-coded presentation requires Aurora object-metadata/render integration",
            ));
        }

        let Some(interleaved) = audio.data.first() else {
            return Err(DecoderError::Decode("AC-4 frame has no PCM plane".into()));
        };
        let bytes_per_sample = 2usize;
        let frame_bytes = samples
            .checked_mul(bytes_per_sample)
            .ok_or_else(|| DecoderError::Decode("AC-4 frame size overflow".into()))?;
        if frame_bytes == 0 || interleaved.len() % frame_bytes != 0 {
            return Err(DecoderError::Decode(
                "AC-4 S16 output has inconsistent frame geometry".into(),
            ));
        }
        let source_channels = interleaved.len() / frame_bytes;
        if source_channels == 0 {
            return Err(DecoderError::Decode("AC-4 decoded zero channels".into()));
        }

        let map = ac4_target_map(info, source_channels, output.channel_count)?;
        self.ensure_pcm_channels();
        for frame_index in 0..samples {
            for (target, source) in map.iter().enumerate() {
                let sample = if let Some(source) = source {
                    let offset = (frame_index * source_channels + *source) * 2;
                    let value = i16::from_le_bytes([interleaved[offset], interleaved[offset + 1]]);
                    f32::from(value) / 32768.0
                } else {
                    0.0
                };
                self.pcm[target].push_back(sample);
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
        let available = self.queued_frames();
        let wanted = output.block_size.max(1);
        // Retain a short PCM tail only while a sync-framed AC-4 packet is still
        // incomplete. Once compressed staging is empty, the remaining samples
        // were decoded from complete input and must be released rather than
        // disappearing on a later reset or backend transition.
        if available < wanted && !allow_short && !self.compressed.is_empty() {
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

/// Return one source index per Aurora output channel. `None` means silence.
///
/// This adapter intentionally supports only layouts whose upstream lane order
/// is explicit and unambiguous. Unknown layouts fail closed rather than routing
/// a valid decode to the wrong loudspeakers.
fn ac4_target_map(
    info: &Ac4FrameInfo,
    source_channels: usize,
    output_channels: usize,
) -> Result<Vec<Option<usize>>, DecoderError> {
    if source_channels > output_channels {
        return Err(DecoderError::UnsupportedInput(
            "AC-4 source has more channels than configured Aurora output",
        ));
    }

    // Mono/stereo are unambiguous and map to the first Aurora front lanes.
    if source_channels <= 2 {
        let mut map = vec![None; output_channels];
        for index in 0..source_channels {
            map[index] = Some(index);
        }
        return Ok(map);
    }

    // OxideAV immersive 7.1.4 output is explicitly documented as
    // [LFE, L, R, C, Ls, Rs, Lb, Rb, Tfl, Tfr, Tbl, Tbr]. Aurora's
    // canonical 7.1.4 order is
    // [FL, FR, C, LFE, SL, SR, BL, BR, TFL, TFR, TBL, TBR].
    if info.first_chan_mode.map(|mode| mode.ch_mode) == Some(12)
        && source_channels == 12
        && output_channels == 12
    {
        return Ok(vec![
            Some(1),
            Some(2),
            Some(3),
            Some(0),
            Some(4),
            Some(5),
            Some(6),
            Some(7),
            Some(8),
            Some(9),
            Some(10),
            Some(11),
        ]);
    }

    Err(DecoderError::UnsupportedInput(
        "AC-4 decoded layout has no accepted Aurora channel-order mapping yet",
    ))
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

    fn format(channels: usize, block_size: usize) -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: channels,
            sample_type: SampleType::F32,
            block_size,
        }
    }

    #[test]
    fn recognizes_both_annex_g_sync_words() {
        assert!(looks_like_ac4_sync(&[0xAC, 0x40, 0, 0]));
        assert!(looks_like_ac4_sync(&[0xAC, 0x41, 0, 0]));
        assert!(!looks_like_ac4_sync(&[0x0B, 0x77, 0, 0]));
    }

    #[test]
    fn adapter_accepts_aurora_40_frame_configuration() {
        let mut decoder = NativeAc4Decoder::new();
        decoder.configure(format(12, 40));
        assert_eq!(decoder.queue_high_watermark(), 160);
        assert!(decoder.poll().unwrap().is_none());
    }

    #[test]
    fn resynchronizer_finds_ac4_after_noise() {
        assert_eq!(find_next_sync_prefix(&[1, 2, 3, 0xAC, 0x40, 0]), Some(3));
    }

    #[test]
    fn resynchronization_counts_dropped_bytes_and_keeps_split_sync_prefix() {
        let mut decoder = NativeAc4Decoder::new();
        decoder.configure(format(2, 40));
        assert!(decoder.push(&[1, 2, 3, 0xAC], false).unwrap().is_none());
        assert_eq!(decoder.dropped_bytes(), 3);
        assert_eq!(decoder.compressed, vec![0xAC]);
    }

    #[test]
    fn complete_ac4_input_releases_short_pcm_tail() {
        let mut decoder = NativeAc4Decoder::new();
        decoder.configure(format(12, 40));
        for queue in &mut decoder.pcm {
            queue.extend(std::iter::repeat(0.25_f32).take(17));
        }
        assert!(decoder.compressed.is_empty());

        let tail = decoder.take_block(false).unwrap().unwrap();

        assert_eq!(tail.audio.frame_count, 17);
        assert_eq!(tail.audio.channels.len(), 12);
        assert!(tail.audio.channels.iter().all(|channel| channel.len() == 17));
        assert_eq!(decoder.queued_frames(), 0);
    }

    #[test]
    fn partial_sync_framed_ac4_input_holds_short_pcm_tail() {
        let mut decoder = NativeAc4Decoder::new();
        decoder.configure(format(12, 40));
        for queue in &mut decoder.pcm {
            queue.extend(std::iter::repeat(0.25_f32).take(17));
        }
        decoder.compressed.push(0xAC);

        assert!(decoder.take_block(false).unwrap().is_none());
        assert_eq!(decoder.queued_frames(), 17);
    }
}
