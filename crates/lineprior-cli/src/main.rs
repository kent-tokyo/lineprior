#![forbid(unsafe_code)]

mod commands;

use clap::{Parser, Subcommand};
use commands::{
    batch_query, binary, build, eval, gate, offpolicy, query, summary, tune, validate,
    verify_artifact,
};
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
    /// Query a frozen book over versioned JSONL rows and write telemetry.
    BatchQuery(batch_query::BatchQueryArgs),
    /// Build a prior book from a JSONL observation log.
    Build(build::BuildArgs),
    /// Evaluate prior quality on held-out data.
    Eval(eval::EvalArgs),
    /// Fit the experimental GateModel and optionally predict a verdict/acquisition score.
    Gate(gate::GateArgs),
    /// Evaluate a logged policy with IPS, DR, and optional bootstrap intervals.
    Offpolicy(offpolicy::OffPolicyArgs),
    /// Pack a JSONL prior book into compact LPB binary.
    Pack(binary::PackArgs),
    /// Unpack an LPB binary prior book into JSONL.
    Unpack(binary::UnpackArgs),
    /// Query a prior book for candidate actions from a state.
    Query(query::QueryArgs),
    /// Summarize a prior book's coverage and confidence.
    Summary(summary::SummaryArgs),
    /// Grid-search BuildConfig candidates and pick the best by held-out eval.
    Tune(tune::TuneArgs),
    /// Validate a JSONL observation log without building a prior book.
    Validate(validate::ValidateArgs),
    /// Verify a prior-artifact manifest against exact input and book bytes.
    VerifyArtifact(verify_artifact::VerifyArtifactArgs),
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::BatchQuery(args) => batch_query::run(args),
        Commands::Build(args) => build::run(args),
        Commands::Eval(args) => eval::run(args),
        Commands::Gate(args) => gate::run(args),
        Commands::Offpolicy(args) => offpolicy::run(args),
        Commands::Pack(args) => binary::pack(args),
        Commands::Unpack(args) => binary::unpack(args),
        Commands::Query(args) => query::run(args),
        Commands::Summary(args) => summary::run(args),
        Commands::Tune(args) => tune::run(args),
        Commands::Validate(args) => validate::run(args),
        Commands::VerifyArtifact(args) => verify_artifact::run(args),
    };

    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(4)
        }
    }
}
