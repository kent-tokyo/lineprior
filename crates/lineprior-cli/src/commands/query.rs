use anyhow::Result;
use clap::Args;
use lineprior::load_prior_book;
use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Args)]
pub struct QueryArgs {
    /// Prior book JSONL produced by `build`.
    input: PathBuf,

    /// State key to look up.
    #[arg(long)]
    state: String,

    /// Return at most this many candidates.
    #[arg(long)]
    top_k: Option<usize>,

    /// A sequence's own recent-action window, oldest first (comma-separated).
    /// When set, queries via context-aware backoff instead of the plain
    /// order-0 lookup -- only useful against a book built with
    /// `--context-order` > 0; otherwise this always resolves to the same
    /// order-0 result plain `query` would give.
    #[arg(long, value_delimiter = ',')]
    recent_actions: Vec<String>,
}

pub fn run(args: QueryArgs) -> Result<ExitCode> {
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

    // An unseen state is not an error: lineprior never invents actions,
    // so an empty result here is normal, successful output.
    if args.recent_actions.is_empty() {
        for candidate in book.query(&args.state, args.top_k) {
            println!("{}", serde_json::to_string(&candidate)?);
        }
    } else {
        let result = book.query_with_context(&args.state, &args.recent_actions, args.top_k);
        println!("{}", serde_json::to_string(&result)?);
    }
    Ok(ExitCode::from(0))
}
