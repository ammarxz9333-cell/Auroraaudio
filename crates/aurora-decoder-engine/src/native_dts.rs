use std::collections::VecDeque;

use aurora_core::{AudioBlock, AudioFormat};
use aurora_decoder_api::{DecodedFrame, DecoderError};
use oxideav_dts::{
    find_next_sync, parse_frame_header, parse_frame_header_14bit, unpack_14bit_to_16bit,
    AmodeArrangement, CoreStreamDecoder, DtsFrameHeader, FourteenBitByteOrder, SyncWordEncoding,
};

const MAX_COMPRESSED_BUFFER: usize = 8 * 1024 * 1024;
const MAX_QUEUE_BLOCKS: usize = 4;
const DTS_SYNC_PREFIXES: [[u8; 4]; 4] = [
    [0x7F, 0xFE, 0x80, 0x01],
    [0xFE, 0x7F, 0x01, 0x80],
    [0x1F, 0xFF, 0xE8, 0x00],
    [0xFF, 0x1F, 0x00, 0xE8],
];

pub fn looks_like_dts_sync(input: &[u8]) -> bool {
    DTS_SYNC_PREFIXES.iter().any(|sync| input.starts_with(sync))
}

fn trailing_dts_sync_prefix_len(input: &[u8]) -> Option<usize> {
    let max = input.len().min(3);
    (1..=max).rev().find(|&len| {
        DTS_SYNC_PREFIXES
            .iter()
            .any(|sync| input[input.len() - len..] == sync[..len])
    })
}

pub struct NativeDtsDecoder {
    output: Option<AudioFormat>,
    compressed: Vec<u8>,
    pcm: Vec<VecDeque<f32>>,
    stream: Option<CoreStreamDecoder>,
    emitted_frames: u64,
    dropped_bytes: u64,
    discontinuity: bool,
    finalizing: bool,
}

impl NativeDtsDecoder {
    pub fn new() -> Self {
        Self {
            output: None,
            compressed: Vec::new(),
            pcm: Vec::new(),
            stream: None,
            emitted_frames: 0,
            dropped_bytes: 0,
            discontinuity: true,
            finalizing: false,
        }
    }

    pub fn configure(&mut self, output: AudioFormat) {
        self.output = Some(output);
        self.ensure_pcm_channels();
    }

    pub fn push(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        if !input.is_empty() {
            if self.finalizing {
                return Err(DecoderError::Decode(
                    "DTS input arrived after finite finalization began".into(),
                ));
            }
            self.compressed.extend_from_slice(input);
            if self.compressed.len() > MAX_COMPRESSED_BUFFER {
                return Err(DecoderError::Decode(
                    "DTS compressed input exceeded bounded buffer".into(),
                ));
            }
        }
        self.process_stream()?;
        self.take_block(false)
    }

    pub fn poll(&mut self) -> Result<Option<DecodedFrame>, DecoderError> {
        self.process_stream()?;
        self.take_block(false)
    }

    /// Mark the current byte stream as finite. Subsequent empty-input polls may
    /// still return already-decoded PCM and complete staged frames, but once all
    /// valid PCM has drained any remaining DTS sync/header/frame prefix becomes
    /// a hard truncation error instead of disappearing on reset.
    pub fn finish_pending(&mut self) -> Result<(), DecoderError> {
        self.finalizing = true;
        self.process_stream()
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

    fn discard_finite_garbage(&mut self) -> Result<(), DecoderError> {
        if self.compressed.is_empty() {
            return Ok(());
        }
        if let Some(prefix) = trailing_dts_sync_prefix_len(&self.compressed) {
            let garbage = self.compressed.len().saturating_sub(prefix);
            if garbage > 0 {
                self.dropped_bytes = self.dropped_bytes.saturating_add(garbage as u64);
                self.discontinuity = true;
                self.compressed.drain(..garbage);
            }
            return Err(DecoderError::Decode(format!(
                "truncated DTS syncword at end of stream: {prefix} byte(s) buffered"
            )));
        }
        let dropped = self.compressed.len();
        self.compressed.clear();
        self.dropped_bytes = self.dropped_bytes.saturating_add(dropped as u64);
        if dropped > 0 {
            self.discontinuity = true;
        }
        Ok(())
    }

    fn process_stream(&mut self) -> Result<(), DecoderError> {
        loop {
            if self.queued_frames() >= self.queue_high_watermark() {
                break;
            }
            // During finite retirement, publish PCM already decoded from earlier
            // complete frames before diagnosing any later truncated compressed
            // suffix. `take_block` below is allowed to emit a short tail in this
            // state, then the next poll resumes compressed validation.
            if self.finalizing && self.queued_frames() > 0 {
                break;
            }
            if self.compressed.len() < 4 {
                if self.finalizing {
                    self.discard_finite_garbage()?;
                }
                break;
            }

            let Some(sync) = find_next_sync(&self.compressed, 0) else {
                if self.finalizing {
                    self.discard_finite_garbage()?;
                    break;
                }
                // Longest DTS sync prefix is six bytes. Keep a five-byte tail
                // so a prefix split across calls can still be completed.
                let keep = self.compressed.len().min(5);
                let drop = self.compressed.len().saturating_sub(keep);
                if drop > 0 {
                    self.dropped_bytes = self.dropped_bytes.saturating_add(drop as u64);
                    self.discontinuity = true;
                    self.compressed.drain(..drop);
                }
                break;
            };

            if sync.offset > 0 {
                self.dropped_bytes = self.dropped_bytes.saturating_add(sync.offset as u64);
                self.discontinuity = true;
                self.compressed.drain(..sync.offset);
                continue;
            }

            // 18 bytes cover the largest sync/header minimum used by the
            // supported raw and 14-bit forms. Wait for more bytes in streaming
            // mode; at finite EOF the same prefix is provably truncated.
            if self.compressed.len() < 18 {
                if self.finalizing {
                    return Err(DecoderError::Decode(format!(
                        "truncated DTS frame header at end of stream: {} byte(s) buffered",
                        self.compressed.len()
                    )));
                }
                break;
            }

            let header = match sync.encoding {
                SyncWordEncoding::RawBigEndian | SyncWordEncoding::RawLittleEndian => {
                    parse_frame_header(&self.compressed)
                }
                SyncWordEncoding::FourteenBitBigEndian
                | SyncWordEncoding::FourteenBitLittleEndian => {
                    parse_frame_header_14bit(&self.compressed)
                }
                _ => {
                    return Err(DecoderError::UnsupportedInput(
                        "DTS sync encoding is not admitted by Aurora",
                    ));
                }
            };

            let header = match header {
                Ok(header) => header,
                Err(_) => {
                    // False-positive/corrupt sync. Advance by one byte and let
                    // the next iteration resynchronize rather than panic or
                    // trust a malformed frame size.
                    self.dropped_bytes = self.dropped_bytes.saturating_add(1);
                    self.discontinuity = true;
                    self.compressed.drain(..1);
                    continue;
                }
            };

            let frame_len = header.frame_size_container_bytes(sync.encoding) as usize;
            if frame_len == 0 || frame_len > MAX_COMPRESSED_BUFFER {
                return Err(DecoderError::Decode(
                    "DTS frame declared an invalid bounded size".into(),
                ));
            }
            if self.compressed.len() < frame_len {
                if self.finalizing {
                    return Err(DecoderError::Decode(format!(
                        "truncated DTS frame at end of stream: expected {frame_len} bytes, only {} buffered",
                        self.compressed.len()
                    )));
                }
                break;
            }

            let frame: Vec<u8> = self.compressed.drain(..frame_len).collect();
            let (normalized, normalized_header) = normalize_frame(frame, sync.encoding)?;
            self.decode_frame(&normalized, normalized_header)?;
        }
        Ok(())
    }

    fn decode_frame(
        &mut self,
        bytes: &[u8],
        header: DtsFrameHeader,
    ) -> Result<(), DecoderError> {
        let output = self
            .output
            .ok_or(DecoderError::Unavailable("DTS decoder is not configured"))?;
        let sample_rate = header
            .sample_rate_hz()
            .ok_or(DecoderError::UnsupportedInput("DTS sample-rate code is unresolved"))?;
        if sample_rate != output.sample_rate {
            return Err(DecoderError::UnsupportedInput(
                "native DTS sample-rate conversion is not wired yet",
            ));
        }

        let primary_channels = usize::from(
            header
                .channel_count()
                .ok_or(DecoderError::UnsupportedInput("DTS user-defined AMODE is unsupported"))?,
        );
        let restart = self
            .stream
            .as_ref()
            .map(|decoder| decoder.channel_count() != primary_channels)
            .unwrap_or(true);
        if restart {
            self.stream = Some(CoreStreamDecoder::new(primary_channels));
            self.discontinuity = true;
        }

        let stream = self.stream.as_mut().ok_or_else(|| {
            DecoderError::Decode("DTS stream decoder was not initialized".into())
        })?;
        let mut planes = stream
            .decode_frame(bytes, &header)
            .map_err(|error| DecoderError::Decode(format!("DTS Core decode failed: {error}")))?;
        let lfe = stream.take_last_lfe_pcm();
        if header.lfe.is_present() {
            if lfe.is_empty() {
                return Err(DecoderError::Decode(
                    "DTS frame signals LFE but decoder emitted no LFE PCM".into(),
                ));
            }
            planes.push(lfe);
        } else if !lfe.is_empty() {
            return Err(DecoderError::Decode(
                "DTS decoder emitted unexpected LFE PCM".into(),
            ));
        }

        if let Some(pad) = header.termination_pad_samples() {
            for plane in &mut planes {
                plane.extend(std::iter::repeat(0).take(usize::from(pad)));
            }
        }

        let samples = planes.first().map(Vec::len).unwrap_or(0);
        if samples == 0 || planes.iter().any(|plane| plane.len() != samples) {
            return Err(DecoderError::Decode(
                "DTS decoder emitted inconsistent planar PCM geometry".into(),
            ));
        }

        let expected_sources = primary_channels + usize::from(header.lfe.is_present());
        if planes.len() != expected_sources {
            return Err(DecoderError::Decode(format!(
                "DTS source channel mismatch: expected {expected_sources}, decoded {}",
                planes.len()
            )));
        }

        let arrangement = header.amode_arrangement();
        let map = dts_target_map(
            arrangement,
            header.lfe.is_present(),
            planes.len(),
            output.channel_count,
        )?;
        let bits = header.source_pcm_bits_per_sample().ok_or(
            DecoderError::UnsupportedInput("DTS source PCM resolution is reserved"),
        )?;
        let full_scale = 2.0_f32.powi(i32::from(bits) - 1);
        if !full_scale.is_finite() || full_scale <= 0.0 {
            return Err(DecoderError::Decode(
                "DTS PCM normalization scale is invalid".into(),
            ));
        }

        self.ensure_pcm_channels();
        for frame_index in 0..samples {
            for (target, source) in map.iter().enumerate() {
                let value = source
                    .map(|source| planes[source][frame_index] as f32 / full_scale)
                    .unwrap_or(0.0);
                self.pcm[target].push_back(value);
            }
        }
        Ok(())
    }

    fn take_block(&mut self, allow_short: bool) -> Result<Option<DecodedFrame>, DecoderError> {
        let output = self
            .output
            .ok_or(DecoderError::Unavailable("DTS decoder is not configured"))?;
        if self.pcm.is_empty() {
            return Ok(None);
        }
        let available = self.queued_frames();
        let wanted = output.block_size.max(1);
        // While compressed input is still staged, retain a sub-block PCM tail so
        // the next complete DTS frame can continue the preferred block cadence.
        // Finite retirement is different: all already-decoded PCM is known-valid
        // audio and must be emitted before a later truncated suffix is reported.
        if available < wanted
            && !allow_short
            && !self.compressed.is_empty()
            && !self.finalizing
        {
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

impl Default for NativeDtsDecoder {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_frame(
    mut frame: Vec<u8>,
    encoding: SyncWordEncoding,
) -> Result<(Vec<u8>, DtsFrameHeader), DecoderError> {
    match encoding {
        SyncWordEncoding::RawBigEndian => {
            let header = parse_frame_header(&frame)
                .map_err(|error| DecoderError::Decode(format!("DTS header rejected: {error}")))?;
            Ok((frame, header))
        }
        SyncWordEncoding::RawLittleEndian => {
            for pair in frame.chunks_exact_mut(2) {
                pair.swap(0, 1);
            }
            let header = parse_frame_header(&frame).map_err(|error| {
                DecoderError::Decode(format!("DTS LE normalization failed: {error}"))
            })?;
            Ok((frame, header))
        }
        SyncWordEncoding::FourteenBitBigEndian | SyncWordEncoding::FourteenBitLittleEndian => {
            let order = FourteenBitByteOrder::from_sync(encoding).ok_or(
                DecoderError::UnsupportedInput("DTS 14-bit byte order could not be resolved"),
            )?;
            let unpacked = unpack_14bit_to_16bit(&frame, order).map_err(|error| {
                DecoderError::Decode(format!("DTS 14-bit unpack failed: {error}"))
            })?;
            let header = parse_frame_header(&unpacked).map_err(|error| {
                DecoderError::Decode(format!("DTS unpacked header rejected: {error}"))
            })?;
            Ok((unpacked, header))
        }
        _ => Err(DecoderError::UnsupportedInput(
            "DTS sync encoding is not admitted by Aurora",
        )),
    }
}

/// Build one source index per Aurora target channel. `None` means silence.
///
/// The decoder only admits arrangements whose DTS Table 5-4 lane semantics can
/// be mapped unambiguously into Aurora's canonical cinema order:
/// `FL FR C LFE SL SR BL BR ...`. Ambiguous dual-mono, mono-surround and
/// extended arrangements fail closed rather than being guessed.
fn dts_target_map(
    arrangement: AmodeArrangement,
    has_lfe: bool,
    source_channels: usize,
    output_channels: usize,
) -> Result<Vec<Option<usize>>, DecoderError> {
    let expected = arrangement
        .channel_count()
        .map(usize::from)
        .ok_or(DecoderError::UnsupportedInput("DTS user-defined AMODE is unsupported"))?
        + usize::from(has_lfe);
    if source_channels != expected {
        return Err(DecoderError::Decode(
            "DTS mapping source geometry does not match AMODE".into(),
        ));
    }

    let mut map = vec![None; output_channels];
    let mut assign = |target: usize, source: usize| -> Result<(), DecoderError> {
        if target >= output_channels || source >= source_channels {
            return Err(DecoderError::UnsupportedInput(
                "configured Aurora output cannot represent DTS layout",
            ));
        }
        map[target] = Some(source);
        Ok(())
    };

    let lfe_source = source_channels.checked_sub(1).filter(|_| has_lfe);
    match arrangement {
        AmodeArrangement::Mono => {
            if output_channels >= 3 {
                assign(2, 0)?;
            } else {
                assign(0, 0)?;
            }
        }
        AmodeArrangement::Stereo | AmodeArrangement::SumDifference | AmodeArrangement::LtRt => {
            assign(0, 0)?;
            assign(1, 1)?;
        }
        AmodeArrangement::ClR => {
            // DTS bitstream order: C, L, R.
            assign(2, 0)?;
            assign(0, 1)?;
            assign(1, 2)?;
        }
        AmodeArrangement::LrSlSr => {
            // DTS bitstream order: L, R, SL, SR.
            assign(0, 0)?;
            assign(1, 1)?;
            assign(4, 2)?;
            assign(5, 3)?;
        }
        AmodeArrangement::ClRSlSr => {
            // DTS bitstream order: C, L, R, SL, SR.
            assign(2, 0)?;
            assign(0, 1)?;
            assign(1, 2)?;
            assign(4, 3)?;
            assign(5, 4)?;
        }
        _ => {
            return Err(DecoderError::UnsupportedInput(
                "DTS AMODE has no accepted Aurora channel-order mapping yet",
            ));
        }
    }

    if let Some(source) = lfe_source {
        assign(3, source)?;
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::SampleType;

    fn format(block_size: usize) -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: 12,
            sample_type: SampleType::F32,
            block_size,
        }
    }

    #[test]
    fn recognizes_all_four_core_sync_forms() {
        assert!(looks_like_dts_sync(&[0x7F, 0xFE, 0x80, 0x01]));
        assert!(looks_like_dts_sync(&[0xFE, 0x7F, 0x01, 0x80]));
        assert!(looks_like_dts_sync(&[0x1F, 0xFF, 0xE8, 0x00]));
        assert!(looks_like_dts_sync(&[0xFF, 0x1F, 0x00, 0xE8]));
        assert!(!looks_like_dts_sync(&[0x64, 0x58, 0x20, 0x25]));
    }

    #[test]
    fn five_one_maps_to_aurora_canonical_order() {
        let map = dts_target_map(AmodeArrangement::ClRSlSr, true, 6, 12).unwrap();
        assert_eq!(map[0], Some(1));
        assert_eq!(map[1], Some(2));
        assert_eq!(map[2], Some(0));
        assert_eq!(map[3], Some(5));
        assert_eq!(map[4], Some(3));
        assert_eq!(map[5], Some(4));
        assert!(map[6..].iter().all(Option::is_none));
    }

    #[test]
    fn ambiguous_dual_mono_fails_closed() {
        let error = dts_target_map(AmodeArrangement::DualMono, false, 2, 12).unwrap_err();
        assert!(matches!(error, DecoderError::UnsupportedInput(_)));
    }

    #[test]
    fn adapter_accepts_aurora_40_frame_configuration() {
        let mut decoder = NativeDtsDecoder::new();
        decoder.configure(format(40));
        assert!(decoder.poll().unwrap().is_none());
    }

    #[test]
    fn complete_dts_input_releases_short_pcm_tail() {
        let mut decoder = NativeDtsDecoder::new();
        decoder.configure(format(40));
        for queue in &mut decoder.pcm {
            queue.extend(std::iter::repeat(0.25_f32).take(32));
        }
        assert!(decoder.compressed.is_empty());

        let tail = decoder.take_block(false).unwrap().unwrap();

        assert_eq!(tail.audio.frame_count, 32);
        assert_eq!(tail.audio.channels.len(), 12);
        assert!(tail.audio.channels.iter().all(|channel| channel.len() == 32));
        assert_eq!(decoder.queued_frames(), 0);
    }

    #[test]
    fn partial_compressed_dts_input_holds_short_pcm_tail() {
        let mut decoder = NativeDtsDecoder::new();
        decoder.configure(format(40));
        for queue in &mut decoder.pcm {
            queue.extend(std::iter::repeat(0.25_f32).take(32));
        }
        decoder.compressed.push(0x7F);

        assert!(decoder.take_block(false).unwrap().is_none());
        assert_eq!(decoder.queued_frames(), 32);
    }

    #[test]
    fn finite_dts_retirement_emits_valid_pcm_before_truncation_error() {
        let mut decoder = NativeDtsDecoder::new();
        decoder.configure(format(40));
        for queue in &mut decoder.pcm {
            queue.extend(std::iter::repeat(0.25_f32).take(32));
        }
        decoder.compressed.extend_from_slice(&[0x7F, 0xFE]);

        decoder.finish_pending().unwrap();
        let tail = decoder.poll().unwrap().expect("valid decoded tail must retire first");
        assert_eq!(tail.audio.frame_count, 32);
        let error = decoder.poll().unwrap_err();
        assert!(error.to_string().contains("truncated DTS syncword"));
    }

    #[test]
    fn finite_dts_eof_drops_non_sync_garbage() {
        let mut decoder = NativeDtsDecoder::new();
        decoder.configure(format(40));
        decoder.compressed.extend_from_slice(&[1, 2, 3]);

        decoder.finish_pending().unwrap();

        assert!(decoder.compressed.is_empty());
        assert_eq!(decoder.dropped_bytes(), 3);
    }
}
