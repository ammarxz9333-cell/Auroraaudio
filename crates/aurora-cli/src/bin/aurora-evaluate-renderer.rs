use std::path::PathBuf;

use anyhow::{bail, Result};
use aurora_cli::evaluation::{evaluate_renderers, EvaluationOptions, RendererTarget};
use clap::{Parser, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "aurora-evaluate-renderer")]
#[command(about = "Deterministic Aurora renderer evaluation and artifact runner")]
struct Cli {
    #[arg(long, value_enum, default_value_t = RendererSelection::All)]
    renderer: RendererSelection,
    #[arg(long, default_value = "output/evaluation")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 2.0)]
    duration_seconds: f64,
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,
    #[arg(long, default_value_t = 256)]
    block_size: usize,
    #[arg(long, default_value_t = 0.15)]
    max_gain_step: f32,
    #[arg(long, default_value_t = 64.0)]
    max_delay_step_samples: f32,
    #[arg(long, default_value_t = 0.05)]
    normalization_tolerance: f32,
    #[arg(long, default_value_t = 1_000.0)]
    max_p95_us: f64,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RendererSelection {
    All,
    GeometricBinaural,
    InverseDistance,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let targets = targets(cli.renderer);
    let options = EvaluationOptions {
        output_dir: cli.output_dir,
        duration_seconds: cli.duration_seconds,
        sample_rate: cli.sample_rate,
        block_size: cli.block_size,
        max_gain_step: cli.max_gain_step,
        max_delay_step_samples: cli.max_delay_step_samples,
        normalization_tolerance: cli.normalization_tolerance,
        max_p95_us: cli.max_p95_us,
    };
    let command = reproducible_command();
    let run = evaluate_renderers(&targets, &options, &command)?;
    print_run(&run);
    if !run.passed() {
        bail!(
            "renderer evaluation failed for: {}",
            run.failed_renderers().join(", ")
        );
    }
    Ok(())
}

fn targets(selection: RendererSelection) -> Vec<RendererTarget> {
    match selection {
        RendererSelection::All => vec![
            RendererTarget::GeometricBinaural,
            RendererTarget::InverseDistance,
        ],
        RendererSelection::GeometricBinaural => vec![RendererTarget::GeometricBinaural],
        RendererSelection::InverseDistance => vec![RendererTarget::InverseDistance],
    }
}

fn print_run(run: &aurora_cli::evaluation::EvaluationRun) {
    for summary in &run.summaries {
        println!(
            "renderer={} passed={} p95_us={:.3} max_gain_step={:.6} max_delay_step_samples={:.6}",
            summary.renderer,
            summary.passed,
            summary.performance.p95_us,
            summary.discontinuities.observed_max_gain_step,
            summary.discontinuities.observed_max_delay_step_samples
        );
    }
}

fn reproducible_command() -> String {
    std::env::args()
        .map(|argument| {
            if argument.chars().all(|character| {
                character.is_ascii_alphanumeric() || "-._/:=\\".contains(character)
            }) {
                argument
            } else {
                format!(
                    "\"{}\"",
                    argument.replace('\\', "\\\\").replace('"', "\\\"")
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
