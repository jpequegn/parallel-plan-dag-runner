use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use runner_core::{
    ExecutionMode, Executor, Ledger, Plan, ToolRegistry, ValidationError, parse_plan,
    plan_json_schema, validate_plan,
};
use tokio_util::sync::CancellationToken;

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
    /// Execute a plan with deterministic tools and persist its event stream.
    Run {
        plan: PathBuf,
        #[arg(long, default_value = "runs.db")]
        db: PathBuf,
        #[arg(long, value_enum, default_value_t = CliMode::Parallel)]
        mode: CliMode,
    },
    /// List persisted runs.
    Runs {
        #[arg(long, default_value = "runs.db")]
        db: PathBuf,
    },
    /// Inspect a verified event stream.
    Inspect {
        run_id: String,
        #[arg(long, default_value = "runs.db")]
        db: PathBuf,
    },
    /// Replay a stored run without invoking tools.
    Replay {
        run_id: String,
        #[arg(long, default_value = "runs.db")]
        db: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum CliMode {
    Sequential,
    #[default]
    Parallel,
}

impl From<CliMode> for ExecutionMode {
    fn from(value: CliMode) -> Self {
        match value {
            CliMode::Sequential => Self::Sequential,
            CliMode::Parallel => Self::Parallel,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::FormatVersion) => println!("{}", runner_core::plan_format_version()),
        Some(Command::Validate { plan }) => validate_file(&plan)?,
        Some(Command::Schema { output }) => write_schema(output.as_ref())?,
        Some(Command::Run { plan, db, mode }) => run_plan(&plan, &db, mode.into()).await?,
        Some(Command::Runs { db }) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&Ledger::open(db)?.list_runs()?)?
            );
        }
        Some(Command::Inspect { run_id, db }) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&Ledger::open(db)?.inspect(&run_id)?)?
            );
        }
        Some(Command::Replay { run_id, db }) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&Ledger::open(db)?.replay(&run_id)?)?
            );
        }
        None => println!("Use --help to list available commands."),
    }
    Ok(())
}

fn validate_file(path: &PathBuf) -> Result<()> {
    let plan = load_plan(path)?;
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

fn load_plan(path: &PathBuf) -> Result<Plan> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read plan {}", path.display()))?;
    let format = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    Ok(parse_plan(&source, format)?)
}

async fn run_plan(path: &PathBuf, db: &PathBuf, mode: ExecutionMode) -> Result<()> {
    let plan = load_plan(path)?;
    let registry = ToolRegistry::deterministic(BTreeMap::new(), BTreeMap::new());
    let result = Executor::new(&registry)
        .mode(mode)
        .execute(&plan, CancellationToken::new())
        .await?;
    let mut ledger = Ledger::open(db)?;
    let run_id = ledger.store_run(&plan, mode, &result)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "run_id": run_id,
            "result": result,
        }))?
    );
    Ok(())
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
