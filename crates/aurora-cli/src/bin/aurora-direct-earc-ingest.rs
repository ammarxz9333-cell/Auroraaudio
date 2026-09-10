//! Prototype direct-eARC carrier normalization helper.
//!
//! This utility is intentionally stdin-only: it converts a captured raw S32_LE
//! two-slot carrier into canonical S16_LE IEC61937 bytes. Native device capture
//! belongs to `aurora-encoded-runtime --alsa-device`, which uses Aurora's owned
//! ALSA backend and recovery/discontinuity semantics instead of an `arecord`
//! subprocess.

use std::io::{self, Read, Write};

use anyhow::{bail, Context, Result};
use aurora_iec61937::{CarrierWordHalf, S32LeCarrierNormalizer};
use clap::{Parser, ValueEnum};

const DEFAULT_CARRIER_RATE_HZ: u32 = 192_000;
const DEFAULT_SLOTS_PER_FRAME: usize = 2;
const IEC61937_SYNC: [u8; 4] = [0x72, 0xf8, 0x1f, 0x4e];

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CarrierWordHalfArg {
    /// Reference SiI9437 path: useful IEC61937 word occupies bits 31..16.
    High,
    /// Alternate packing for bring-up only.
    Low,
}

impl From<CarrierWordHalfArg> for CarrierWordHalf {
    fn from(value: CarrierWordHalfArg) -> Self {
        match value {
            CarrierWordHalfArg::High => CarrierWordHalf::High,
            CarrierWordHalfArg::Low => CarrierWordHalf::Low,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "aurora-direct-earc-ingest",
    about = "Normalize stdin S32_LE direct-eARC carrier into canonical IEC61937 S16_LE"
)]
struct Args {
    /// Legacy option retained only to fail with a migration message. Native ALSA
    /// capture is owned by `aurora-encoded-runtime --alsa-device`.
    #[arg(long)]
    alsa_device: Option<String>,

    /// Recovered eARC carrier frame rate. Kept for capture-contract validation;
    /// stdin already contains sampled S32_LE data and is never resampled here.
    #[arg(long, default_value_t = DEFAULT_CARRIER_RATE_HZ)]
    carrier_rate: u32,

    /// Number of 32-bit carrier slots per frame. The proven SiI9437 path is two
    /// slots; wider TDM capture requires an explicit slot selector not provided
    /// by this normalization helper.
    #[arg(long, default_value_t = DEFAULT_SLOTS_PER_FRAME)]
    slots: usize,

    /// Select which 16-bit half of each S32_LE slot contains IEC61937.
    #[arg(long, value_enum, default_value_t = CarrierWordHalfArg::High)]
    word_half: CarrierWordHalfArg,

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

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let stats = process_stream(
        stdin.lock(),
        &mut output,
        args.slots,
        args.word_half.into(),
        args.read_bytes,
    )?;

    output.flush().context("failed to flush IEC61937 output")?;
    eprintln!(
        "aurora-direct-earc-ingest: frames={} words={} iec61937_sync_bursts={}",
        stats.carrier_frames, stats.output_words, stats.sync_bursts
    );
    Ok(())
}

fn validate_args(args: &Args) -> Result<()> {
    if args.alsa_device.is_some() {
        bail!(
            "--alsa-device is no longer supported by this helper; use aurora-encoded-runtime --input direct-earc --alsa-device <device> for native ALSA capture"
        );
    }
    if args.carrier_rate == 0 {
        bail!("carrier rate must be greater than zero");
    }
    if args.slots != DEFAULT_SLOTS_PER_FRAME {
        bail!(
            "direct eARC normalization currently requires exactly {} S32 slots; got {}. Wider TDM capture needs explicit carrier-slot selection",
            DEFAULT_SLOTS_PER_FRAME,
            args.slots
        );
    }
    if args.read_bytes == 0 {
        bail!("read size must be greater than zero");
    }
    Ok(())
}

fn process_stream<R: Read, W: Write>(
    mut input: R,
    mut output: W,
    slots: usize,
    word_half: CarrierWordHalf,
    read_bytes: usize,
) -> Result<IngestStats> {
    if read_bytes == 0 {
        bail!("read size must be greater than zero");
    }
    if slots != DEFAULT_SLOTS_PER_FRAME {
        bail!(
            "direct eARC normalization currently requires exactly {} S32 slots; got {}",
            DEFAULT_SLOTS_PER_FRAME,
            slots
        );
    }

    let mut normalizer = S32LeCarrierNormalizer::new(slots, word_half)?;
    let mut read_buffer = vec![0_u8; read_bytes];
    let mut scanner = Iec61937SyncScanner::default();

    loop {
        let count = input
            .read(&mut read_buffer)
            .context("failed reading direct eARC carrier")?;
        if count == 0 {
            break;
        }

        let normalized = normalizer.push(&read_buffer[..count]);
        if normalized.is_empty() {
            continue;
        }
        scanner.feed(&normalized);
        output
            .write_all(&normalized)
            .context("failed writing canonical IEC61937 bytes")?;
    }

    normalizer.finish()?;
    Ok(IngestStats {
        carrier_frames: normalizer.carrier_frames(),
        output_words: normalizer.output_words(),
        sync_bursts: scanner.sync_bursts,
    })
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

    fn valid_args() -> Args {
        Args {
            alsa_device: None,
            carrier_rate: DEFAULT_CARRIER_RATE_HZ,
            slots: DEFAULT_SLOTS_PER_FRAME,
            word_half: CarrierWordHalfArg::High,
            read_bytes: 16_384,
        }
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
    fn direct_ingest_rejects_arecord_and_unproven_slot_layouts() {
        let mut args = valid_args();
        args.alsa_device = Some("hw:0,0".to_owned());
        assert!(validate_args(&args).is_err());

        let mut args = valid_args();
        args.slots = 4;
        assert!(validate_args(&args).is_err());
        assert!(process_stream(
            Cursor::new(Vec::<u8>::new()),
            Vec::<u8>::new(),
            4,
            CarrierWordHalf::High,
            16,
        )
        .is_err());
    }

    #[test]
    fn sync_scanner_detects_preamble_across_chunk_boundary() {
        let mut scanner = Iec61937SyncScanner::default();
        scanner.feed(&[0xaa, 0x72, 0xf8]);
        scanner.feed(&[0x1f, 0x4e, 0xbb]);
        assert_eq!(scanner.sync_bursts, 1);
    }
}
