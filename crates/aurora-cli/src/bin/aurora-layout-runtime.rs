//! Explicit-layout Aurora encoded playback appliance.
//!
//! This additive binary leaves the canonical 7.1.4 `aurora-encoded-runtime`
//! untouched. It consumes proven direct-eARC S32_LE carrier input from stdin,
//! binds one explicit speaker-layout identity end to end, then writes dynamic
//! interleaved speaker PCM either as F32_LE on stdout or through Aurora's native
//! ALSA S32_LE/TDM backend.
//!
//! Native threaded ALSA capture remains in the canonical appliance until the
//! generic capture/runtime boundary is validated separately. No layout is ever
//! inferred from a bare channel count.

use std::io::{self, Read, Write};

use anyhow::{bail, Context, Result};
use aurora_alsa_output::{AlsaOutputConfig, NativeAlsaPlayback};
use aurora_core::{AudioFormat, SampleType, StandardLayout};
use aurora_decoder_engine::EngineConfig;
use aurora_dsp_basic::output::{OutputDspConfig, SAMPLE_RATE as OUTPUT_SAMPLE_RATE};
use aurora_encoded_input::EncodedInputConfig;
use aurora_iec61937::CarrierWordHalf;
use aurora_layout_playback_runtime::{LayoutPlaybackBatch, LayoutPlaybackRuntime};
use clap::{Parser, ValueEnum};

const DEFAULT_SLOTS: usize = 2;
const DEFAULT_READ_BYTES: usize = 64 * 1024;
const DEFAULT_OUTPUT_PERIOD_FRAMES: usize = 256;
const DEFAULT_OUTPUT_BUFFER_FRAMES: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LayoutArg {
    #[value(name = "2.0")]
    Stereo,
    #[value(name = "5.1")]
    FiveOne,
    #[value(name = "5.1.2")]
    FiveOneTwo,
    #[value(name = "5.1.4")]
    FiveOneFour,
    #[value(name = "7.1")]
    SevenOne,
    #[value(name = "7.1.2")]
    SevenOneTwo,
    #[value(name = "7.1.4")]
    SevenOneFour,
    #[value(name = "aurora-11.1.4-reference")]
    AuroraElevenOneFourReference,
}

impl LayoutArg {
    const fn standard_layout(self) -> Option<StandardLayout> {
        match self {
            Self::Stereo => Some(StandardLayout::Stereo),
            Self::FiveOne => Some(StandardLayout::FiveOne),
            Self::FiveOneTwo => Some(StandardLayout::FiveOneTwo),
            Self::FiveOneFour => Some(StandardLayout::FiveOneFour),
            Self::SevenOne => Some(StandardLayout::SevenOne),
            Self::SevenOneTwo => Some(StandardLayout::SevenOneTwo),
            Self::SevenOneFour => Some(StandardLayout::SevenOneFour),
            Self::AuroraElevenOneFourReference => None,
        }
    }

    const fn channel_count(self) -> usize {
        match self {
            Self::Stereo => 2,
            Self::FiveOne => 6,
            Self::FiveOneTwo | Self::SevenOne => 8,
            Self::FiveOneFour | Self::SevenOneTwo => 10,
            Self::SevenOneFour => 12,
            Self::AuroraElevenOneFourReference => 16,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Stereo => "2.0",
            Self::FiveOne => "5.1",
            Self::FiveOneTwo => "5.1.2",
            Self::FiveOneFour => "5.1.4",
            Self::SevenOne => "7.1",
            Self::SevenOneTwo => "7.1.2",
            Self::SevenOneFour => "7.1.4",
            Self::AuroraElevenOneFourReference => "aurora-11.1.4-reference-v1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OutputMode {
    #[value(name = "stdout-f32")]
    StdoutF32,
    #[value(name = "alsa-s32")]
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
    name = "aurora-layout-runtime",
    about = "Run proven direct-eARC carrier input through an explicit Aurora speaker layout"
)]
struct Args {
    #[arg(long, value_enum, default_value = "aurora-11.1.4-reference")]
    layout: LayoutArg,
    #[arg(long, value_enum, default_value = "stdout-f32")]
    output: OutputMode,
    #[arg(long)]
    output_device: Option<String>,
    #[arg(long, default_value_t = 16)]
    hardware_output_channels: usize,
    #[arg(long, default_value_t = DEFAULT_OUTPUT_PERIOD_FRAMES)]
    output_period_frames: usize,
    #[arg(long, default_value_t = DEFAULT_OUTPUT_BUFFER_FRAMES)]
    output_buffer_frames: usize,
    #[arg(long, default_value_t = DEFAULT_READ_BYTES)]
    read_bytes: usize,
    #[arg(long, default_value_t = DEFAULT_SLOTS)]
    slots: usize,
    #[arg(long, value_enum, default_value = "high")]
    word_half: WordHalfArg,
    #[arg(long, default_value_t = 40)]
    block_size: usize,
}

trait SpeakerSink {
    fn reset_for_transport_discontinuity(&mut self) -> Result<()>;
    fn write_frame(&mut self, interleaved_f32: &[f32], discontinuity: bool) -> Result<()>;
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
    fn reset_for_transport_discontinuity(&mut self) -> Result<()> {
        Ok(())
    }

    fn write_frame(&mut self, interleaved_f32: &[f32], _discontinuity: bool) -> Result<()> {
        for sample in interleaved_f32 {
            self.writer
                .write_all(&sample.to_le_bytes())
                .context("failed writing explicit-layout F32 speaker output")?;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        self.writer
            .flush()
            .context("failed flushing explicit-layout F32 speaker output")
    }
}

struct NativeAlsaSink {
    playback: NativeAlsaPlayback,
    suppress_next_frame_discontinuity: bool,
}

impl NativeAlsaSink {
    fn open(config: AlsaOutputConfig) -> Result<Self> {
        Ok(Self {
            playback: NativeAlsaPlayback::open(config)
                .context("failed to open explicit-layout native ALSA output")?,
            suppress_next_frame_discontinuity: false,
        })
    }
}

impl SpeakerSink for NativeAlsaSink {
    fn reset_for_transport_discontinuity(&mut self) -> Result<()> {
        self.playback
            .reset_for_discontinuity()
            .context("failed resetting explicit-layout ALSA output at discontinuity")?;
        self.suppress_next_frame_discontinuity = true;
        Ok(())
    }

    fn write_frame(&mut self, interleaved_f32: &[f32], discontinuity: bool) -> Result<()> {
        let discontinuity = if self.suppress_next_frame_discontinuity {
            self.suppress_next_frame_discontinuity = false;
            false
        } else {
            discontinuity
        };
        self.playback
            .write_interleaved_f32(interleaved_f32, discontinuity)
            .context("failed writing explicit-layout speaker PCM to ALSA")
    }

    fn finish(&mut self) -> Result<()> {
        self.playback
            .drain()
            .context("failed draining explicit-layout ALSA output")
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;

    let logical_channels = args.layout.channel_count();
    let output_format = AudioFormat {
        sample_rate: OUTPUT_SAMPLE_RATE,
        channel_count: logical_channels,
        sample_type: SampleType::F32,
        block_size: args.block_size,
    };
    let input_config = EncodedInputConfig::DirectEarc {
        slots: args.slots,
        word_half: args.word_half.into(),
    };
    let engine = EngineConfig::default();
    let mut runtime = match args.layout.standard_layout() {
        Some(layout) => LayoutPlaybackRuntime::new_for_standard_layout(
            input_config,
            engine,
            output_format,
            layout,
            OutputDspConfig::default(),
        ),
        None => LayoutPlaybackRuntime::new_for_aurora_eleven_one_four_reference(
            input_config,
            engine,
            output_format,
            OutputDspConfig::default(),
        ),
    }
    .context("failed to initialize explicit-layout Aurora playback runtime")?;

    if runtime.output_layout().channel_count() != logical_channels
        || runtime.output_layout().name() != args.layout.name()
    {
        bail!("constructed runtime layout does not match the explicit CLI layout identity");
    }

    match args.output {
        OutputMode::StdoutF32 => {
            let stdout = io::stdout();
            let mut sink = StdoutF32Sink::new(stdout.lock());
            run_stdin(&args, &mut runtime, &mut sink)?;
            sink.finish()?;
        }
        OutputMode::AlsaS32 => {
            let config = AlsaOutputConfig {
                device: args.output_device.as_deref().unwrap_or("default").to_owned(),
                sample_rate: OUTPUT_SAMPLE_RATE,
                logical_channels,
                hardware_channels: args.hardware_output_channels,
                period_frames: args.output_period_frames,
                buffer_frames: args.output_buffer_frames,
            };
            let mut sink = NativeAlsaSink::open(config)?;
            run_stdin(&args, &mut runtime, &mut sink)?;
            sink.finish()?;
        }
    }

    let decoder = runtime.encoded().decoder();
    let joc = decoder.engine().joc_status();
    eprintln!(
        "aurora-layout-runtime: layout={} channels={} joc_classified={} joc_render_active={} joc_layout={:?} joc_channels={:?} joc_objects={:?} joc_fallback={:?}",
        runtime.output_layout().name(),
        runtime.output_layout().channel_count(),
        joc.codec_classified_joc,
        joc.speaker_render_active,
        joc.layout_name,
        joc.channel_count,
        joc.object_count,
        joc.fallback_reason
    );
    Ok(())
}

fn validate_args(args: &Args) -> Result<()> {
    if args.read_bytes == 0 {
        bail!("stdin read size must be greater than zero");
    }
    if args.block_size == 0 {
        bail!("decoder output block size must be greater than zero");
    }
    if args.slots != DEFAULT_SLOTS {
        bail!("direct eARC wider-layout runtime currently requires exactly two S32 carrier slots");
    }
    if args.word_half != WordHalfArg::High {
        bail!("direct eARC wider-layout runtime currently requires proven high-half S32 packing");
    }
    if matches!(args.output, OutputMode::StdoutF32) && args.output_device.is_some() {
        bail!("--output-device is valid only with --output alsa-s32");
    }
    if matches!(args.output, OutputMode::AlsaS32) {
        validate_output_geometry(args.layout.channel_count(), args.hardware_output_channels)?;
        if args.output_period_frames == 0 {
            bail!("ALSA output period must be greater than zero");
        }
        if args.output_buffer_frames < args.output_period_frames.saturating_mul(2) {
            bail!("ALSA output buffer must be at least two periods");
        }
    }
    Ok(())
}

fn validate_output_geometry(logical_channels: usize, hardware_channels: usize) -> Result<()> {
    if hardware_channels < logical_channels {
        bail!(
            "selected layout requires {logical_channels} logical channels but hardware exposes only {hardware_channels} slots"
        );
    }
    Ok(())
}

fn run_stdin<S: SpeakerSink>(
    args: &Args,
    runtime: &mut LayoutPlaybackRuntime,
    sink: &mut S,
) -> Result<()> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut buffer = vec![0_u8; args.read_bytes];
    loop {
        let read = input
            .read(&mut buffer)
            .context("failed reading direct-eARC S32_LE input from stdin")?;
        if read == 0 {
            break;
        }
        let batch = runtime
            .push_direct_s32(&buffer[..read])
            .context("explicit-layout direct-eARC ingest failed")?;
        consume_batch(batch, runtime, sink)?;
    }
    let batch = runtime
        .finish()
        .context("failed finishing explicit-layout playback runtime")?;
    consume_batch(batch, runtime, sink)
}

fn consume_batch<S: SpeakerSink>(
    mut batch: LayoutPlaybackBatch,
    runtime: &mut LayoutPlaybackRuntime,
    sink: &mut S,
) -> Result<()> {
    if batch.discontinuity {
        sink.reset_for_transport_discontinuity()?;
    }
    for frame in batch.frames.drain(..) {
        let write_result = sink.write_frame(&frame.interleaved_f32, frame.discontinuity);
        runtime.recycle_output_frame(frame);
        write_result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_layout_widths_are_not_inferred() {
        let cases = [
            (LayoutArg::Stereo, 2),
            (LayoutArg::FiveOne, 6),
            (LayoutArg::FiveOneTwo, 8),
            (LayoutArg::SevenOne, 8),
            (LayoutArg::FiveOneFour, 10),
            (LayoutArg::SevenOneTwo, 10),
            (LayoutArg::SevenOneFour, 12),
            (LayoutArg::AuroraElevenOneFourReference, 16),
        ];
        for (layout, channels) in cases {
            assert_eq!(layout.channel_count(), channels);
        }
    }

    #[test]
    fn hardware_must_expose_at_least_the_selected_layout_width() {
        assert!(validate_output_geometry(16, 15).is_err());
        validate_output_geometry(16, 16).unwrap();
        validate_output_geometry(16, 24).unwrap();
    }
}
