use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use runner_core::{ValidationError, parse_plan, plan_json_schema, validate_plan};

#[derive(Debug, Parser)]
#[command(
    name = "plan-runner",
    version,
    about = "Validate, execute, and replay typed plan DAGs"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the supported plan format version.
    FormatVersion,
    /// Validate a JSON or YAML plan without executing tools.
    Validate { plan: PathBuf },
    /// Print the JSON Schema for the supported plan format.
    Schema {
        /// Write to a file instead of standard output.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::FormatVersion) => println!("{}", runner_core::plan_format_version()),
        Some(Command::Validate { plan }) => validate_file(&plan)?,
        Some(Command::Schema { output }) => write_schema(output.as_ref())?,
        None => println!("Use --help to list available commands."),
    }
    Ok(())
}

fn validate_file(path: &PathBuf) -> Result<()> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read plan {}", path.display()))?;
    let format = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let plan = parse_plan(&source, format)?;
    match validate_plan(&plan) {
        Ok(()) => {
            println!("valid plan: {} ({} nodes)", plan.id, plan.nodes.len());
            Ok(())
        }
        Err(ValidationError::Invalid { diagnostics }) => {
            eprintln!("{}", serde_json::to_string_pretty(&diagnostics)?);
            bail!("plan failed preflight validation")
        }
        Err(error) => Err(error.into()),
    }
}

fn write_schema(output: Option<&PathBuf>) -> Result<()> {
    let schema = plan_json_schema()?;
    if let Some(path) = output {
        fs::write(path, schema).with_context(|| format!("failed to write {}", path.display()))?;
    } else {
        println!("{schema}");
    }
    Ok(())
}
