//! Inspect canonical IEC61937 carrier bytes from Aurora's direct-eARC ingest.
//!
//! This binary deliberately stops at the codec boundary. It classifies transport
//! bursts, reports source-format changes, and can emit the untouched elementary
//! stream payload for the next decoder stage. E-AC-3 type 0x15 is not labelled
//! JOC here; JOC/object presence must be established by a decoder.

use std::io::{self, Read, Write};

use anyhow::{Context, Result};
use aurora_iec61937::{
    BurstParser, CodecFilter, TransportCodec, DATA_TYPE_AC3, DATA_TYPE_EAC3, DATA_TYPE_MAT,
};
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum FilterArg {
    All,
    Ac3,
    Eac3,
    Mat,
    Dts,
}

impl From<FilterArg> for CodecFilter {
    fn from(value: FilterArg) -> Self {
        match value {
            FilterArg::All => CodecFilter::All,
            FilterArg::Ac3 => CodecFilter::Ac3,
            FilterArg::Eac3 => CodecFilter::Eac3,
            FilterArg::Mat => CodecFilter::MatTrueHd,
            FilterArg::Dts => CodecFilter::DtsCore,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ExtractArg {
    None,
    All,
    Eac3,
}

#[derive(Debug, Parser)]
#[command(
    name = "aurora-direct-earc-probe",
    about = "Classify direct-eARC IEC61937 bursts and optionally extract codec payloads"
)]
struct Args {
    /// Which IEC61937 transport bursts to admit to telemetry.
    #[arg(long, value_enum, default_value_t = FilterArg::All)]
    filter: FilterArg,

    /// Emit selected native codec payload bytes to stdout.
    #[arg(long, value_enum, default_value_t = ExtractArg::None)]
    extract: ExtractArg,

    /// Internal stdin read size. Burst framing may cross any read boundary.
    #[arg(long, default_value_t = 16_384)]
    read_bytes: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct ProbeStats {
    bursts: u64,
    ac3: u64,
    eac3: u64,
    mat: u64,
    dts: u64,
    other: u64,
    format_changes: u64,
    payload_bytes_emitted: u64,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.read_bytes == 0 {
        anyhow::bail!("read size must be greater than zero");
    }

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut parser = BurstParser::new(args.filter.into());
    let mut stats = ProbeStats::default();
    let mut read_buffer = vec![0_u8; args.read_bytes];

    loop {
        let count = input
            .read(&mut read_buffer)
            .context("failed reading canonical IEC61937 carrier")?;
        if count == 0 {
            break;
        }

        for observation in parser.push(&read_buffer[..count]) {
            observe_burst(&mut stats, observation.burst.codec);

            if let Some(change) = observation.format_change {
                stats.format_changes = stats.format_changes.saturating_add(1);
                eprintln!(
                    "aurora-direct-earc-probe: format-change {:?} -> {:?}",
                    change.previous, change.current
                );
            }

            if should_extract(args.extract, observation.burst.data_type) {
                output
                    .write_all(&observation.burst.payload)
                    .context("failed writing extracted codec payload")?;
                stats.payload_bytes_emitted = stats
                    .payload_bytes_emitted
                    .saturating_add(observation.burst.payload.len() as u64);
            }
        }
    }

    // A diagnostic/extraction tool must not report success after silently
    // discarding a partial Pa/Pb header or declared payload. Ordinary idle
    // carrier padding remains accepted by BurstParser::finish().
    parser
        .finish()
        .context("IEC61937 stream ended on an incomplete burst")?;
    output.flush().context("failed to flush extracted payload")?;

    eprintln!(
        "aurora-direct-earc-probe: bursts={} eac3={} ac3={} mat={} dts={} other={} format_changes={} payload_bytes={} discarded_carrier_bytes={} malformed_headers={} pending_bytes={}",
        stats.bursts,
        stats.eac3,
        stats.ac3,
        stats.mat,
        stats.dts,
        stats.other,
        stats.format_changes,
        stats.payload_bytes_emitted,
        parser.discarded_bytes(),
        parser.malformed_headers(),
        parser.pending_bytes(),
    );

    Ok(())
}

fn observe_burst(stats: &mut ProbeStats, codec: TransportCodec) {
    stats.bursts = stats.bursts.saturating_add(1);
    match codec {
        TransportCodec::Ac3 => stats.ac3 = stats.ac3.saturating_add(1),
        TransportCodec::Eac3 => stats.eac3 = stats.eac3.saturating_add(1),
        TransportCodec::MatTrueHd => stats.mat = stats.mat.saturating_add(1),
        TransportCodec::DtsCore => stats.dts = stats.dts.saturating_add(1),
        TransportCodec::Other(_) => stats.other = stats.other.saturating_add(1),
    }
}

fn should_extract(mode: ExtractArg, data_type: u8) -> bool {
    match mode {
        ExtractArg::None => false,
        ExtractArg::All => true,
        ExtractArg::Eac3 => data_type == DATA_TYPE_EAC3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_mode_never_mistakes_ac3_or_mat_for_eac3() {
        assert!(should_extract(ExtractArg::Eac3, DATA_TYPE_EAC3));
        assert!(!should_extract(ExtractArg::Eac3, DATA_TYPE_AC3));
        assert!(!should_extract(ExtractArg::Eac3, DATA_TYPE_MAT));
    }

    #[test]
    fn telemetry_keeps_eac3_as_transport_only_classification() {
        let mut stats = ProbeStats::default();
        observe_burst(&mut stats, TransportCodec::Eac3);
        assert_eq!(stats.eac3, 1);
        assert_eq!(stats.bursts, 1);
    }

    #[test]
    fn parser_eof_contract_rejects_partial_preamble_and_accepts_padding() {
        let mut partial = BurstParser::new(CodecFilter::All);
        assert!(partial.push(&[0x72, 0xF8]).is_empty());
        assert!(partial.finish().is_err());

        let mut padding = BurstParser::new(CodecFilter::All);
        assert!(padding.push(&[0, 0, 0, 0, 0]).is_empty());
        padding.finish().unwrap();
        assert_eq!(padding.pending_bytes(), 0);
    }
}
