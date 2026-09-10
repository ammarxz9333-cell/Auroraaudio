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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerCommand {
    pub program: String,
    pub args: Vec<String>,
    pub decoded_channels: usize,
    /// Source-channel index -> Aurora canonical target-channel index.
    pub channel_map: Vec<usize>,
}

/// Resolve one deterministic FFmpeg raw-PCM layout and its semantic mapping to
/// Aurora's canonical speaker order.
///
/// FFmpeg native 7.1 order is `FL FR FC LFE BL BR SL SR`, while Aurora's first
/// eight canonical lanes are `FL FR FC LFE SL SR SBL SBR`. The worker must
/// therefore remap the last four lanes instead of treating raw channel indices
/// as speaker semantics. Unsupported widths fail closed rather than guessing.
fn worker_channel_contract(
    decoded_channels: usize,
    output_channels: usize,
) -> Result<(&'static str, Vec<usize>), DecoderError> {
    let contract = match decoded_channels {
        1 if output_channels >= 1 => ("mono", vec![0]),
        2 if output_channels >= 2 => ("stereo", vec![0, 1]),
        6 if output_channels >= 6 => ("5.1", vec![0, 1, 2, 3, 4, 5]),
        8 if output_channels >= 8 => ("7.1", vec![0, 1, 2, 3, 6, 7, 4, 5]),
        _ => {
            return Err(DecoderError::UnsupportedInput(
                "FFmpeg worker output width has no proven Aurora semantic channel mapping",
            ));
        }
    };
    Ok(contract)
}

/// Build the deterministic FFmpeg command used by the persistent worker.
///
/// Immersive output layouts above 7.1 decode to an 8-channel compatibility
/// bed. Aurora pads the remaining height/object lanes with silence until the
/// native metadata renderer supplies them; FFmpeg is never allowed to invent
/// Atmos/DTS:X object positions.
pub fn build_worker_command(
    codec: CodecKind,
    encapsulation: Encapsulation,
    output: AudioFormat,
) -> Result<WorkerCommand, DecoderError> {
    let decoded_channels = output.channel_count.min(8).max(1);
    let (channel_layout, channel_map) =
        worker_channel_contract(decoded_channels, output.channel_count)?;
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
        "-vn".into(),
        "-sn".into(),
        "-dn".into(),
        "-acodec".into(),
        "pcm_f32le".into(),
        "-ar".into(),
        output.sample_rate.to_string(),
        "-ac".into(),
        decoded_channels.to_string(),
        "-channel_layout".into(),
        channel_layout.into(),
        "-f".into(),
        "f32le".into(),
        "pipe:1".into(),
    ]);

    Ok(WorkerCommand {
        program: "ffmpeg".into(),
        args,
        decoded_channels,
        channel_map,
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
        // Ogg-carried Opus/Vorbis/Speex and ALAC commonly require container
        // headers, so they intentionally rely on FFmpeg probing.
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

pub struct OpenWorkerDecoder {
    codec: CodecKind,
    encapsulation: Encapsulation,
    output: AudioFormat,
    decoded_channels: usize,
    channel_map: Vec<usize>,
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Vec<u8>>,
    reader: Option<JoinHandle<io::Result<()>>>,
    pcm_bytes: Vec<u8>,
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
            decoded_channels: command.decoded_channels,
            channel_map: command.channel_map,
            child,
            stdin: Some(stdin),
            rx,
            reader: Some(reader),
            pcm_bytes: Vec::new(),
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

    /// Feed compressed bytes and return at most one Aurora-sized PCM block.
    /// Additional decoded blocks stay queued in `pcm_bytes` for the next call.
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

    /// Drain any bytes the reader thread has already delivered.
    pub fn poll(&mut self) -> Result<Option<DecodedFrame>, DecoderError> {
        self.collect_stdout();
        self.take_block(false)
    }

    /// Close input, wait for FFmpeg to flush codec delay, then return all final
    /// PCM blocks including a short final block if present. A successful child
    /// exit is accepted only when the stdout reader also ended cleanly and the
    /// raw F32 stream ends on a complete PCM-frame boundary.
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
            return Err(DecoderError::Decode(format!(
                "FFmpeg worker ended with {} trailing byte(s) that do not form a complete PCM frame",
                self.pcm_bytes.len()
            )));
        }
        Ok(frames)
    }

    fn collect_stdout(&mut self) {
        while let Ok(bytes) = self.rx.try_recv() {
            self.pcm_bytes.extend_from_slice(&bytes);
        }
    }

    fn take_block(&mut self, allow_short: bool) -> Result<Option<DecodedFrame>, DecoderError> {
        let bytes_per_frame = self
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
        let block: Vec<u8> = self.pcm_bytes.drain(..take).collect();
        let mut planar = (0..self.output.channel_count)
            .map(|_| vec![0.0_f32; frame_count])
            .collect::<Vec<_>>();
        for frame in 0..frame_count {
            let base = frame * bytes_per_frame;
            for source_channel in 0..self.decoded_channels {
                let at = base + source_channel * 4;
                let sample = decode_worker_sample([
                    block[at],
                    block[at + 1],
                    block[at + 2],
                    block[at + 3],
                ])?;
                let target_channel = self.channel_map[source_channel];
                planar[target_channel][frame] = sample;
            }
        }
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

    #[test]
    fn truehd_uses_explicit_demuxer_and_semantic_seven_one_bed() {
        let cmd =
            build_worker_command(CodecKind::TrueHd, Encapsulation::Elementary, fmt(12)).unwrap();
        assert!(cmd.args.windows(2).any(|p| p == ["-f", "truehd"]));
        assert_eq!(cmd.decoded_channels, 8);
        assert!(cmd.args.windows(2).any(|p| p == ["-ac", "8"]));
        assert!(cmd
            .args
            .windows(2)
            .any(|p| p == ["-channel_layout", "7.1"]));
        assert_eq!(cmd.channel_map, vec![0, 1, 2, 3, 6, 7, 4, 5]);
    }

    #[test]
    fn five_one_worker_layout_matches_aurora_first_six_lanes() {
        let cmd = build_worker_command(CodecKind::Flac, Encapsulation::Elementary, fmt(6)).unwrap();
        assert!(cmd
            .args
            .windows(2)
            .any(|p| p == ["-channel_layout", "5.1"]));
        assert_eq!(cmd.channel_map, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn ambiguous_worker_width_fails_closed() {
        let error = build_worker_command(CodecKind::Flac, Encapsulation::Elementary, fmt(7))
            .unwrap_err();
        assert!(matches!(error, DecoderError::UnsupportedInput(_)));
    }

    #[test]
    fn ogg_opus_keeps_container_probe() {
        let cmd = build_worker_command(CodecKind::Opus, Encapsulation::Ogg, fmt(2)).unwrap();
        assert!(!cmd.args.windows(2).any(|p| p == ["-f", "opus"]));
        assert_eq!(cmd.decoded_channels, 2);
        assert_eq!(cmd.channel_map, vec![0, 1]);
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
