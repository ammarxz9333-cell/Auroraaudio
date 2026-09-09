//! Single-process prototype for Aurora encoded input -> decode -> speaker DSP -> output.
//!
//! Direct eARC mode captures S32_LE from ALSA (or stdin), normalizes it and
//! decodes in one process. Legacy mode consumes complete Aurora USB v1
//! SOCK_SEQPACKET messages from the existing STM32 bridge socket and feeds the
//! exact ENCODED_IEC61937 payload into the same runtime.
//!
//! The final speaker stream can remain raw canonical 12-channel interleaved F32
//! on stdout for evidence/inspection or be sent to Aurora's native Linux ALSA
//! backend as exact S32_LE. Wider physical TDM layouts are zero-padded only; no
//! extra channels are synthesized.

use std::io::{self, Read, Write};
use std::process::Child;

#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use aurora_alsa_output::{AlsaOutputConfig, NativeAlsaPlayback};
use aurora_core::{AudioFormat, SampleType};
use aurora_decoder_engine::EngineConfig;
use aurora_dsp_basic::output::{
    OutputDspConfig, CHANNELS as OUTPUT_CHANNELS, SAMPLE_RATE as OUTPUT_SAMPLE_RATE,
};
use aurora_encoded_input::EncodedInputConfig;
use aurora_encoded_runtime::{AuroraPlaybackRuntime, PlaybackBatch, SpeakerOutputFrame};
use aurora_iec61937::CarrierWordHalf;
use clap::{Parser, ValueEnum};

const DEFAULT_CARRIER_RATE_HZ: u32 = 192_000;
const DEFAULT_SLOTS: usize = 2;
const DEFAULT_BRIDGE_SOCKET: &str = "/run/aurora/usb-bridge.sock";
const LEGACY_PACKET_BUFFER_BYTES: usize = 512 * 1024;
const DEFAULT_OUTPUT_PERIOD_FRAMES: usize = 256;
const DEFAULT_OUTPUT_BUFFER_FRAMES: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum InputMode {
    DirectEarc,
    LegacyUsb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputMode {
    /// Write canonical 12-channel interleaved little-endian F32 to stdout.
    StdoutF32,
    /// Write native S32_LE directly to a Linux ALSA/ASoC endpoint.
    AlsaS32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum WordHalfArg {
    High,
    Low,
}

impl From<WordHalfArg> for CarrierWordHalf {
    fn from(value: WordHalfArg) -> Self {
        match value {
            WordHalfArg::High => Self::High,
            WordHalfArg::Low => Self::Low,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "aurora-encoded-runtime",
    about = "Run direct eARC or legacy STM32/USB through Aurora decode, DSP and output"
)]
struct Args {
    /// Select the physical encoded input explicitly. Aurora never auto-switches.
    #[arg(long, value_enum, default_value = "direct-earc")]
    input: InputMode,

    /// ALSA capture device for direct eARC, e.g. hw:0,0. Omit to read S32_LE from stdin.
    #[arg(long)]
    alsa_device: Option<String>,

    /// Select the final speaker sink.
    #[arg(long, value_enum, default_value = "stdout-f32")]
    output: OutputMode,

    /// ALSA playback device for --output alsa-s32, for example hw:1,0.
    /// Omit to use ALSA's `default` device.
    #[arg(long)]
    output_device: Option<String>,

    /// Physical ALSA playback channel count. Values above 12 zero-pad Aurora's
    /// canonical 7.1.4 output, allowing a 12-channel render to drive TDM16.
    #[arg(long, default_value_t = OUTPUT_CHANNELS)]
    hardware_output_channels: usize,

    /// Preferred ALSA hardware period size. The device may negotiate a nearby value.
    #[arg(long, default_value_t = DEFAULT_OUTPUT_PERIOD_FRAMES)]
    output_period_frames: usize,

    /// Preferred ALSA hardware buffer size. Must be at least two periods.
    #[arg(long, default_value_t = DEFAULT_OUTPUT_BUFFER_FRAMES)]
    output_buffer_frames: usize,

    /// Recovered direct-eARC carrier frame rate.
    #[arg(long, default_value_t = DEFAULT_CARRIER_RATE_HZ)]
    carrier_rate: u32,

    /// S32_LE serial-audio slots per direct-eARC frame.
    #[arg(long, default_value_t = DEFAULT_SLOTS)]
    slots: usize,

    /// 16-bit half of each S32_LE slot containing the IEC61937 carrier word.
    #[arg(long, value_enum, default_value = "high")]
    word_half: WordHalfArg,

    /// Existing Aurora USB bridge SOCK_SEQPACKET path for legacy STM32 fallback.
    #[arg(long, default_value = DEFAULT_BRIDGE_SOCKET)]
    bridge_socket: String,

    /// Process read size for direct eARC capture.
    #[arg(long, default_value_t = 16_384)]
    read_bytes: usize,

    /// Decoder/output sample rate. Integrated speaker DSP currently requires 48 kHz.
    #[arg(long, default_value_t = OUTPUT_SAMPLE_RATE)]
    output_rate: u32,

    /// Logical output channel count. Integrated speaker DSP requires canonical 7.1.4 = 12.
    #[arg(long, default_value_t = OUTPUT_CHANNELS)]
    output_channels: usize,

    /// Decoder preferred output block size.
    #[arg(long, default_value_t = 40)]
    block_size: usize,
}

#[derive(Debug, Default)]
struct RuntimeStats {
    carrier_bursts: u64,
    format_changes: u64,
    decoded_frames: u64,
    decoded_pcm_frames: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;

    let input_config = match args.input {
        InputMode::DirectEarc => EncodedInputConfig::DirectEarc {
            slots: args.slots,
            word_half: args.word_half.into(),
        },
        InputMode::LegacyUsb => EncodedInputConfig::LegacyUsb,
    };
    let output_format = AudioFormat {
        sample_rate: args.output_rate,
        channel_count: args.output_channels,
        sample_type: SampleType::F32,
        block_size: args.block_size,
    };
    let mut runtime = AuroraPlaybackRuntime::new(
        input_config,
        EngineConfig::default(),
        output_format,
        OutputDspConfig::default(),
    )
    .context("failed to initialize Aurora encoded playback runtime")?;

    let stats = match args.output {
        OutputMode::StdoutF32 => {
            let stdout = io::stdout();
            let mut sink = StdoutF32Sink::new(stdout.lock());
            let stats = run_selected_input(&args, &mut runtime, &mut sink)?;
            sink.finish()?;
            stats
        }
        OutputMode::AlsaS32 => {
            let device = args.output_device.as_deref().unwrap_or("default");
            let mut sink = NativeAlsaSink::open(AlsaOutputConfig {
                device: device.to_owned(),
                sample_rate: args.output_rate,
                logical_channels: OUTPUT_CHANNELS,
                hardware_channels: args.hardware_output_channels,
                period_frames: args.output_period_frames,
                buffer_frames: args.output_buffer_frames,
            })?;
            let stats = run_selected_input(&args, &mut runtime, &mut sink)?;
            sink.finish()?;
            stats
        }
    };

    eprintln!(
        "aurora-encoded-runtime: input={:?} output={:?} bursts={} format_changes={} decoded_frames={} decoded_pcm_frames={}",
        args.input,
        args.output,
        stats.carrier_bursts,
        stats.format_changes,
        stats.decoded_frames,
        stats.decoded_pcm_frames
    );
    Ok(())
}

fn validate_args(args: &Args) -> Result<()> {
    if args.carrier_rate == 0 {
        bail!("carrier rate must be greater than zero");
    }
    if args.slots == 0 {
        bail!("slot count must be greater than zero");
    }
    if args.read_bytes == 0 {
        bail!("read size must be greater than zero");
    }
    if args.block_size == 0 {
        bail!("decoder output block size must be greater than zero");
    }
    if args.output_rate != OUTPUT_SAMPLE_RATE || args.output_channels != OUTPUT_CHANNELS {
        bail!(
            "integrated Aurora speaker DSP currently requires {} Hz and {} canonical 7.1.4 channels",
            OUTPUT_SAMPLE_RATE,
            OUTPUT_CHANNELS
        );
    }
    if args.hardware_output_channels < OUTPUT_CHANNELS {
        bail!(
            "physical output must expose at least {} channels; got {}",
            OUTPUT_CHANNELS,
            args.hardware_output_channels
        );
    }
    if args.output_period_frames == 0 {
        bail!("ALSA output period must be greater than zero");
    }
    if args.output_buffer_frames < args.output_period_frames.saturating_mul(2) {
        bail!("ALSA output buffer must be at least two periods");
    }
    if matches!(args.input, InputMode::LegacyUsb) && args.alsa_device.is_some() {
        bail!("--alsa-device is valid only with --input direct-earc");
    }
    if matches!(args.output, OutputMode::StdoutF32) && args.output_device.is_some() {
        bail!("--output-device is valid only with --output alsa-s32");
    }
    Ok(())
}

fn run_selected_input<S: SpeakerSink>(
    args: &Args,
    runtime: &mut AuroraPlaybackRuntime,
    sink: &mut S,
) -> Result<RuntimeStats> {
    match args.input {
        InputMode::DirectEarc => run_direct(args, runtime, sink),
        InputMode::LegacyUsb => run_legacy(args, runtime, sink),
    }
}

fn run_direct<S: SpeakerSink>(
    args: &Args,
    runtime: &mut AuroraPlaybackRuntime,
    sink: &mut S,
) -> Result<RuntimeStats> {
    if let Some(device) = args.alsa_device.as_deref() {
        let mut child = spawn_arecord(device, args.carrier_rate, args.slots)?;
        let capture_stdout = child
            .stdout
            .take()
            .context("arecord stdout was not captured")?;
        let stats = run_direct_stream(capture_stdout, runtime, sink, args.read_bytes)?;
        let status = child.wait().context("failed waiting for arecord")?;
        if !status.success() {
            bail!("arecord exited with status {status}");
        }
        Ok(stats)
    } else {
        let stdin = io::stdin();
        run_direct_stream(stdin.lock(), runtime, sink, args.read_bytes)
    }
}

fn run_direct_stream<R: Read, S: SpeakerSink>(
    mut input: R,
    runtime: &mut AuroraPlaybackRuntime,
    sink: &mut S,
    read_bytes: usize,
) -> Result<RuntimeStats> {
    let mut read_buffer = vec![0_u8; read_bytes];
    let mut stats = RuntimeStats::default();
    loop {
        let count = input
            .read(&mut read_buffer)
            .context("failed reading direct eARC capture")?;
        if count == 0 {
            break;
        }
        let batch = runtime
            .push_direct_s32(&read_buffer[..count])
            .context("direct eARC playback runtime ingest failed")?;
        consume_batch(batch, sink, &mut stats)?;
    }
    runtime
        .finish()
        .context("direct eARC capture ended on an incomplete serial-audio frame")?;
    Ok(stats)
}

fn spawn_arecord(device: &str, carrier_rate: u32, slots: usize) -> Result<Child> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (device, carrier_rate, slots);
        bail!("--alsa-device direct eARC capture is supported only on Linux");
    }

    #[cfg(target_os = "linux")]
    {
        Command::new("arecord")
            .args([
                "-q",
                "-D",
                device,
                "-t",
                "raw",
                "-f",
                "S32_LE",
                "-r",
                &carrier_rate.to_string(),
                "-c",
                &slots.to_string(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to start arecord for ALSA device {device}"))
    }
}

#[cfg(unix)]
fn run_legacy<S: SpeakerSink>(
    args: &Args,
    runtime: &mut AuroraPlaybackRuntime,
    sink: &mut S,
) -> Result<RuntimeStats> {
    let socket = SeqPacketSocket::connect(&args.bridge_socket)
        .with_context(|| format!("failed to connect legacy bridge {}", args.bridge_socket))?;
    let mut packet = vec![0_u8; LEGACY_PACKET_BUFFER_BYTES];
    let mut stats = RuntimeStats::default();

    loop {
        let count = socket.recv(&mut packet).context("legacy bridge receive failed")?;
        if count == 0 {
            break;
        }
        let batch = runtime
            .push_legacy_usb_packet(&packet[..count])
            .context("legacy STM32/USB playback runtime ingest failed")?;
        consume_batch(batch, sink, &mut stats)?;
    }
    runtime.finish().context("legacy runtime finalization failed")?;
    Ok(stats)
}

#[cfg(not(unix))]
fn run_legacy<S: SpeakerSink>(
    args: &Args,
    _runtime: &mut AuroraPlaybackRuntime,
    _sink: &mut S,
) -> Result<RuntimeStats> {
    let _ = args;
    bail!("legacy STM32/USB bridge socket mode currently requires Unix/Linux")
}

fn consume_batch<S: SpeakerSink>(
    batch: PlaybackBatch,
    sink: &mut S,
    stats: &mut RuntimeStats,
) -> Result<()> {
    stats.carrier_bursts = stats.carrier_bursts.saturating_add(batch.bursts as u64);
    stats.format_changes = stats
        .format_changes
        .saturating_add(batch.format_changes as u64);

    for frame in batch.frames {
        sink.write_frame(&frame)?;
        stats.decoded_frames = stats.decoded_frames.saturating_add(1);
        stats.decoded_pcm_frames = stats
            .decoded_pcm_frames
            .saturating_add(frame.frame_count as u64);
    }
    Ok(())
}

trait SpeakerSink {
    fn write_frame(&mut self, frame: &SpeakerOutputFrame) -> Result<()>;
    fn finish(&mut self) -> Result<()>;
}

struct StdoutF32Sink<W: Write> {
    writer: W,
}

impl<W: Write> StdoutF32Sink<W> {
    fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write> SpeakerSink for StdoutF32Sink<W> {
    fn write_frame(&mut self, frame: &SpeakerOutputFrame) -> Result<()> {
        validate_speaker_frame(frame)?;
        for sample in &frame.interleaved_f32 {
            if !sample.is_finite() {
                bail!("speaker DSP produced a non-finite sample");
            }
            self.writer
                .write_all(&sample.to_le_bytes())
                .context("failed writing interleaved processed F32 speaker PCM")?;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        self.writer
            .flush()
            .context("failed to flush processed speaker PCM output")
    }
}

struct NativeAlsaSink {
    playback: NativeAlsaPlayback,
}

impl NativeAlsaSink {
    fn open(config: AlsaOutputConfig) -> Result<Self> {
        let playback = NativeAlsaPlayback::open(config)
            .context("failed to open Aurora native ALSA speaker output")?;
        Ok(Self { playback })
    }
}

impl SpeakerSink for NativeAlsaSink {
    fn write_frame(&mut self, frame: &SpeakerOutputFrame) -> Result<()> {
        validate_speaker_frame(frame)?;
        self.playback
            .write_interleaved_f32(&frame.interleaved_f32, frame.discontinuity)
            .context("native ALSA speaker write failed")
    }

    fn finish(&mut self) -> Result<()> {
        self.playback
            .drain()
            .context("failed draining native ALSA speaker output")?;
        #[cfg(target_os = "linux")]
        {
            let telemetry = self.playback.telemetry();
            eprintln!(
                "aurora-alsa-output: device={} rate={} channels={} period={} buffer={} frames_written={} xruns={} recoveries={} discontinuity_resets={}",
                telemetry.device,
                telemetry.sample_rate,
                telemetry.hardware_channels,
                telemetry.period_frames,
                telemetry.buffer_frames,
                telemetry.frames_written,
                telemetry.xruns,
                telemetry.recoveries,
                telemetry.discontinuity_resets
            );
        }
        Ok(())
    }
}

fn validate_speaker_frame(frame: &SpeakerOutputFrame) -> Result<()> {
    let expected = frame.frame_count.saturating_mul(OUTPUT_CHANNELS);
    if frame.interleaved_f32.len() != expected {
        bail!(
            "speaker frame contains {} samples; expected {} for {} frames x {} channels",
            frame.interleaved_f32.len(),
            expected,
            frame.frame_count,
            OUTPUT_CHANNELS
        );
    }
    Ok(())
}

#[cfg(unix)]
struct SeqPacketSocket {
    fd: std::os::fd::OwnedFd,
}

#[cfg(unix)]
impl SeqPacketSocket {
    fn connect(path: &str) -> io::Result<Self> {
        use std::mem::{size_of, zeroed};
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

        if path.as_bytes().contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "bridge socket path contains NUL",
            ));
        }

        let raw_fd = unsafe {
            libc::socket(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
            )
        };
        if raw_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        let mut address: libc::sockaddr_un = unsafe { zeroed() };
        address.sun_family = libc::AF_UNIX as libc::sa_family_t;
        if path.len() >= address.sun_path.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "bridge socket path is too long",
            ));
        }
        for (destination, source) in address
            .sun_path
            .iter_mut()
            .zip(path.as_bytes().iter().copied())
        {
            *destination = source as libc::c_char;
        }

        let rc = unsafe {
            libc::connect(
                fd.as_raw_fd(),
                (&address as *const libc::sockaddr_un).cast::<libc::sockaddr>(),
                size_of::<libc::sockaddr_un>() as libc::socklen_t,
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd })
    }

    fn recv(&self, buffer: &mut [u8]) -> io::Result<usize> {
        use std::os::fd::AsRawFd;

        let count = unsafe {
            libc::recv(
                self.fd.as_raw_fd(),
                buffer.as_mut_ptr().cast::<libc::c_void>(),
                buffer.len(),
                0,
            )
        };
        if count < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(count as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_args() -> Args {
        Args {
            input: InputMode::DirectEarc,
            alsa_device: None,
            output: OutputMode::StdoutF32,
            output_device: None,
            hardware_output_channels: OUTPUT_CHANNELS,
            output_period_frames: DEFAULT_OUTPUT_PERIOD_FRAMES,
            output_buffer_frames: DEFAULT_OUTPUT_BUFFER_FRAMES,
            carrier_rate: 192_000,
            slots: 2,
            word_half: WordHalfArg::High,
            bridge_socket: DEFAULT_BRIDGE_SOCKET.to_owned(),
            read_bytes: 16_384,
            output_rate: OUTPUT_SAMPLE_RATE,
            output_channels: OUTPUT_CHANNELS,
            block_size: 40,
        }
    }

    #[test]
    fn rejects_noncanonical_runtime_shape() {
        let mut args = valid_args();
        args.output_channels = 8;
        assert!(validate_args(&args).is_err());

        let mut args = valid_args();
        args.output_rate = 96_000;
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn rejects_physical_output_narrower_than_aurora_layout() {
        let mut args = valid_args();
        args.hardware_output_channels = OUTPUT_CHANNELS - 1;
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn rejects_output_buffer_smaller_than_two_periods() {
        let mut args = valid_args();
        args.output_period_frames = 256;
        args.output_buffer_frames = 256;
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn legacy_mode_rejects_alsa_capture_device() {
        let mut args = valid_args();
        args.input = InputMode::LegacyUsb;
        args.alsa_device = Some("hw:0,0".to_owned());
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn stdout_mode_rejects_alsa_output_device() {
        let mut args = valid_args();
        args.output_device = Some("hw:1,0".to_owned());
        assert!(validate_args(&args).is_err());
    }

    #[test]
    fn stdout_sink_preserves_canonical_interleaving() {
        let mut bytes = Vec::new();
        {
            let mut sink = StdoutF32Sink::new(&mut bytes);
            let samples = (0..OUTPUT_CHANNELS)
                .map(|value| value as f32 / OUTPUT_CHANNELS as f32)
                .collect::<Vec<_>>();
            sink.write_frame(&SpeakerOutputFrame {
                interleaved_f32: samples.clone(),
                frame_count: 1,
                presentation_time_seconds: 0.0,
                discontinuity: false,
            })
            .unwrap();
            sink.finish().unwrap();
        }
        assert_eq!(bytes.len(), OUTPUT_CHANNELS * 4);
        for (index, chunk) in bytes.chunks_exact(4).enumerate() {
            let sample = f32::from_le_bytes(chunk.try_into().unwrap());
            assert_eq!(sample, index as f32 / OUTPUT_CHANNELS as f32);
        }
    }
}
