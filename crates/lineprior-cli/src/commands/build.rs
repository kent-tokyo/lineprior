use anyhow::{Context, Result};
use clap::Args;
use lineprior::{BuildConfig, build_prior_book_from_reader, save_prior_book};
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

    /// Minimum observation count for an action to appear in the output.
    #[arg(long, default_value_t = 1)]
    min_count: u64,

    /// Minimum weighted count for an action to appear in the output.
    #[arg(long, default_value_t = 0.0)]
    min_weighted_count: f64,

    /// Minimum confidence (see --confidence-k) for an action to appear in the output.
    #[arg(long, default_value_t = 0.0)]
    min_confidence: f64,

    /// Drop observations with `step` greater than this value.
    #[arg(long)]
    max_step: Option<u32>,

    /// Smoothing strength: higher values pull low-sample rates further
    /// toward the dataset-wide rate.
    #[arg(long, default_value_t = 5.0)]
    smoothing_alpha: f64,

    /// Keep only the top N ranked actions per state.
    #[arg(long)]
    max_actions_per_state: Option<usize>,

    /// Confidence half-life `k` in `confidence = weighted_count / (weighted_count + k)`.
    #[arg(long, default_value_t = lineprior::DEFAULT_CONFIDENCE_K)]
    confidence_k: f64,

    /// Keep only observations carrying at least one of these tags (comma-separated).
    #[arg(long, value_delimiter = ',')]
    tags: Vec<String>,

    /// Success credit for a Draw outcome (0.0 scores like a loss, 1.0 like a win).
    #[arg(long, default_value_t = lineprior::DEFAULT_DRAW_VALUE)]
    draw_value: f64,

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

    let config = BuildConfig {
        min_count: args.min_count,
        min_weighted_count: args.min_weighted_count,
        min_confidence: args.min_confidence,
        max_step: args.max_step,
        smoothing_alpha: args.smoothing_alpha,
        max_actions_per_state: args.max_actions_per_state,
        confidence_k: args.confidence_k,
        draw_value: args.draw_value,
        tag_filter: if args.tags.is_empty() {
            None
        } else {
            Some(args.tags)
        },
        ..BuildConfig::default()
    };

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

    let out_file =
        File::create(&args.out).with_context(|| format!("creating {}", args.out.display()))?;
    save_prior_book(&output.book, BufWriter::new(out_file)).context("writing prior book")?;

    if !output.warnings.is_empty() {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::from(0))
}
