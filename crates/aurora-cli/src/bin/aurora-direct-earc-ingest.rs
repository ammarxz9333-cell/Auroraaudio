//! Prototype direct-eARC Linux ingest path.
//!
//! The utility consumes the raw S32_LE carrier exposed by an eARC receiver on a
//! Linux ALSA capture device (or stdin), extracts the valid IEC61937 16-bit word
//! from each slot without resampling or decoding, and writes canonical S16_LE
//! IEC61937 bytes to stdout.

use std::io::{self, Read, Write};
use std::process::{Child, Command, Stdio};

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};

const DEFAULT_CARRIER_RATE_HZ: u32 = 192_000;
const DEFAULT_SLOTS_PER_FRAME: usize = 2;
const IEC61937_SYNC: [u8; 4] = [0x72, 0xf8, 0x1f, 0x4e];

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CarrierWordHalf {
    /// Reference SiI9437 path: useful IEC61937 word occupies bits 31..16.
    High,
    /// Alternate packing for bring-up only.
    Low,
}

#[derive(Debug, Parser)]
#[command(
    name = "aurora-direct-earc-ingest",
    about = "Normalize direct Linux eARC S32_LE carrier capture into canonical IEC61937 S16_LE"
)]
struct Args {
    /// ALSA capture device passed to arecord, for example hw:0,0.
    /// If omitted, raw S32_LE carrier bytes are read from stdin.
    #[arg(long)]
    alsa_device: Option<String>,

    /// Recovered eARC carrier frame rate.
    #[arg(long, default_value_t = DEFAULT_CARRIER_RATE_HZ)]
    carrier_rate: u32,

    /// Number of 32-bit carrier slots per frame. DD+ reference path uses 2.
    #[arg(long, default_value_t = DEFAULT_SLOTS_PER_FRAME)]
    slots: usize,

    /// Select which 16-bit half of each S32_LE slot contains IEC61937.
    #[arg(long, value_enum, default_value_t = CarrierWordHalf::High)]
    word_half: CarrierWordHalf,

    /// Internal read size. Arbitrary read boundaries are handled safely.
    #[arg(long, default_value_t = 16_384)]
    read_bytes: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct IngestStats {
    carrier_frames: u64,
    output_words: u64,
    sync_bursts: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    validate_args(&args)?;

    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut child = None;

    let stats = if let Some(device) = args.alsa_device.as_deref() {
        let mut capture = spawn_arecord(device, args.carrier_rate, args.slots)?;
        let capture_stdout = capture
            .stdout
            .take()
            .context("arecord stdout was not captured")?;
        child = Some(capture);
        process_stream(
            capture_stdout,
            &mut output,
            args.slots,
            args.word_half,
            args.read_bytes,
        )?
    } else {
        let stdin = io::stdin();
        process_stream(
            stdin.lock(),
            &mut output,
            args.slots,
            args.word_half,
            args.read_bytes,
        )?
    };

    output.flush().context("failed to flush IEC61937 output")?;

    if let Some(mut capture) = child {
        let status = capture.wait().context("failed waiting for arecord")?;
        if !status.success() {
            bail!("arecord exited with status {status}");
        }
    }

    eprintln!(
        "aurora-direct-earc-ingest: frames={} words={} iec61937_sync_bursts={}",
        stats.carrier_frames, stats.output_words, stats.sync_bursts
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
    Ok(())
}

fn spawn_arecord(device: &str, carrier_rate: u32, slots: usize) -> Result<Child> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (device, carrier_rate, slots);
        bail!("--alsa-device is supported only on Linux");
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

fn process_stream<R: Read, W: Write>(
    mut input: R,
    mut output: W,
    slots: usize,
    word_half: CarrierWordHalf,
    read_bytes: usize,
) -> Result<IngestStats> {
    if slots == 0 {
        bail!("slot count must be greater than zero");
    }
    if read_bytes == 0 {
        bail!("read size must be greater than zero");
    }

    let carrier_frame_bytes = slots
        .checked_mul(4)
        .context("carrier frame byte count overflow")?;
    let mut read_buffer = vec![0_u8; read_bytes];
    let mut pending = Vec::<u8>::with_capacity(read_bytes + carrier_frame_bytes);
    let mut scanner = Iec61937SyncScanner::default();
    let mut stats = IngestStats::default();

    loop {
        let count = input
            .read(&mut read_buffer)
            .context("failed reading direct eARC carrier")?;
        if count == 0 {
            break;
        }
        pending.extend_from_slice(&read_buffer[..count]);

        let complete_bytes = pending.len() / carrier_frame_bytes * carrier_frame_bytes;
        if complete_bytes == 0 {
            continue;
        }

        let normalized = normalize_s32le_carrier(&pending[..complete_bytes], slots, word_half)?;
        scanner.feed(&normalized);
        output
            .write_all(&normalized)
            .context("failed writing canonical IEC61937 bytes")?;

        let frames = complete_bytes / carrier_frame_bytes;
        stats.carrier_frames = stats.carrier_frames.saturating_add(frames as u64);
        stats.output_words = stats
            .output_words
            .saturating_add((frames.saturating_mul(slots)) as u64);
        pending.drain(..complete_bytes);
    }

    if !pending.is_empty() {
        bail!(
            "capture ended with {} trailing bytes; expected complete {}-byte carrier frames",
            pending.len(),
            carrier_frame_bytes
        );
    }

    stats.sync_bursts = scanner.sync_bursts;
    Ok(stats)
}

fn normalize_s32le_carrier(
    input: &[u8],
    slots: usize,
    word_half: CarrierWordHalf,
) -> Result<Vec<u8>> {
    if slots == 0 {
        bail!("slot count must be greater than zero");
    }
    let frame_bytes = slots.checked_mul(4).context("carrier frame overflow")?;
    if input.len() % frame_bytes != 0 {
        bail!(
            "carrier input length {} is not a multiple of {} bytes",
            input.len(),
            frame_bytes
        );
    }

    let mut output = Vec::with_capacity(input.len() / 2);
    for raw in input.chunks_exact(4) {
        let word = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        let iec_word = match word_half {
            CarrierWordHalf::High => (word >> 16) as u16,
            CarrierWordHalf::Low => word as u16,
        };
        output.extend_from_slice(&iec_word.to_le_bytes());
    }
    Ok(output)
}

#[derive(Debug, Default)]
struct Iec61937SyncScanner {
    tail: Vec<u8>,
    sync_bursts: u64,
}

impl Iec61937SyncScanner {
    fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut joined = Vec::with_capacity(self.tail.len() + bytes.len());
        joined.extend_from_slice(&self.tail);
        joined.extend_from_slice(bytes);
        self.sync_bursts = self.sync_bursts.saturating_add(
            joined
                .windows(IEC61937_SYNC.len())
                .filter(|window| *window == IEC61937_SYNC)
                .count() as u64,
        );
        let keep = IEC61937_SYNC.len().saturating_sub(1).min(joined.len());
        self.tail.clear();
        self.tail.extend_from_slice(&joined[joined.len() - keep..]);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn s32_with_high_word(word: u16) -> [u8; 4] {
        (u32::from(word) << 16).to_le_bytes()
    }

    #[test]
    fn reference_high_half_preserves_iec61937_sync_words_bit_exactly() {
        let mut input = Vec::new();
        input.extend_from_slice(&s32_with_high_word(0xf872));
        input.extend_from_slice(&s32_with_high_word(0x4e1f));

        let output = normalize_s32le_carrier(&input, 2, CarrierWordHalf::High).unwrap();
        assert_eq!(output, IEC61937_SYNC);
    }

    #[test]
    fn low_half_mode_is_explicit_and_bit_exact() {
        let mut input = Vec::new();
        input.extend_from_slice(&0x1234_abcd_u32.to_le_bytes());
        input.extend_from_slice(&0x5678_ef01_u32.to_le_bytes());

        let output = normalize_s32le_carrier(&input, 2, CarrierWordHalf::Low).unwrap();
        assert_eq!(output, [0xcd, 0xab, 0x01, 0xef]);
    }

    #[test]
    fn arbitrary_read_boundaries_do_not_change_carrier_words() {
        let words = [0xf872_u16, 0x4e1f, 0x0015, 0x0080];
        let mut raw = Vec::new();
        for word in words {
            raw.extend_from_slice(&s32_with_high_word(word));
        }

        let mut output = Vec::new();
        let stats = process_stream(
            Cursor::new(raw),
            &mut output,
            2,
            CarrierWordHalf::High,
            5,
        )
        .unwrap();

        assert_eq!(
            output,
            [0x72, 0xf8, 0x1f, 0x4e, 0x15, 0x00, 0x80, 0x00]
        );
        assert_eq!(stats.carrier_frames, 2);
        assert_eq!(stats.output_words, 4);
        assert_eq!(stats.sync_bursts, 1);
    }

    #[test]
    fn incomplete_carrier_frame_is_rejected_instead_of_padded() {
        let error = process_stream(
            Cursor::new(vec![0_u8; 7]),
            Vec::<u8>::new(),
            2,
            CarrierWordHalf::High,
            3,
        )
        .unwrap_err();
        assert!(error.to_string().contains("trailing bytes"));
    }

    #[test]
    fn sync_scanner_detects_preamble_across_chunk_boundary() {
        let mut scanner = Iec61937SyncScanner::default();
        scanner.feed(&[0xaa, 0x72, 0xf8]);
        scanner.feed(&[0x1f, 0x4e, 0xbb]);
        assert_eq!(scanner.sync_bursts, 1);
    }
}
