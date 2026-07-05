use anyhow::Result;
use clap::Args;
use lineprior::{load_prior_book, state_entropy, summarize};
use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct SummaryArgs {
    /// Prior book JSONL produced by `build`.
    input: PathBuf,
}

pub fn run(args: SummaryArgs) -> Result<ExitCode> {
    let file = match File::open(&args.input) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("error: opening {}: {err}", args.input.display());
            return Ok(ExitCode::from(3));
        }
    };

    let book = match load_prior_book(file) {
        Ok(book) => book,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(super::exit_code_for_lineprior_error(&err));
        }
    };

    if book.entries.is_empty() {
        eprintln!("error: no usable data");
        return Ok(ExitCode::from(2));
    }

    let summary = summarize(&book);
    println!("states:             {}", summary.num_states);
    println!("action entries:     {}", summary.num_action_entries);
    println!("avg confidence:     {:.3}", summary.avg_confidence);
    println!("avg entropy (bits): {:.3}", summary.avg_entropy_bits);
    println!();
    for entry in state_entropy(&book) {
        println!(
            "  {:<20} actions={:<4} entropy={:.3} bits",
            entry.state, entry.num_actions, entry.entropy_bits
        );
    }

    Ok(ExitCode::from(0))
}
