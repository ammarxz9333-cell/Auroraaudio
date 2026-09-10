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

/// Standard Pa-to-Pa period of a canonical E-AC-3 IEC61937 burst stream.
/// This is observational only: a mismatch is reported, never promoted to a
/// physical eARC unlock or JOC/Atmos failure by the probe.
const EAC3_IEC61937_PERIOD_BYTES: u64 = 24_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum FilterArg {
    All,
    Ac3,
    Eac3,
    Mat,
    Dts,
}

impl FilterArg {
    const fn accepts(self, data_type: u8) -> bool {
        match self {
            Self::All => true,
            Self::Ac3 => data_type == DATA_TYPE_AC3,
            Self::Eac3 => data_type == DATA_TYPE_EAC3,
            Self::Mat => data_type == DATA_TYPE_MAT,
            Self::Dts => matches!(data_type, 0x0B..=0x0D),
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
    /// Which IEC61937 transport bursts to include in selected counters/output.
    /// Transport continuity is still observed across every valid burst so a
    /// filtered-out codec cannot create a false E-AC-3 cadence comparison.
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
    previous_codec: Option<TransportCodec>,
    previous_carrier_offset: Option<u64>,
    eac3_last_spacing_bytes: Option<u64>,
    eac3_min_spacing_bytes: Option<u64>,
    eac3_max_spacing_bytes: Option<u64>,
    eac3_nominal_period_matches: u64,
    eac3_period_mismatches: u64,
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
    // Always parse all valid transport classes. Filtering happens after parsing
    // so a hidden AC-3/MAT/DTS burst still breaks E-AC-3 consecutive-cadence
    // comparison instead of producing a false mismatch across the hidden burst.
    let mut parser = BurstParser::new(CodecFilter::All);
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
            let selected = args.filter.accepts(observation.burst.data_type);
            observe_transport(
                &mut stats,
                observation.burst.codec,
                observation.carrier_offset_bytes,
                selected,
            );

            if selected {
                observe_selected_burst(&mut stats, observation.burst.codec);
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
    }

    // A diagnostic/extraction tool must not report success after silently
    // discarding a partial Pa/Pb header or declared payload. Ordinary idle
    // carrier padding remains accepted by BurstParser::finish().
    parser
        .finish()
        .context("IEC61937 stream ended on an incomplete burst")?;
    output.flush().context("failed to flush extracted payload")?;

    eprintln!(
        "aurora-direct-earc-probe: bursts={} eac3={} ac3={} mat={} dts={} other={} format_changes={} payload_bytes={} discarded_carrier_bytes={} malformed_headers={} pending_bytes={} eac3_nominal_period_bytes={} eac3_last_spacing_bytes={:?} eac3_min_spacing_bytes={:?} eac3_max_spacing_bytes={:?} eac3_nominal_period_matches={} eac3_period_mismatches={}",
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
        EAC3_IEC61937_PERIOD_BYTES,
        stats.eac3_last_spacing_bytes,
        stats.eac3_min_spacing_bytes,
        stats.eac3_max_spacing_bytes,
        stats.eac3_nominal_period_matches,
        stats.eac3_period_mismatches,
    );

    Ok(())
}

fn observe_selected_burst(stats: &mut ProbeStats, codec: TransportCodec) {
    stats.bursts = stats.bursts.saturating_add(1);
    match codec {
        TransportCodec::Ac3 => stats.ac3 = stats.ac3.saturating_add(1),
        TransportCodec::Eac3 => stats.eac3 = stats.eac3.saturating_add(1),
        TransportCodec::MatTrueHd => stats.mat = stats.mat.saturating_add(1),
        TransportCodec::DtsCore => stats.dts = stats.dts.saturating_add(1),
        TransportCodec::Other(_) => stats.other = stats.other.saturating_add(1),
    }
}

fn observe_transport(
    stats: &mut ProbeStats,
    codec: TransportCodec,
    carrier_offset_bytes: u64,
    selected: bool,
) {
    // Compare only physically consecutive E-AC-3 bursts. Every transport class
    // updates previous_codec/offset even when filtered from the selected counts.
    if selected && codec == TransportCodec::Eac3 && stats.previous_codec == Some(TransportCodec::Eac3) {
        if let Some(previous_offset) = stats.previous_carrier_offset {
            let spacing = carrier_offset_bytes.saturating_sub(previous_offset);
            stats.eac3_last_spacing_bytes = Some(spacing);
            stats.eac3_min_spacing_bytes = Some(
                stats
                    .eac3_min_spacing_bytes
                    .map_or(spacing, |current| current.min(spacing)),
            );
            stats.eac3_max_spacing_bytes = Some(
                stats
                    .eac3_max_spacing_bytes
                    .map_or(spacing, |current| current.max(spacing)),
            );
            if spacing == EAC3_IEC61937_PERIOD_BYTES {
                stats.eac3_nominal_period_matches =
                    stats.eac3_nominal_period_matches.saturating_add(1);
            } else {
                stats.eac3_period_mismatches = stats.eac3_period_mismatches.saturating_add(1);
            }
        }
    }
    stats.previous_codec = Some(codec);
    stats.previous_carrier_offset = Some(carrier_offset_bytes);
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
    fn filter_matches_transport_data_types_without_hiding_parser_continuity() {
        assert!(FilterArg::All.accepts(DATA_TYPE_EAC3));
        assert!(FilterArg::Eac3.accepts(DATA_TYPE_EAC3));
        assert!(!FilterArg::Eac3.accepts(DATA_TYPE_AC3));
        assert!(FilterArg::Dts.accepts(0x0B));
        assert!(FilterArg::Dts.accepts(0x0D));
        assert!(!FilterArg::Dts.accepts(DATA_TYPE_MAT));
    }

    #[test]
    fn telemetry_keeps_eac3_as_transport_only_classification() {
        let mut stats = ProbeStats::default();
        observe_transport(&mut stats, TransportCodec::Eac3, 0, true);
        observe_selected_burst(&mut stats, TransportCodec::Eac3);
        assert_eq!(stats.eac3, 1);
        assert_eq!(stats.bursts, 1);
        assert_eq!(stats.eac3_last_spacing_bytes, None);
    }

    #[test]
    fn consecutive_eac3_cadence_counts_exact_nominal_and_mismatch_periods() {
        let mut stats = ProbeStats::default();
        observe_transport(&mut stats, TransportCodec::Eac3, 0, true);
        observe_transport(
            &mut stats,
            TransportCodec::Eac3,
            EAC3_IEC61937_PERIOD_BYTES,
            true,
        );
        observe_transport(
            &mut stats,
            TransportCodec::Eac3,
            EAC3_IEC61937_PERIOD_BYTES * 2 + 8,
            true,
        );

        assert_eq!(stats.eac3_nominal_period_matches, 1);
        assert_eq!(stats.eac3_period_mismatches, 1);
        assert_eq!(stats.eac3_last_spacing_bytes, Some(24_584));
        assert_eq!(stats.eac3_min_spacing_bytes, Some(24_576));
        assert_eq!(stats.eac3_max_spacing_bytes, Some(24_584));
    }

    #[test]
    fn filtered_non_eac3_burst_still_breaks_eac3_cadence_comparison() {
        let mut stats = ProbeStats::default();
        observe_transport(&mut stats, TransportCodec::Eac3, 0, true);
        observe_transport(&mut stats, TransportCodec::Ac3, 24_576, false);
        observe_transport(&mut stats, TransportCodec::Eac3, 30_000, true);

        assert_eq!(stats.eac3_nominal_period_matches, 0);
        assert_eq!(stats.eac3_period_mismatches, 0);
        assert_eq!(stats.eac3_last_spacing_bytes, None);
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
