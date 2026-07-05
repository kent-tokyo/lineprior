#![forbid(unsafe_code)]

mod commands;

use clap::{Parser, Subcommand};
use commands::{build, query, summary, validate};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "lineprior",
    version,
    about = "Build and query domain-agnostic action priors from historical action sequences."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a prior book from a JSONL observation log.
    Build(build::BuildArgs),
    /// Query a prior book for candidate actions from a state.
    Query(query::QueryArgs),
    /// Summarize a prior book's coverage and confidence.
    Summary(summary::SummaryArgs),
    /// Validate a JSONL observation log without building a prior book.
    Validate(validate::ValidateArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Build(args) => build::run(args),
        Commands::Query(args) => query::run(args),
        Commands::Summary(args) => summary::run(args),
        Commands::Validate(args) => validate::run(args),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(4)
        }
    }
}
