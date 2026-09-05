use std::path::PathBuf;

use anyhow::{bail, Result};
use aurora_cli::vbap3d_evaluation::{evaluate_vbap3d, Vbap3dEvaluationOptions};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "aurora-evaluate-vbap3d")]
#[command(about = "Generate deterministic offline 3D VBAP evidence for the canonical 5.1.2 scene")]
struct Cli {
    #[arg(long, default_value = "fixtures/scenes/5_1_2_upfiring.json")]
    scene: PathBuf,
    #[arg(long, default_value = "output/evaluation/vbap3d-5.1.2")]
    output_dir: PathBuf,
    #[arg(long, default_value_t = 2.0)]
    duration_seconds: f64,
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,
    #[arg(long, default_value_t = 256)]
    block_size: usize,
    #[arg(long, default_value_t = 0.15)]
    max_gain_step: f32,
    #[arg(long, default_value_t = 0.02)]
    normalization_tolerance: f32,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let options = Vbap3dEvaluationOptions {
        scene_path: cli.scene,
        output_dir: cli.output_dir,
        duration_seconds: cli.duration_seconds,
        sample_rate: cli.sample_rate,
        block_size: cli.block_size,
        max_gain_step: cli.max_gain_step,
        normalization_tolerance: cli.normalization_tolerance,
    };
    let summary = evaluate_vbap3d(&options, &reproducible_command())?;
    println!(
        "renderer=aurora-vbap3d passed={} triplets={} closed_hull={} max_gain_step={:.6} gain_step_violations={} normalization_failures={} non_finite_failures={}",
        summary.passed,
        summary.validated_triplets,
        summary.listener_inside_hull,
        summary.observed_max_gain_step,
        summary.gain_step_violations,
        summary.normalization_failures,
        summary.non_finite_failures,
    );
    if !summary.passed {
        bail!("3D VBAP evaluation failed one or more software evidence gates");
    }
    Ok(())
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
