//! Persistent open-source FFmpeg worker backend for codecs that are not yet
//! implemented natively inside Aurora.
//!
//! The worker is deliberately a replaceable compatibility backend. Aurora owns
//! transport detection, output timing, block sizing and the PCM/object contract;
//! FFmpeg only performs codec decode/resample for open codec families.

use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};

use aurora_core::{AudioBlock, AudioFormat};
use aurora_decoder_api::{DecodedFrame, DecoderError};

use crate::sniff::{CodecKind, Encapsulation};

const MAX_RECYCLED_PLANAR_BLOCKS: usize = 32;
const MAX_RECYCLED_PLANAR_FRAMES: usize = 2_048;
const MAX_WORKER_CHANNELS: usize = 12;
const MAX_WAV_HEADER_BYTES: usize = 64 * 1024;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerCommand {
    pub program: String,
    pub args: Vec<String>,
    /// Zero means channel count is intentionally negotiated from FFmpeg's WAV
    /// output header instead of being forced with `-ac`.
    pub decoded_channels: usize,
    /// Empty for the same reason: the semantic map is derived from the WAV
    /// WAVE_FORMAT_EXTENSIBLE channel mask after FFmpeg has decoded the source.
    pub channel_map: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkerPcmContract {
    decoded_channels: usize,
    /// Source-channel index -> Aurora canonical target-channel index.
    channel_map: [usize; MAX_WORKER_CHANNELS],
}

/// Build the deterministic FFmpeg command used by the persistent worker.
///
/// The worker deliberately does not pass `-ac` or `-channel_layout`. Forcing a
/// 12-channel Aurora output into FFmpeg 7.1 would silently rematrix stereo/5.1
/// sources before Aurora sees them. Instead FFmpeg preserves its decoded source
/// layout and emits self-describing F32 WAV. Aurora validates the WAV channel
/// mask and performs only an explicit semantic lane permutation/zero-extension.
pub fn build_worker_command(
    codec: CodecKind,
    encapsulation: Encapsulation,
    output: AudioFormat,
) -> Result<WorkerCommand, DecoderError> {
    if output.sample_rate == 0 || output.channel_count == 0 || output.channel_count > MAX_WORKER_CHANNELS {
        return Err(DecoderError::UnsupportedInput(
            "FFmpeg worker requires a non-zero output rate and at most twelve Aurora channels",
        ));
    }

    let mut args = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-nostdin".into(),
        "-fflags".into(),
        "+discardcorrupt".into(),
        "-probesize".into(),
        "65536".into(),
        "-analyzeduration".into(),
        "0".into(),
    ];

    if matches!(encapsulation, Encapsulation::Elementary | Encapsulation::Iec61937) {
        if let Some(format) = ffmpeg_input_format(codec) {
            args.push("-f".into());
            args.push(format.into());
        }
    }
    args.extend([
        "-i".into(),
        "pipe:0".into(),
        "-map".into(),
        "0:a:0".into(),
        "-map_metadata".into(),
        "-1".into(),
        "-vn".into(),
        "-sn".into(),
        "-dn".into(),
        "-acodec".into(),
        "pcm_f32le".into(),
        "-ar".into(),
        output.sample_rate.to_string(),
        "-f".into(),
        "wav".into(),
        "-rf64".into(),
        "never".into(),
        "pipe:1".into(),
    ]);

    Ok(WorkerCommand {
        program: "ffmpeg".into(),
        args,
        decoded_channels: 0,
        channel_map: Vec::new(),
    })
}

pub const fn ffmpeg_input_format(codec: CodecKind) -> Option<&'static str> {
    match codec {
        CodecKind::TrueHd => Some("truehd"),
        CodecKind::Mlp => Some("mlp"),
        CodecKind::Dts | CodecKind::DtsHd => Some("dts"),
        CodecKind::AacAdts => Some("aac"),
        CodecKind::AacLatm => Some("loas"),
        CodecKind::Flac => Some("flac"),
        CodecKind::Mp3 => Some("mp3"),
        CodecKind::WavPack => Some("wv"),
        CodecKind::MonkeyAudio => Some("ape"),
        CodecKind::Tta => Some("tta"),
        CodecKind::AmrNb | CodecKind::AmrWb => Some("amr"),
        CodecKind::Opus
        | CodecKind::Vorbis
        | CodecKind::Speex
        | CodecKind::Alac
        | CodecKind::Musepack
        | CodecKind::Sbc
        | CodecKind::OggUnknown
        | CodecKind::Pcm
        | CodecKind::Ac3
        | CodecKind::Eac3
        | CodecKind::Eac3Joc
        | CodecKind::DolbyMat
        | CodecKind::Unknown => None,
    }
}

fn decode_worker_sample(bytes: [u8; 4]) -> Result<f32, DecoderError> {
    let sample = f32::from_le_bytes(bytes);
    if !sample.is_finite() {
        return Err(DecoderError::Decode(
            "FFmpeg worker returned non-finite PCM; refusing to sanitize corrupted decoder output"
                .to_owned(),
        ));
    }
    Ok(sample)
}

fn wav_mask_target(bit: u32) -> Option<usize> {
    match bit {
        0x0000_0001 => Some(0),  // FL
        0x0000_0002 => Some(1),  // FR
        0x0000_0004 => Some(2),  // FC
        0x0000_0008 => Some(3),  // LFE
        0x0000_0200 => Some(4),  // SL
        0x0000_0400 => Some(5),  // SR
        0x0000_0010 => Some(6),  // BL
        0x0000_0020 => Some(7),  // BR
        0x0000_1000 => Some(8),  // TFL
        0x0000_4000 => Some(9),  // TFR
        0x0000_8000 => Some(10), // TBL
        0x0002_0000 => Some(11), // TBR
        _ => None,
    }
}

fn wav_channel_contract(
    decoded_channels: usize,
    channel_mask: u32,
    output_channels: usize,
) -> Result<WorkerPcmContract, DecoderError> {
    if decoded_channels == 0
        || decoded_channels > MAX_WORKER_CHANNELS
        || decoded_channels > output_channels
    {
        return Err(DecoderError::UnsupportedInput(
            "FFmpeg WAV channel count cannot be represented by Aurora output",
        ));
    }

    let mut map = [usize::MAX; MAX_WORKER_CHANNELS];
    if channel_mask == 0 {
        match decoded_channels {
            1 => {
                map[0] = if output_channels >= 3 { 2 } else { 0 };
                return Ok(WorkerPcmContract {
                    decoded_channels,
                    channel_map: map,
                });
            }
            2 => {
                map[0] = 0;
                map[1] = 1;
                return Ok(WorkerPcmContract {
                    decoded_channels,
                    channel_map: map,
                });
            }
            _ => {
                return Err(DecoderError::UnsupportedInput(
                    "multichannel FFmpeg WAV output has no channel mask; refusing ambiguous speaker order",
                ));
            }
        }
    }

    if channel_mask.count_ones() as usize != decoded_channels {
        return Err(DecoderError::UnsupportedInput(
            "FFmpeg WAV channel mask does not match channel count",
        ));
    }

    let mut source = 0usize;
    for bit_index in 0..32 {
        let bit = 1u32 << bit_index;
        if channel_mask & bit == 0 {
            continue;
        }
        let target = wav_mask_target(bit).ok_or(DecoderError::UnsupportedInput(
            "FFmpeg WAV channel mask contains a speaker role without an Aurora mapping",
        ))?;
        if target >= output_channels || source >= MAX_WORKER_CHANNELS {
            return Err(DecoderError::UnsupportedInput(
                "FFmpeg WAV speaker layout exceeds configured Aurora output",
            ));
        }
        map[source] = target;
        source += 1;
    }

    if source != decoded_channels {
        return Err(DecoderError::UnsupportedInput(
            "FFmpeg WAV channel mask could not be resolved completely",
        ));
    }

    Ok(WorkerPcmContract {
        decoded_channels,
        channel_map: map,
    })
}

fn parse_wav_fmt_chunk(
    fmt: &[u8],
    output: AudioFormat,
) -> Result<WorkerPcmContract, DecoderError> {
    if fmt.len() < 16 {
        return Err(DecoderError::Decode(
            "FFmpeg WAV fmt chunk is truncated".to_owned(),
        ));
    }
    let format_tag = u16::from_le_bytes([fmt[0], fmt[1]]);
    let decoded_channels = usize::from(u16::from_le_bytes([fmt[2], fmt[3]]));
    let sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
    let block_align = usize::from(u16::from_le_bytes([fmt[12], fmt[13]]));
    let bits_per_sample = u16::from_le_bytes([fmt[14], fmt[15]]);

    if sample_rate != output.sample_rate || bits_per_sample != 32 {
        return Err(DecoderError::UnsupportedInput(
            "FFmpeg WAV output rate or sample width drifted from F32 Aurora contract",
        ));
    }
    let expected_align = decoded_channels
        .checked_mul(std::mem::size_of::<f32>())
        .ok_or(DecoderError::UnsupportedInput("worker frame size overflow"))?;
    if expected_align == 0 || block_align != expected_align {
        return Err(DecoderError::Decode(
            "FFmpeg WAV block alignment does not match F32 channel geometry".to_owned(),
        ));
    }

    let channel_mask = match format_tag {
        WAVE_FORMAT_IEEE_FLOAT => {
            if decoded_channels > 2 {
                return Err(DecoderError::UnsupportedInput(
                    "multichannel FFmpeg WAV output omitted WAVE_FORMAT_EXTENSIBLE channel semantics",
                ));
            }
            0
        }
        WAVE_FORMAT_EXTENSIBLE => {
            if fmt.len() < 40 {
                return Err(DecoderError::Decode(
                    "FFmpeg WAVE_FORMAT_EXTENSIBLE fmt chunk is truncated".to_owned(),
                ));
            }
            let extension_size = u16::from_le_bytes([fmt[16], fmt[17]]);
            let valid_bits = u16::from_le_bytes([fmt[18], fmt[19]]);
            let mask = u32::from_le_bytes([fmt[20], fmt[21], fmt[22], fmt[23]]);
            let subformat = u32::from_le_bytes([fmt[24], fmt[25], fmt[26], fmt[27]]);
            if extension_size < 22 || valid_bits != 32 || subformat != u32::from(WAVE_FORMAT_IEEE_FLOAT) {
                return Err(DecoderError::UnsupportedInput(
                    "FFmpeg WAV extensible format is not 32-bit IEEE float",
                ));
            }
            mask
        }
        _ => {
            return Err(DecoderError::UnsupportedInput(
                "FFmpeg worker emitted a WAV sample format other than IEEE F32",
            ));
        }
    };

    wav_channel_contract(decoded_channels, channel_mask, output.channel_count)
}

fn parse_wav_stream_header(
    bytes: &[u8],
    output: AudioFormat,
) -> Result<Option<(usize, WorkerPcmContract)>, DecoderError> {
    if bytes.len() < 12 {
        return Ok(None);
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(DecoderError::Decode(
            "FFmpeg worker stdout is not a RIFF/WAVE stream".to_owned(),
        ));
    }

    let mut cursor = 12usize;
    let mut contract = None;
    loop {
        if cursor > MAX_WAV_HEADER_BYTES {
            return Err(DecoderError::Decode(
                "FFmpeg WAV header exceeded bounded size".to_owned(),
            ));
        }
        let header_end = cursor
            .checked_add(8)
            .ok_or_else(|| DecoderError::Decode("FFmpeg WAV chunk offset overflow".to_owned()))?;
        if bytes.len() < header_end {
            return Ok(None);
        }
        let id = &bytes[cursor..cursor + 4];
        let size = u32::from_le_bytes([
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]) as usize;
        let payload_start = header_end;

        if id == b"data" {
            let contract = contract.ok_or_else(|| {
                DecoderError::Decode("FFmpeg WAV data chunk arrived before fmt chunk".to_owned())
            })?;
            if payload_start > MAX_WAV_HEADER_BYTES {
                return Err(DecoderError::Decode(
                    "FFmpeg WAV header exceeded bounded size".to_owned(),
                ));
            }
            return Ok(Some((payload_start, contract)));
        }

        let padded_size = size
            .checked_add(size & 1)
            .ok_or_else(|| DecoderError::Decode("FFmpeg WAV chunk size overflow".to_owned()))?;
        let payload_end = payload_start
            .checked_add(padded_size)
            .ok_or_else(|| DecoderError::Decode("FFmpeg WAV chunk size overflow".to_owned()))?;
        if payload_end > MAX_WAV_HEADER_BYTES {
            return Err(DecoderError::Decode(
                "FFmpeg WAV header exceeded bounded size".to_owned(),
            ));
        }
        if bytes.len() < payload_end {
            return Ok(None);
        }

        if id == b"fmt " {
            if contract.is_some() {
                return Err(DecoderError::Decode(
                    "FFmpeg WAV stream contains multiple fmt chunks".to_owned(),
                ));
            }
            contract = Some(parse_wav_fmt_chunk(
                &bytes[payload_start..payload_start + size],
                output,
            )?);
        }
        cursor = payload_end;
    }
}

pub struct OpenWorkerDecoder {
    codec: CodecKind,
    encapsulation: Encapsulation,
    output: AudioFormat,
    pcm_contract: Option<WorkerPcmContract>,
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<io::Result<()>>>,
    pcm_bytes: Vec<u8>,
    recycled_planar: Vec<Vec<Vec<f32>>>,
    emitted_frames: u64,
    discontinuity: bool,
}

impl OpenWorkerDecoder {
    pub fn spawn(
        codec: CodecKind,
        encapsulation: Encapsulation,
        output: AudioFormat,
    ) -> Result<Self, DecoderError> {
        let command = build_worker_command(codec, encapsulation, output)?;
        let mut child = Command::new(&command.program)
            .args(&command.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                DecoderError::Unavailable(
                    "FFmpeg open-worker executable is not installed or not executable",
                )
            })?;
        let stdin = child.stdin.take().ok_or(DecoderError::Unavailable(
            "FFmpeg worker stdin pipe unavailable",
        ))?;
        let mut stdout = child.stdout.take().ok_or(DecoderError::Unavailable(
            "FFmpeg worker stdout pipe unavailable",
        ))?;
        let (tx, rx) = mpsc::channel();
        let reader = thread::Builder::new()
            .name("aurora-open-decoder-ffmpeg".into())
            .spawn(move || -> io::Result<()> {
                let mut chunk = vec![0_u8; 32 * 1024];
                loop {
                    match stdout.read(&mut chunk) {
                        Ok(0) => return Ok(()),
                        Ok(count) => {
                            if tx.send(chunk[..count].to_vec()).is_err() {
                                return Ok(());
                            }
                        }
                        Err(error) => return Err(error),
                    }
                }
            })
            .map_err(|_| DecoderError::Unavailable("failed to start FFmpeg stdout reader"))?;

        Ok(Self {
            codec,
            encapsulation,
            output,
            pcm_contract: None,
            child,
            stdin: Some(stdin),
            rx,
            reader: Some(reader),
            pcm_bytes: Vec::new(),
            recycled_planar: Vec::with_capacity(MAX_RECYCLED_PLANAR_BLOCKS),
            emitted_frames: 0,
            discontinuity: true,
        })
    }

    pub fn codec(&self) -> CodecKind {
        self.codec
    }

    pub fn encapsulation(&self) -> Encapsulation {
        self.encapsulation
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        if !bytes.is_empty() {
            let stdin = self.stdin.as_mut().ok_or(DecoderError::ExternalProcess(
                "FFmpeg worker input already closed".into(),
            ))?;
            stdin
                .write_all(bytes)
                .and_then(|_| stdin.flush())
                .map_err(|e| {
                    DecoderError::ExternalProcess(format!("FFmpeg worker input write failed: {e}"))
                })?;
        }
        self.collect_stdout();
        self.take_block(false)
    }

    pub fn poll(&mut self) -> Result<Option<DecodedFrame>, DecoderError> {
        self.collect_stdout();
        self.take_block(false)
    }

    pub fn finish(&mut self) -> Result<Vec<DecodedFrame>, DecoderError> {
        self.stdin.take();
        let status = self
            .child
            .wait()
            .map_err(|e| DecoderError::ExternalProcess(format!("FFmpeg wait failed: {e}")))?;
        if let Some(reader) = self.reader.take() {
            match reader.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    return Err(DecoderError::ExternalProcess(format!(
                        "FFmpeg stdout reader failed: {error}"
                    )));
                }
                Err(_) => {
                    return Err(DecoderError::ExternalProcess(
                        "FFmpeg stdout reader thread panicked".to_owned(),
                    ));
                }
            }
        }
        self.collect_stdout();
        if !status.success() {
            return Err(DecoderError::ExternalProcess(format!(
                "FFmpeg worker exited with status {status}"
            )));
        }
        let mut frames = Vec::new();
        while let Some(frame) = self.take_block(true)? {
            frames.push(frame);
        }
        if !self.pcm_bytes.is_empty() {
            let reason = if self.pcm_contract.is_none() {
                "an incomplete WAV header"
            } else {
                "bytes that do not form a complete PCM frame"
            };
            return Err(DecoderError::Decode(format!(
                "FFmpeg worker ended with {} trailing byte(s): {reason}",
                self.pcm_bytes.len()
            )));
        }
        Ok(frames)
    }

    /// Return one consumed worker PCM frame to a bounded planar storage pool.
    pub fn recycle_frame(&mut self, frame: DecodedFrame) {
        if !frame.objects.is_empty()
            || frame.audio.channels.len() != self.output.channel_count
            || frame.audio.frame_count > MAX_RECYCLED_PLANAR_FRAMES
            || frame
                .audio
                .channels
                .iter()
                .any(|channel| channel.len() != frame.audio.frame_count)
        {
            return;
        }
        self.recycle_planar_storage(frame.audio.channels);
    }

    fn collect_stdout(&mut self) {
        while let Ok(bytes) = self.rx.try_recv() {
            self.pcm_bytes.extend_from_slice(&bytes);
        }
    }

    fn ensure_pcm_contract(&mut self) -> Result<bool, DecoderError> {
        if self.pcm_contract.is_some() {
            return Ok(true);
        }
        let Some((header_bytes, contract)) = parse_wav_stream_header(&self.pcm_bytes, self.output)?
        else {
            return Ok(false);
        };
        self.pcm_bytes.drain(..header_bytes);
        self.pcm_contract = Some(contract);
        Ok(true)
    }

    fn take_planar_storage(&mut self, frame_count: usize) -> Vec<Vec<f32>> {
        if let Some(index) = self.recycled_planar.iter().position(|planar| {
            planar.len() == self.output.channel_count
                && planar
                    .iter()
                    .all(|channel| channel.capacity() >= frame_count)
        }) {
            let mut planar = self.recycled_planar.swap_remove(index);
            for channel in &mut planar {
                channel.clear();
                channel.resize(frame_count, 0.0);
            }
            return planar;
        }
        (0..self.output.channel_count)
            .map(|_| vec![0.0_f32; frame_count])
            .collect()
    }

    fn recycle_planar_storage(&mut self, mut planar: Vec<Vec<f32>>) {
        if planar.len() != self.output.channel_count
            || self.recycled_planar.len() >= MAX_RECYCLED_PLANAR_BLOCKS
            || planar
                .iter()
                .any(|channel| channel.capacity() > MAX_RECYCLED_PLANAR_FRAMES)
        {
            return;
        }
        for channel in &mut planar {
            channel.clear();
        }
        self.recycled_planar.push(planar);
    }

    fn take_block(&mut self, allow_short: bool) -> Result<Option<DecodedFrame>, DecoderError> {
        if !self.ensure_pcm_contract()? {
            return Ok(None);
        }
        let contract = self
            .pcm_contract
            .expect("PCM contract was established immediately above");
        let bytes_per_frame = contract
            .decoded_channels
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or(DecoderError::UnsupportedInput("worker frame size overflow"))?;
        let wanted_frames = self.output.block_size.max(1);
        let wanted_bytes = wanted_frames
            .checked_mul(bytes_per_frame)
            .ok_or(DecoderError::UnsupportedInput("worker block size overflow"))?;
        if self.pcm_bytes.len() < wanted_bytes && !allow_short {
            return Ok(None);
        }
        let available_frames = self.pcm_bytes.len() / bytes_per_frame;
        if available_frames == 0 {
            return Ok(None);
        }
        let frame_count = available_frames.min(wanted_frames);
        let take = frame_count * bytes_per_frame;
        let mut planar = self.take_planar_storage(frame_count);
        for frame in 0..frame_count {
            let base = frame * bytes_per_frame;
            for source_channel in 0..contract.decoded_channels {
                let at = base + source_channel * 4;
                let sample = decode_worker_sample([
                    self.pcm_bytes[at],
                    self.pcm_bytes[at + 1],
                    self.pcm_bytes[at + 2],
                    self.pcm_bytes[at + 3],
                ])?;
                let target_channel = contract.channel_map[source_channel];
                planar[target_channel][frame] = sample;
            }
        }
        self.pcm_bytes.drain(..take);
        let pts = self.emitted_frames as f64 / f64::from(self.output.sample_rate);
        self.emitted_frames = self.emitted_frames.saturating_add(frame_count as u64);
        let discontinuity = std::mem::replace(&mut self.discontinuity, false);
        Ok(Some(DecodedFrame {
            audio: AudioBlock {
                channels: planar,
                frame_count,
                presentation_time_seconds: pts,
                discontinuity,
            },
            objects: Vec::new(),
        }))
    }
}

impl Drop for OpenWorkerDecoder {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::SampleType;

    fn fmt(channels: usize) -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: channels,
            sample_type: SampleType::F32,
            block_size: 40,
        }
    }

    fn wav_header(channels: u16, mask: Option<u32>) -> Vec<u8> {
        let mut fmt = Vec::new();
        let block_align = channels * 4;
        let byte_rate = 48_000u32 * u32::from(block_align);
        if let Some(mask) = mask {
            fmt.extend_from_slice(&WAVE_FORMAT_EXTENSIBLE.to_le_bytes());
            fmt.extend_from_slice(&channels.to_le_bytes());
            fmt.extend_from_slice(&48_000u32.to_le_bytes());
            fmt.extend_from_slice(&byte_rate.to_le_bytes());
            fmt.extend_from_slice(&block_align.to_le_bytes());
            fmt.extend_from_slice(&32u16.to_le_bytes());
            fmt.extend_from_slice(&22u16.to_le_bytes());
            fmt.extend_from_slice(&32u16.to_le_bytes());
            fmt.extend_from_slice(&mask.to_le_bytes());
            fmt.extend_from_slice(&3u32.to_le_bytes());
            fmt.extend_from_slice(&0x0010u16.to_le_bytes());
            fmt.extend_from_slice(&0x0000u16.to_le_bytes());
            fmt.extend_from_slice(&[0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71]);
        } else {
            fmt.extend_from_slice(&WAVE_FORMAT_IEEE_FLOAT.to_le_bytes());
            fmt.extend_from_slice(&channels.to_le_bytes());
            fmt.extend_from_slice(&48_000u32.to_le_bytes());
            fmt.extend_from_slice(&byte_rate.to_le_bytes());
            fmt.extend_from_slice(&block_align.to_le_bytes());
            fmt.extend_from_slice(&32u16.to_le_bytes());
            fmt.extend_from_slice(&0u16.to_le_bytes());
        }

        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&u32::MAX.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        wav.extend_from_slice(&fmt);
        if fmt.len() & 1 != 0 {
            wav.push(0);
        }
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&u32::MAX.to_le_bytes());
        wav
    }

    #[test]
    fn worker_command_preserves_source_channel_layout() {
        let cmd =
            build_worker_command(CodecKind::TrueHd, Encapsulation::Elementary, fmt(12)).unwrap();
        assert!(cmd.args.windows(2).any(|p| p == ["-f", "truehd"]));
        assert!(cmd.args.windows(2).any(|p| p == ["-f", "wav"]));
        assert!(cmd.args.windows(2).any(|p| p == ["-rf64", "never"]));
        assert!(cmd.args.windows(2).any(|p| p == ["-map_metadata", "-1"]));
        assert!(!cmd.args.iter().any(|arg| arg == "-ac"));
        assert!(!cmd.args.iter().any(|arg| arg == "-channel_layout"));
        assert_eq!(cmd.decoded_channels, 0);
        assert!(cmd.channel_map.is_empty());
    }

    #[test]
    fn wav_side_five_one_maps_directly_to_aurora_surrounds() {
        let wav = wav_header(6, Some(0x0000_060F));
        let (_, contract) = parse_wav_stream_header(&wav, fmt(12)).unwrap().unwrap();
        assert_eq!(contract.decoded_channels, 6);
        assert_eq!(&contract.channel_map[..6], &[0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn wav_back_five_one_maps_to_aurora_back_channels() {
        let wav = wav_header(6, Some(0x0000_003F));
        let (_, contract) = parse_wav_stream_header(&wav, fmt(12)).unwrap().unwrap();
        assert_eq!(&contract.channel_map[..6], &[0, 1, 2, 3, 6, 7]);
    }

    #[test]
    fn wav_seven_one_reorders_back_then_side_into_aurora_order() {
        let wav = wav_header(8, Some(0x0000_063F));
        let (_, contract) = parse_wav_stream_header(&wav, fmt(12)).unwrap().unwrap();
        assert_eq!(
            &contract.channel_map[..8],
            &[0, 1, 2, 3, 6, 7, 4, 5]
        );
    }

    #[test]
    fn wav_plain_mono_and_stereo_have_unambiguous_semantics() {
        let mono = wav_header(1, None);
        let (_, mono_contract) = parse_wav_stream_header(&mono, fmt(12)).unwrap().unwrap();
        assert_eq!(mono_contract.channel_map[0], 2);

        let stereo = wav_header(2, None);
        let (_, stereo_contract) = parse_wav_stream_header(&stereo, fmt(12)).unwrap().unwrap();
        assert_eq!(&stereo_contract.channel_map[..2], &[0, 1]);
    }

    #[test]
    fn multichannel_wav_without_mask_fails_closed() {
        let wav = wav_header(6, None);
        assert!(matches!(
            parse_wav_stream_header(&wav, fmt(12)),
            Err(DecoderError::UnsupportedInput(_))
        ));
    }

    #[test]
    fn wav_unknown_speaker_role_fails_closed() {
        // FL + FR + front-left-of-center. Aurora has no semantic output role for
        // FLC, so accepting this by index would be an incorrect speaker mapping.
        let wav = wav_header(3, Some(0x0000_0043));
        assert!(matches!(
            parse_wav_stream_header(&wav, fmt(12)),
            Err(DecoderError::UnsupportedInput(_))
        ));
    }

    #[test]
    fn fragmented_wav_header_waits_for_more_bytes() {
        let wav = wav_header(8, Some(0x0000_063F));
        assert_eq!(parse_wav_stream_header(&wav[..20], fmt(12)).unwrap(), None);
    }

    #[test]
    fn ogg_opus_keeps_container_probe() {
        let cmd = build_worker_command(CodecKind::Opus, Encapsulation::Ogg, fmt(12)).unwrap();
        assert!(!cmd.args.windows(2).any(|p| p == ["-f", "opus"]));
    }

    #[test]
    fn dts_hd_uses_dts_demuxer() {
        assert_eq!(ffmpeg_input_format(CodecKind::DtsHd), Some("dts"));
    }

    #[test]
    fn worker_pcm_rejects_non_finite_samples_instead_of_silencing_them() {
        assert_eq!(decode_worker_sample(0.25_f32.to_le_bytes()).unwrap(), 0.25);
        for sample in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let error = decode_worker_sample(sample.to_le_bytes()).unwrap_err();
            assert!(matches!(error, DecoderError::Decode(_)));
        }
    }
}
