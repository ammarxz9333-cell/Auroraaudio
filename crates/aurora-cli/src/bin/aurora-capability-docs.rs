use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use aurora_cli::capabilities::{
    replace_readme_capability_section, verify_readme_capability_section,
};
use clap::{Parser, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "aurora-capability-docs")]
#[command(about = "Verify or regenerate README capability claims from the canonical registry")]
struct Cli {
    #[arg(long, value_enum, default_value_t = Mode::Check)]
    mode: Mode,
    #[arg(long, default_value = "README.md")]
    readme: PathBuf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Mode {
    Check,
    Write,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let source = fs::read_to_string(&cli.readme)
        .with_context(|| format!("read {}", cli.readme.display()))?;
    match cli.mode {
        Mode::Check => {
            verify_readme_capability_section(&source)?;
            println!("capability README section is synchronized with registry");
        }
        Mode::Write => {
            let generated = replace_readme_capability_section(&source)?;
            if generated == source {
                println!("capability README section already synchronized");
            } else {
                fs::write(&cli.readme, generated)
                    .with_context(|| format!("write {}", cli.readme.display()))?;
                println!("updated capability README section from registry");
            }
        }
    }
    if !cli.readme.is_file() {
        bail!("README path is not a file: {}", cli.readme.display());
    }
    Ok(())
}
