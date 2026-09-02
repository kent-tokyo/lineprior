use anyhow::{Context, Result};
use clap::Args;
use lineprior::{
    BootstrapConfig, DoublyRobustReport, OffPolicyBootstrapReport, OffPolicyConfig,
    OffPolicyObservation, OffPolicyReport, bootstrap_self_normalized_ips, evaluate_doubly_robust,
    evaluate_self_normalized_ips,
};
use serde::Serialize;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct OffPolicyArgs {
    /// JSONL log containing one OffPolicyObservation per line.
    input: PathBuf,

    /// JSON output path for the estimates and diagnostics.
    #[arg(long)]
    out: PathBuf,

    /// Name recorded for the evaluation policy.
    #[arg(long, default_value = "unspecified")]
    policy_name: String,

    /// Optional version recorded for the evaluation policy.
    #[arg(long)]
    policy_version: Option<String>,

    /// Exclude rows whose importance weight exceeds this value.
    #[arg(long)]
    max_importance_weight: Option<f64>,

    /// Also compute doubly robust estimation from the model fields in every row.
    #[arg(long)]
    doubly_robust: bool,

    /// Number of deterministic bootstrap resamples. Omit to skip intervals.
    #[arg(long)]
    bootstrap_resamples: Option<usize>,

    /// Seed for deterministic bootstrap resampling.
    #[arg(long, default_value_t = 0)]
    bootstrap_seed: u64,

    /// Percentile-bootstrap confidence level.
    #[arg(long, default_value_t = 0.95)]
    confidence_level: f64,
}

#[derive(Serialize)]
struct OffPolicyOutput {
    ips: OffPolicyReport,
    doubly_robust: Option<DoublyRobustReport>,
    bootstrap: Option<OffPolicyBootstrapReport>,
}

pub fn run(args: OffPolicyArgs) -> Result<ExitCode> {
    let file = match File::open(&args.input) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };
    let mut observations = Vec::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.with_context(|| format!("reading line {line_number}"))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<OffPolicyObservation>(&line) {
            Ok(observation) => observations.push(observation),
            Err(err) => {
                eprintln!("error: line {line_number}: invalid off-policy JSON: {err}");
                return Ok(ExitCode::from(3));
            }
        }
    }

    let config = OffPolicyConfig {
        max_importance_weight: args.max_importance_weight,
        policy_name: args.policy_name,
        policy_version: args.policy_version,
    };
    let ips = match evaluate_self_normalized_ips(&observations, &config) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(ExitCode::from(3));
        }
    };
    let doubly_robust = if args.doubly_robust {
        match evaluate_doubly_robust(&observations, &config) {
            Ok(report) => Some(report),
            Err(err) => {
                eprintln!("error: {err}");
                return Ok(ExitCode::from(3));
            }
        }
    } else {
        None
    };
    let bootstrap = match args.bootstrap_resamples {
        Some(resamples) => match bootstrap_self_normalized_ips(
            &observations,
            &config,
            BootstrapConfig {
                resamples,
                seed: args.bootstrap_seed,
                confidence_level: args.confidence_level,
            },
        ) {
            Ok(report) => Some(report),
            Err(err) => {
                eprintln!("error: {err}");
                return Ok(ExitCode::from(3));
            }
        },
        None => None,
    };

    let output = OffPolicyOutput {
        ips,
        doubly_robust,
        bootstrap,
    };
    let out =
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    serde_json::to_writer_pretty(BufWriter::new(out), &output)
        .context("writing off-policy report")?;
    Ok(ExitCode::from(0))
}
