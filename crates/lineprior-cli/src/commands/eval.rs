use super::{BuildConfigArgs, SplitBy};
use anyhow::{Context, Result};
use clap::Args;
use lineprior::{EvalConfig, evaluate};
use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct EvalArgs {
    /// Input JSONL observation log.
    input: PathBuf,

    /// How to assign observations to train/test. Only `sequence` is
    /// supported: every observation in the same `sequence_id` lands on the
    /// same side, so sequence-level information can't leak across the split.
    #[arg(long, value_enum, default_value_t = SplitBy::Sequence)]
    split_by: SplitBy,

    /// Fraction of sequences assigned to the train split.
    #[arg(long, default_value_t = 0.8)]
    train_ratio: f64,

    /// Which top-k hit rates to report (comma-separated).
    #[arg(long, value_delimiter = ',', default_value = "1,3,5")]
    top_k: Vec<usize>,

    /// Number of equal-width confidence bins in [0,1] for `confidence_calibration`.
    /// Omit to skip calibration reporting.
    #[arg(long)]
    calibration_bins: Option<usize>,

    /// Confidence thresholds to sweep for `threshold_sweep` (comma-separated).
    /// Omit to skip.
    #[arg(long, value_delimiter = ',')]
    thresholds: Vec<f64>,

    /// Write the JSON report here instead of stdout.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Load BuildConfig from this JSON file (e.g. from `tune
    /// --save-best-config`) instead of the individual flags below. Errors
    /// if combined with any of them.
    #[arg(long = "config")]
    config_file: Option<PathBuf>,

    #[command(flatten)]
    config: BuildConfigArgs,

    /// Fail on the first invalid record instead of skipping it with a warning.
    #[arg(long)]
    strict: bool,
}

pub fn run(args: EvalArgs) -> Result<ExitCode> {
    let SplitBy::Sequence = args.split_by;

    let train_file = match File::open(&args.input) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };
    // Opened independently from train_file: evaluate() reads each source
    // once, so a second pass over the same path is how the CLI gets two
    // streaming reads without holding the file in memory between them.
    let test_file = match File::open(&args.input) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };

    let build_config = super::resolve_build_config(args.config_file.as_deref(), args.config)?;
    let eval_config = EvalConfig {
        train_ratio: args.train_ratio,
        top_k: args.top_k,
        calibration_bins: args.calibration_bins,
        thresholds: args.thresholds,
    };

    let output = match evaluate(
        train_file,
        test_file,
        args.strict,
        &build_config,
        &eval_config,
    ) {
        Ok(output) => output,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(super::exit_code_for_lineprior_error(&err));
        }
    };
    for warning in &output.warnings {
        eprintln!("warning: {warning}");
    }

    if output.report.num_train_observations == 0 || output.report.num_test_observations == 0 {
        eprintln!("error: no usable data for evaluation");
        return Ok(ExitCode::from(2));
    }

    let json = serde_json::to_string_pretty(&output.report)?;
    match args.out {
        Some(path) => {
            std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
        }
        None => println!("{json}"),
    }

    if !output.warnings.is_empty() {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::from(0))
}
