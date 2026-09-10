//! Convert a raw finite E-AC-3 elementary stream into Aurora's canonical IEC61937 carrier.
//!
//! Input is read from stdin and framed with the exact OpenJOC revision pinned by
//! `aurora-sim-source`. Each proven E-AC-3 access unit is emitted as one fixed
//! 24,576-byte type-0x15 carrier period. An optional deterministic byte-deletion
//! fault can be applied to exactly one generated period.

use std::io::{self, Read, Write};

use anyhow::{Context, Result, bail};
use aurora_sim_source::{
    CarrierFault, EAC3_BURST_PERIOD_BYTES, Eac3AccessUnitFramer, inject_carrier_fault,
    write_eac3_period,
};
use clap::Parser;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeleteFaultSpec {
    period: usize,
    offset: usize,
    count: usize,
}

#[derive(Debug, Parser)]
#[command(
    name = "aurora-direct-earc-sim",
    about = "Wrap raw E-AC-3 access units into canonical IEC61937 eARC carrier periods"
)]
struct Args {
    /// Internal stdin read size. E-AC-3 access-unit framing may cross any read boundary.
    #[arg(long, default_value_t = 16_384)]
    read_bytes: usize,

    /// Zero-based generated carrier-period index on which to inject byte deletion.
    #[arg(long)]
    delete_period: Option<usize>,

    /// Byte offset within the selected 24,576-byte carrier period at which deletion starts.
    #[arg(long)]
    delete_offset: Option<usize>,

    /// Number of carrier bytes to delete from the selected period.
    #[arg(long)]
    delete_count: Option<usize>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.read_bytes == 0 {
        bail!("read size must be greater than zero");
    }
    let fault = parse_fault(&args)?;

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    let mut read_buffer = vec![0_u8; args.read_bytes];
    let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];
    let mut framer = Eac3AccessUnitFramer::new();

    let mut input_bytes = 0_u64;
    let mut output_bytes = 0_u64;
    let mut access_units = 0_usize;
    let mut fault_applied = false;

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
            let (written, applied) = emit_period(
                &unit,
                access_units,
                &mut period,
                fault,
                &mut output,
            )?;
            output_bytes = output_bytes.saturating_add(written as u64);
            fault_applied |= applied;
            access_units = access_units.saturating_add(1);
        }
    }

    for unit in framer
        .finish()
        .map_err(|error| anyhow::anyhow!("finite E-AC-3 stream finalization failed: {error}"))?
    {
        let (written, applied) = emit_period(
            &unit,
            access_units,
            &mut period,
            fault,
            &mut output,
        )?;
        output_bytes = output_bytes.saturating_add(written as u64);
        fault_applied |= applied;
        access_units = access_units.saturating_add(1);
    }

    if fault.is_some() && !fault_applied {
        bail!(
            "requested delete-period was not generated; produced {access_units} carrier period(s)"
        );
    }

    output
        .flush()
        .context("failed flushing generated IEC61937 carrier")?;
    eprintln!(
        "aurora-direct-earc-sim: input_bytes={input_bytes} access_units={access_units} nominal_period_bytes={EAC3_BURST_PERIOD_BYTES} output_bytes={output_bytes} fault_applied={fault_applied}"
    );
    Ok(())
}

fn parse_fault(args: &Args) -> Result<Option<DeleteFaultSpec>> {
    match (args.delete_period, args.delete_offset, args.delete_count) {
        (None, None, None) => Ok(None),
        (Some(period), Some(offset), Some(count)) if count > 0 => Ok(Some(DeleteFaultSpec {
            period,
            offset,
            count,
        })),
        (Some(_), Some(_), Some(0)) => bail!("delete-count must be greater than zero"),
        _ => bail!("delete-period, delete-offset and delete-count must be supplied together"),
    }
}

fn emit_period<W: Write>(
    access_unit: &[u8],
    period_index: usize,
    period: &mut [u8; EAC3_BURST_PERIOD_BYTES],
    fault: Option<DeleteFaultSpec>,
    output: &mut W,
) -> Result<(usize, bool)> {
    write_eac3_period(access_unit, period)
        .map_err(|error| anyhow::anyhow!("failed building E-AC-3 carrier period: {error}"))?;

    if let Some(spec) = fault.filter(|spec| spec.period == period_index) {
        let damaged = inject_carrier_fault(
            period,
            CarrierFault::DeleteBytes {
                offset: spec.offset,
                count: spec.count,
            },
        )
        .map_err(|error| anyhow::anyhow!("failed applying carrier fault: {error}"))?;
        output
            .write_all(&damaged)
            .context("failed writing fault-injected IEC61937 carrier")?;
        Ok((damaged.len(), true))
    } else {
        output
            .write_all(period)
            .context("failed writing IEC61937 carrier")?;
        Ok((period.len(), false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(
        delete_period: Option<usize>,
        delete_offset: Option<usize>,
        delete_count: Option<usize>,
    ) -> Args {
        Args {
            read_bytes: 4096,
            delete_period,
            delete_offset,
            delete_count,
        }
    }

    #[test]
    fn deletion_fault_requires_all_three_coordinates() {
        assert!(parse_fault(&args(None, None, None)).unwrap().is_none());
        assert!(parse_fault(&args(Some(0), None, None)).is_err());
        assert!(parse_fault(&args(Some(0), Some(10), None)).is_err());
        assert!(parse_fault(&args(None, Some(10), Some(2))).is_err());
    }

    #[test]
    fn deletion_fault_rejects_zero_count() {
        assert!(parse_fault(&args(Some(3), Some(10), Some(0))).is_err());
    }

    #[test]
    fn emit_period_applies_fault_only_to_selected_period() {
        let payload = [0x0B_u8, 0x77, 0xAA, 0x55];
        let fault = Some(DeleteFaultSpec {
            period: 1,
            offset: 10,
            count: 2,
        });
        let mut period = [0_u8; EAC3_BURST_PERIOD_BYTES];

        let mut first = Vec::new();
        let (first_bytes, first_applied) =
            emit_period(&payload, 0, &mut period, fault, &mut first).unwrap();
        assert_eq!(first_bytes, EAC3_BURST_PERIOD_BYTES);
        assert!(!first_applied);

        let mut second = Vec::new();
        let (second_bytes, second_applied) =
            emit_period(&payload, 1, &mut period, fault, &mut second).unwrap();
        assert_eq!(second_bytes, EAC3_BURST_PERIOD_BYTES - 2);
        assert!(second_applied);
    }
}
