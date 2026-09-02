//! `lineprior`: domain-agnostic action priors built from historical action
//! sequences. Given a state, it answers "what actions have historically
//! worked well from here?" -- as a prior for another system to weigh, not
//! as an oracle. See `AGENTS.md` for the full design rationale.
//!
//! This library never writes to stdout/stderr and never panics on
//! malformed user input; all failure paths return [`Error`].
#![forbid(unsafe_code)]

mod binary;
mod build;
mod error;
mod eval;
mod gate;
mod hash;
mod input;
mod macro_action;
mod merge;
mod model;
mod offpolicy;
mod query;
mod report;
mod score;
mod similarity;
mod trie;
mod tune;

pub use binary::{load_prior_book_binary, save_prior_book_binary};
pub use build::{BuildStats, build_prior_book};
pub use error::{Error, Result, Warning};
pub use eval::{
    CalibrationBin, EvalConfig, EvalOutput, EvalReport, ThresholdSweepEntry, TopKHitRate, evaluate,
};
pub use gate::{
    GateAcquisition, GateAcquisitionConfig, GateAcquisitionQuery, GateCalibrationBin,
    GateFitOutput, GateFitReport, GateModel, GateModelConfig, GateObservation, GateOofPrediction,
    GatePrediction, GateQuery, GateStatus, GateValidationOutput, GateVerdict, GateVerdictConfig,
    GateVerdictPrediction, MonotonicDirection, PredictionStatus, default_gate_lambda_grid,
};
pub use input::{BuildOutput, ParseOutcome, build_prior_book_from_reader, parse_jsonl};
pub use macro_action::{
    MacroAction, MacroActionConfig, build_macro_actions, macro_action_candidates,
};
pub use merge::{PriorBookSource, merge_prior_books};
pub use model::{
    BuildConfig, ConfidenceMode, ContextQueryResult, DEFAULT_CONFIDENCE_K, DEFAULT_CONFIDENCE_Z,
    DEFAULT_DRAW_VALUE, DEFAULT_SOURCE_WEIGHT, MissingTimestampPolicy, Observation, Outcome,
    PriorAction, PriorBook, PriorEntry, ScoringStrategy, SequencePriorScore, StepScore,
};
pub use offpolicy::{
    BootstrapConfig, BootstrapInterval, DoublyRobustReport, OffPolicyBootstrapReport,
    OffPolicyConfig, OffPolicyError, OffPolicyObservation, OffPolicyReport,
    bootstrap_self_normalized_ips, evaluate_doubly_robust, evaluate_self_normalized_ips,
};
pub use query::{
    build_config_fingerprint, load_prior_book, load_prior_book_with_config, save_prior_book,
    save_prior_book_with_config,
};
pub use report::{StateEntropy, SummaryReport, state_entropy, summarize};
pub use similarity::{
    SimilarState, SimilarityCandidate, SimilarityConfig, SimilarityEvidence, SimilarityQueryResult,
};
pub use trie::PriorTrie;
pub use tune::{
    ParetoEntry, TuneCandidateResult, TuneConstraints, TuneMetrics, TuneObjective, TuneOutput,
    TuneParam, build_candidate_result, covered_fraction, expand_grid, meets_constraints,
    objective_value, pareto_front, select_best,
};
