use super::BuildConfigArgs;
use anyhow::{Context, Result};
use clap::Args;
use lineprior::{build_prior_book_from_reader, save_prior_book_with_config};
use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct BuildArgs {
    /// Input JSONL observation log.
    input: PathBuf,

    /// Output path for the prior book JSONL.
    #[arg(long)]
    out: PathBuf,

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

pub fn run(args: BuildArgs) -> Result<ExitCode> {
    let file = match File::open(&args.input) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };

    let config = super::resolve_build_config(args.config_file.as_deref(), args.config)?;

    // Streams straight from the file into the prior book -- memory stays
    // bounded by unique (state, action) pairs, not the number of lines
    // read, instead of collecting every observation into a Vec first.
    let output = match build_prior_book_from_reader(file, args.strict, &config) {
        Ok(output) => output,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(super::exit_code_for_lineprior_error(&err));
        }
    };
    for warning in &output.warnings {
        eprintln!("warning: {warning}");
    }

    if output.book.entries.is_empty() {
        eprintln!("error: no usable data");
        return Ok(ExitCode::from(2));
    }

    let stats = &output.stats;
    eprintln!(
        "stats: {}/{} observations kept, {}/{} candidates kept \
         ({} by min_count, {} by min_weighted_count, {} by min_confidence, {} by max_actions_per_state)",
        stats.observations_kept,
        stats.observations_kept
            + stats.observations_dropped_by_step_or_tag_filter
            + stats.observations_dropped_by_missing_timestamp,
        stats.candidates_kept,
        stats.candidates_before_filtering,
        stats.candidates_dropped_by_min_count,
        stats.candidates_dropped_by_min_weighted_count,
        stats.candidates_dropped_by_min_confidence,
        stats.candidates_dropped_by_max_actions_per_state,
    );

    let out_file =
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    save_prior_book_with_config(&output.book, &config, BufWriter::new(out_file))
        .context("writing prior book")?;

    if !output.warnings.is_empty() {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::from(0))
}
