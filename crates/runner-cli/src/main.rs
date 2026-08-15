use clap::{Parser, Subcommand};

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
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::FormatVersion) => println!("{}", runner_core::plan_format_version()),
        None => println!("Use --help to list available commands."),
    }
}
