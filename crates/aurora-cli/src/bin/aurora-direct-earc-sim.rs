//! Convert a raw finite E-AC-3 elementary stream into Aurora's canonical IEC61937 carrier.
//!
//! Input is read from stdin and framed with the exact OpenJOC revision pinned by
//! `aurora-sim-source`. Each proven E-AC-3 access unit is emitted as one fixed
//! 24,576-byte type-0x15 carrier period. Deterministic edge-case injection is
//! intentionally implemented outside Aurora's production parser/decoder.

use std::io::{self, Read, Write};

use anyhow::{Context, Result, bail};
use aurora_sim_source::{
    CarrierFault, EAC3_BURST_PERIOD_BYTES, EAC3_CARRIER_BYTES_PER_MS, Eac3AccessUnitFramer,
    inject_carrier_fault, write_eac3_period, write_idle_period,
};
use clap::Parser;

const IEC61937_HEADER_BYTES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimaryFault {
    CutBurst { period: usize },
    CorruptPa { period: usize },
    Gap { start: usize, count: usize },
    CodecSwitch { period: usize },
    TruncatedEof,
}

impl PrimaryFault {
    fn completion_period(self) -> Option<usize> {
        match self {
            Self::CutBurst { period }
            | Self::CorruptPa { period }
            | Self::CodecSwitch { period } => Some(period),
            Self::Gap { start, count } => start.checked_add(count.saturating_sub(1)),
            Self::TruncatedEof => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FaultPlan {
    primary: Option<PrimaryFault>,
    cadence_jitter_bytes: usize,
}

#[derive(Debug, Parser)]
#[command(
    name = "aurora-sim-source",
    about = "Wrap raw E-AC-3 access units into canonical IEC61937 carrier periods and inject deterministic faults"
)]
struct Args {
    /// Internal stdin read size. E-AC-3 access-unit framing may cross any read boundary.
    #[arg(long, default_value_t = 16_384)]
    read_bytes: usize,

    /// Cut zero-based burst N in the middle of its declared encoded payload.
    #[arg(long)]
    cut_burst: Option<usize>,

    /// Corrupt Pa in zero-based burst N. The next valid Pa/Pb must resynchronize.
    #[arg(long)]
    corrupt_pa: Option<usize>,

    /// Drop M E-AC-3 bursts starting at zero-based N while preserving their carrier time as idle periods.
    #[arg(long, num_args = 2, value_names = ["N", "M"])]
    gap: Option<Vec<usize>>,

    /// Replace zero-based burst N with one non-IEC61937 idle/silence period, then return to E-AC-3.
    /// This models the encoded parser's view of an E-AC-3 -> LPCM source interval; it is not a
    /// full LPCM decoder/path proof.
    #[arg(long)]
    codec_switch: Option<usize>,

    /// Add deterministic late-burst jitter in milliseconds before every odd-numbered period.
    /// File-mode jitter changes carrier spacing; it does not sleep the process.
    #[arg(long = "cadence-jitter")]
    cadence_jitter_ms: Option<usize>,

    /// End the generated carrier in the middle of the final AU's declared payload.
    #[arg(long)]
    truncated_eof: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.read_bytes == 0 {
        bail!("read size must be greater than zero");
    }
    let plan = parse_fault_plan(&args)?;

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut read_buffer = vec![0_u8; args.read_bytes];
    let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut framer = Eac3AccessUnitFramer::new();

    // Targeted faults stage output until the complete requested target range has
    // actually been generated. A nonexistent target therefore leaves redirected
    // stdout empty rather than a misleading partial carrier capture.
    let completion_period = plan.primary.and_then(PrimaryFault::completion_period);
    let mut staged_output = completion_period.map(|_| Vec::new());
    let mut pending_eof_unit: Option<Vec<u8>> = None;
    let mut input_bytes = 0_u64;
    let mut output_bytes = 0_u64;
    let mut access_units = 0_usize;
    let mut fault_complete = completion_period.is_none();

    loop {
        let count = input
            .read(&mut read_buffer)
            .context("failed reading raw E-AC-3 elementary stream")?;
        if count == 0 {
            break;
        }
        input_bytes = input_bytes.saturating_add(count as u64);

        let units = framer
            .push(&read_buffer[..count])
            .map_err(|error| anyhow::anyhow!("E-AC-3 access-unit framing failed: {error}"))?;
        for unit in units {
            if plan.primary == Some(PrimaryFault::TruncatedEof) {
                if let Some(previous) = pending_eof_unit.replace(unit) {
                    output_bytes = output_bytes.saturating_add(emit_stream_period(
                        &previous,
                        access_units,
                        &mut period,
                        plan,
                        &mut staged_output,
                        &mut output,
                    )? as u64);
                    access_units = access_units.saturating_add(1);
                }
            } else {
                output_bytes = output_bytes.saturating_add(emit_stream_period(
                    &unit,
                    access_units,
                    &mut period,
                    plan,
                    &mut staged_output,
                    &mut output,
                )? as u64);
                access_units = access_units.saturating_add(1);
                if completion_period == access_units.checked_sub(1) {
                    fault_complete = true;
                }
            }
        }
    }

    for unit in framer
        .finish()
        .map_err(|error| anyhow::anyhow!("finite E-AC-3 stream finalization failed: {error}"))?
    {
        if plan.primary == Some(PrimaryFault::TruncatedEof) {
            if let Some(previous) = pending_eof_unit.replace(unit) {
                output_bytes = output_bytes.saturating_add(emit_stream_period(
                    &previous,
                    access_units,
                    &mut period,
                    plan,
                    &mut staged_output,
                    &mut output,
                )? as u64);
                access_units = access_units.saturating_add(1);
            }
        } else {
            output_bytes = output_bytes.saturating_add(emit_stream_period(
                &unit,
                access_units,
                &mut period,
                plan,
                &mut staged_output,
                &mut output,
            )? as u64);
            access_units = access_units.saturating_add(1);
            if completion_period == access_units.checked_sub(1) {
                fault_complete = true;
            }
        }
    }

    if plan.primary == Some(PrimaryFault::TruncatedEof) {
        let final_unit = pending_eof_unit
            .take()
            .ok_or_else(|| anyhow::anyhow!("--truncated-eof requires at least one complete E-AC-3 AU"))?;
        write_eac3_period(&final_unit, &mut period)
            .map_err(|error| anyhow::anyhow!("failed building final E-AC-3 carrier period: {error}"))?;
        maybe_write_jitter(access_units, plan, &mut output, &mut output_bytes)?;
        let truncate_at = mid_payload_cut_length(final_unit.len());
        let truncated = inject_carrier_fault(&period, CarrierFault::Truncate { length: truncate_at })
            .map_err(|error| anyhow::anyhow!("failed truncating final carrier period: {error}"))?;
        output
            .write_all(&truncated)
            .context("failed writing truncated final IEC61937 carrier")?;
        output_bytes = output_bytes.saturating_add(truncated.len() as u64);
        access_units = access_units.saturating_add(1);
        fault_complete = true;
    }

    if completion_period.is_some() && !fault_complete {
        bail!(
            "requested fault target was not fully generated; produced {access_units} carrier period(s)"
        );
    }

    output
        .flush()
        .context("failed flushing generated IEC61937 carrier")?;
    eprintln!(
        "aurora-sim-source: input_bytes={input_bytes} access_units={access_units} nominal_period_bytes={EAC3_BURST_PERIOD_BYTES} output_bytes={output_bytes} fault={:?} cadence_jitter_bytes={} fault_complete={fault_complete}",
        plan.primary,
        plan.cadence_jitter_bytes,
    );
    Ok(())
}

fn parse_fault_plan(args: &Args) -> Result<FaultPlan> {
    let mut primary = None;
    let mut select = |candidate: Option<PrimaryFault>| -> Result<()> {
        if let Some(candidate) = candidate {
            if primary.replace(candidate).is_some() {
                bail!("only one of --cut-burst, --corrupt-pa, --gap, --codec-switch or --truncated-eof may be used at a time");
            }
        }
        Ok(())
    };

    select(args.cut_burst.map(|period| PrimaryFault::CutBurst { period }))?;
    select(
        args.corrupt_pa
            .map(|period| PrimaryFault::CorruptPa { period }),
    )?;
    if let Some(values) = args.gap.as_ref() {
        if values.len() != 2 || values[1] == 0 {
            bail!("--gap requires N M with M greater than zero");
        }
        select(Some(PrimaryFault::Gap {
            start: values[0],
            count: values[1],
        }))?;
    }
    select(
        args.codec_switch
            .map(|period| PrimaryFault::CodecSwitch { period }),
    )?;
    if args.truncated_eof {
        select(Some(PrimaryFault::TruncatedEof))?;
    }

    let cadence_jitter_bytes = args
        .cadence_jitter_ms
        .unwrap_or(0)
        .checked_mul(EAC3_CARRIER_BYTES_PER_MS)
        .ok_or_else(|| anyhow::anyhow!("cadence jitter byte count overflow"))?;

    Ok(FaultPlan {
        primary,
        cadence_jitter_bytes,
    })
}

fn mid_payload_cut_length(payload_bytes: usize) -> usize {
    let carrier_payload = payload_bytes.saturating_add(payload_bytes & 1);
    IEC61937_HEADER_BYTES + carrier_payload.max(2) / 2
}

fn maybe_write_jitter<W: Write>(
    period_index: usize,
    plan: FaultPlan,
    output: &mut W,
    output_bytes: &mut u64,
) -> Result<()> {
    if period_index > 0 && period_index % 2 == 1 && plan.cadence_jitter_bytes > 0 {
        const ZERO_CHUNK: [u8; 4096] = [0; 4096];
        let mut remaining = plan.cadence_jitter_bytes;
        while remaining > 0 {
            let count = remaining.min(ZERO_CHUNK.len());
            output
                .write_all(&ZERO_CHUNK[..count])
                .context("failed writing deterministic cadence jitter")?;
            *output_bytes = output_bytes.saturating_add(count as u64);
            remaining -= count;
        }
    }
    Ok(())
}

fn emit_stream_period<W: Write>(
    access_unit: &[u8],
    period_index: usize,
    period: &mut [u8; EAC3_BURST_PERIOD_BYTES],
    plan: FaultPlan,
    staged_output: &mut Option<Vec<u8>>,
    output: &mut W,
) -> Result<usize> {
    write_eac3_period(access_unit, period)
        .map_err(|error| anyhow::anyhow!("failed building E-AC-3 carrier period: {error}"))?;

    let mut emitted: Vec<u8> = Vec::new();
    let bytes: &[u8] = match plan.primary {
        Some(PrimaryFault::CutBurst { period: target }) if target == period_index => {
            emitted = inject_carrier_fault(
                period,
                CarrierFault::Truncate {
                    length: mid_payload_cut_length(access_unit.len()),
                },
            )
            .map_err(|error| anyhow::anyhow!("failed cutting carrier burst: {error}"))?;
            &emitted
        }
        Some(PrimaryFault::CorruptPa { period: target }) if target == period_index => {
            emitted = inject_carrier_fault(period, CarrierFault::CorruptPa)
                .map_err(|error| anyhow::anyhow!("failed corrupting Pa: {error}"))?;
            &emitted
        }
        Some(PrimaryFault::Gap { start, count })
            if period_index >= start && period_index < start.saturating_add(count) =>
        {
            write_idle_period(period)
                .map_err(|error| anyhow::anyhow!("failed writing gap idle period: {error}"))?;
            period
        }
        Some(PrimaryFault::CodecSwitch { period: target }) if target == period_index => {
            // An encoded-only IEC61937 parser cannot decode LPCM. One full carrier
            // interval of zero PCM/silence therefore represents the parser-visible
            // non-IEC interval while preserving time before E-AC-3 returns.
            write_idle_period(period)
                .map_err(|error| anyhow::anyhow!("failed writing LPCM switch span: {error}"))?;
            period
        }
        _ => period,
    };

    let destination: &mut dyn Write = if let Some(staged) = staged_output.as_mut() {
        staged
    } else {
        output
    };
    let mut jitter_bytes = 0_u64;
    maybe_write_jitter(period_index, plan, destination, &mut jitter_bytes)?;
    destination
        .write_all(bytes)
        .context("failed writing generated IEC61937 carrier")?;

    if plan.primary.and_then(PrimaryFault::completion_period) == Some(period_index) {
        if let Some(staged) = staged_output.take() {
            output
                .write_all(&staged)
                .context("failed committing staged fault-injected IEC61937 carrier")?;
        }
    }

    usize::try_from(jitter_bytes)
        .unwrap_or(usize::MAX)
        .checked_add(bytes.len())
        .ok_or_else(|| anyhow::anyhow!("simulator output byte count overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> Args {
        Args {
            read_bytes: 4096,
            cut_burst: None,
            corrupt_pa: None,
            gap: None,
            codec_switch: None,
            cadence_jitter_ms: None,
            truncated_eof: false,
        }
    }

    #[test]
    fn fault_modes_are_mutually_exclusive() {
        let mut args = base_args();
        args.cut_burst = Some(1);
        args.corrupt_pa = Some(2);
        assert!(parse_fault_plan(&args).is_err());
    }

    #[test]
    fn gap_requires_positive_count() {
        let mut args = base_args();
        args.gap = Some(vec![3, 0]);
        assert!(parse_fault_plan(&args).is_err());
    }

    #[test]
    fn cadence_jitter_converts_ms_to_canonical_carrier_bytes() {
        let mut args = base_args();
        args.cadence_jitter_ms = Some(5);
        let plan = parse_fault_plan(&args).unwrap();
        assert_eq!(plan.cadence_jitter_bytes, 5 * EAC3_CARRIER_BYTES_PER_MS);
    }

    #[test]
    fn cut_length_is_inside_declared_payload() {
        let payload = 128;
        let cut = mid_payload_cut_length(payload);
        assert!(cut > IEC61937_HEADER_BYTES);
        assert!(cut < IEC61937_HEADER_BYTES + payload);
    }

    #[test]
    fn gap_replaces_selected_period_with_full_idle_time() {
        let payload = [0x0B_u8, 0x77, 0xAA, 0x55];
        let plan = FaultPlan {
            primary: Some(PrimaryFault::Gap { start: 0, count: 1 }),
            cadence_jitter_bytes: 0,
        };
        let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];
        let mut staged = Some(Vec::new());
        let mut visible = Vec::new();
        let written = emit_stream_period(
            &payload,
            0,
            &mut period,
            plan,
            &mut staged,
            &mut visible,
        )
        .unwrap();

        assert_eq!(written, EAC3_BURST_PERIOD_BYTES);
        assert!(staged.is_none());
        assert_eq!(visible.len(), EAC3_BURST_PERIOD_BYTES);
        assert!(visible.iter().all(|byte| *byte == 0));
    }
}
