use anyhow::Result;
use clap::Args;
use lineprior::parse_jsonl;
use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct ValidateArgs {
    /// Input JSONL observation log to validate.
    input: PathBuf,

    /// Fail on the first invalid record instead of collecting warnings.
    #[arg(long)]
    strict: bool,
}

pub fn run(args: ValidateArgs) -> Result<ExitCode> {
    let file = match File::open(&args.input) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };

    let parsed = match parse_jsonl(file, args.strict) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(super::exit_code_for_lineprior_error(&err));
        }
    };

    for warning in &parsed.warnings {
        eprintln!("warning: {warning}");
    }
    println!(
        "{} valid observation(s), {} warning(s)",
        parsed.observations.len(),
        parsed.warnings.len()
    );

    if parsed.observations.is_empty() {
        eprintln!("error: no usable data");
        return Ok(ExitCode::from(2));
    }
    if !parsed.warnings.is_empty() {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::from(0))
}
