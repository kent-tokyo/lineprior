//! `lineprior`: domain-agnostic action priors built from historical action
//! sequences. Given a state, it answers "what actions have historically
//! worked well from here?" -- as a prior for another system to weigh, not
//! as an oracle. See `AGENTS.md` for the full design rationale.
//!
//! This library never writes to stdout/stderr and never panics on
//! malformed user input; all failure paths return [`Error`].
#![forbid(unsafe_code)]

mod build;
mod error;
mod eval;
mod input;
mod model;
mod query;
mod report;
mod score;

pub use build::build_prior_book;
pub use error::{Error, Result, Warning};
pub use eval::{EvalConfig, EvalOutput, EvalReport, TopKHitRate, evaluate};
pub use input::{BuildOutput, ParseOutcome, build_prior_book_from_reader, parse_jsonl};
pub use model::{
    BuildConfig, DEFAULT_CONFIDENCE_K, DEFAULT_DRAW_VALUE, Observation, Outcome, PriorAction,
    PriorBook, PriorEntry,
};
pub use query::{load_prior_book, save_prior_book};
pub use report::{StateEntropy, SummaryReport, state_entropy, summarize};
