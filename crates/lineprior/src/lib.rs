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
mod hash;
mod input;
mod model;
mod query;
mod report;
mod score;
mod tune;

pub use build::{BuildStats, build_prior_book};
pub use error::{Error, Result, Warning};
pub use eval::{
    CalibrationBin, EvalConfig, EvalOutput, EvalReport, ThresholdSweepEntry, TopKHitRate, evaluate,
};
pub use input::{BuildOutput, ParseOutcome, build_prior_book_from_reader, parse_jsonl};
pub use model::{
    BuildConfig, ConfidenceMode, DEFAULT_CONFIDENCE_K, DEFAULT_CONFIDENCE_Z, DEFAULT_DRAW_VALUE,
    DEFAULT_SOURCE_WEIGHT, MissingTimestampPolicy, Observation, Outcome, PriorAction, PriorBook,
    PriorEntry, SequencePriorScore, StepScore,
};
pub use query::{
    build_config_fingerprint, load_prior_book, load_prior_book_with_config, save_prior_book,
    save_prior_book_with_config,
};
pub use report::{StateEntropy, SummaryReport, state_entropy, summarize};
pub use tune::{
    ParetoEntry, TuneCandidateResult, TuneConstraints, TuneMetrics, TuneObjective, TuneOutput,
    TuneParam, build_candidate_result, covered_fraction, expand_grid, meets_constraints,
    objective_value, pareto_front, select_best,
};
