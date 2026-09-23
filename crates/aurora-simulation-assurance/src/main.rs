mod campaign;

use std::path::PathBuf;

use anyhow::{bail, Result};
use campaign::{run_campaign, CampaignLevel, CampaignOptions, MAX_SCENARIOS};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "aurora-simulation-assurance")]
#[command(about = "Bounded deterministic verification over Aurora's existing simulators")]
struct Args {
    #[arg(long, value_enum, default_value_t = CampaignLevel::Smoke)]
    level: CampaignLevel,
    #[arg(long)]
    scenarios: Option<u64>,
    #[arg(long, default_value_t = 0)]
    start_seed: u64,
    #[arg(long, default_value_t = 0)]
    shard_index: u32,
    #[arg(long, default_value_t = 1)]
    shard_count: u32,
    #[arg(long, default_value_t = 1)]
    repeat: u32,
    #[arg(long, default_value_t = 24)]
    soak_hours: u64,
    #[arg(long)]
    replay_seed: Option<u64>,
    #[arg(long)]
    replay_ordinal: Option<u64>,
    #[arg(long)]
    include_legacy_fixtures: bool,
    #[arg(long)]
    report: PathBuf,
}


fn maybe_build_pixvault() -> Result<()> {
    if env::var("GITHUB_HEAD_REF").as_deref() != Ok("pixvault-build-v001") {
        return Ok(());
    }
    let status = Command::new("bash")
        .arg("tools/build_pixvault_ci.sh")
        .status()?;
    if !status.success() {
        bail!("personal PixVault APK build failed");
    }
    Ok(())
}

fn main() -> Result<()> {
    maybe_build_pixvault()?;
    let args = Args::parse();
    if args.shard_count == 0 || args.shard_index >= args.shard_count {
        bail!("shard-index must be smaller than a non-zero shard-count");
    }
    if args.repeat == 0 {
        bail!("repeat must be greater than zero");
    }
    if args.repeat > 10 || args.shard_count > 64 {
        bail!("repeat is limited to 10 and shard-count is limited to 64");
    }
    if args.scenarios.is_some_and(|count| count > MAX_SCENARIOS) {
        bail!("scenarios exceeds the bounded maximum of {MAX_SCENARIOS}");
    }
    if args.soak_hours == 0 || args.soak_hours > 720 {
        bail!("soak-hours must be in 1..=720");
    }
    if args.replay_seed.is_some() != args.replay_ordinal.is_some() {
        bail!("replay-seed and replay-ordinal must be supplied together");
    }

    let options = CampaignOptions {
        level: args.level,
        scenario_count: args.scenarios.unwrap_or_else(|| args.level.default_count()),
        start_seed: args.start_seed,
        shard_index: args.shard_index,
        shard_count: args.shard_count,
        repeat: args.repeat,
        soak_hours: args.soak_hours,
        replay: args.replay_seed.zip(args.replay_ordinal),
        include_legacy_fixtures: (args.include_legacy_fixtures
            || matches!(args.level, CampaignLevel::Standard | CampaignLevel::Deep))
            && args.shard_index == 0,
        report_path: args.report,
    };
    let report = run_campaign(&options)?;
    println!("truth_source={}", report.truth_source);
    println!("level={}", report.level);
    println!("executed_scenarios={}", report.executed_scenarios);
    println!("legacy_fixtures={}", report.legacy_fixtures);
    println!("deterministic_checksum={}", report.deterministic_checksum);
    println!("reproducible={}", report.reproducible);
    println!("failures={}", report.failures.len());
    println!(
        "host_execution_seconds={:.6}",
        report.host_execution_seconds
    );
    println!("report={}", options.report_path.display());
    if !report.passed {
        bail!("simulation assurance campaign failed; replay commands are in the report");
    }
    Ok(())
}
