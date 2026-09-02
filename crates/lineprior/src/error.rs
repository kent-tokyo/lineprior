use thiserror::Error;

/// Errors produced while parsing, validating, or building a prior book.
///
/// Every variant that originates from a JSONL file carries the 1-indexed
/// line number so callers can point users at the offending record.
#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("line {line}: invalid JSON: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },

    #[error("line {line}: missing required field `{field}`")]
    MissingField { line: usize, field: &'static str },

    #[error("line {line}: state must not be empty")]
    EmptyState { line: usize },

    #[error("line {line}: action must not be empty")]
    EmptyAction { line: usize },

    #[error("line {line}: weight must not be negative, got {value}")]
    NegativeWeight { line: usize, value: f64 },

    #[error("line {line}: score must not be NaN")]
    NanScore { line: usize },

    #[error("line {line}: unsupported outcome value `{value}`")]
    UnsupportedOutcome { line: usize, value: String },

    #[error("no observations remain after parsing and filtering")]
    NoObservations,

    #[error(
        "prior book was built with a different config than expected \
         (expected fingerprint {expected}, found {found})"
    )]
    BuildConfigMismatch { expected: u64, found: u64 },

    #[error("invalid build config: {message}")]
    InvalidConfig { message: String },

    #[error("invalid compact prior book: {message}")]
    InvalidBinary { message: String },

    /// Raised only when `BuildConfig::context_order > 0`: deriving a
    /// sequence's recent-action window while streaming requires that
    /// sequence's own rows be contiguous and in increasing `step` order.
    /// Identified by `sequence_id`/`step` rather than a line number --
    /// unlike the JSONL-parse errors above, this is checked after parsing,
    /// against the observation stream itself (shared by both the eager and
    /// streaming build paths, only one of which has line numbers at all).
    /// Unconditional -- not gated by `--strict`, since this is a stream-wide
    /// structural precondition, not a single bad record.
    #[error(
        "sequence `{sequence_id}`: step {step} does not follow step {last_step} \
         -- input must be sorted by (sequence_id, step) when context_order > 0"
    )]
    SequenceNotSorted {
        sequence_id: String,
        step: u32,
        last_step: u32,
    },

    /// Raised by [`crate::gate::GateModel::fit`]: fewer than 2 distinct
    /// `group_id`s means there is no way to hold out a group for
    /// cross-validation without training on 100% of the data.
    #[error("gate fit requires at least 2 distinct group_ids, found {num_groups}")]
    InsufficientGateGroups { num_groups: usize },

    /// Raised by [`crate::gate::GateModel::fit`]: too few observations to fit
    /// `num_features` coefficients without massively overfitting. `required`
    /// is `max(6, num_features + 2)`.
    #[error(
        "gate fit requires at least {required} observations for {num_features} features, found {num_observations}"
    )]
    InsufficientGateObservations {
        num_observations: usize,
        num_features: usize,
        required: usize,
    },

    /// Raised by [`crate::gate::GateModel::fit`]: every [`crate::gate::GateObservation`]
    /// must carry the same set of feature names -- a per-row-varying feature
    /// set would silently change what each coefficient means, unlike
    /// `predict`'s query-time missing/unknown-feature handling, which is
    /// explicit and reported rather than baked into training.
    #[error(
        "gate observation `{candidate_id}` has a different feature set than the first observation"
    )]
    InconsistentGateFeatures { candidate_id: String },

    /// Raised by [`crate::gate::GateModel::fit`]: a `gate_elo_delta`,
    /// `gate_games_played`, or feature value was NaN or infinite.
    #[error("gate observation `{candidate_id}` has a non-finite value in field `{field}`")]
    NonFiniteGateValue { candidate_id: String, field: String },

    /// Raised by [`crate::gate::GateModel::fit`]: `gate_games_played` must be
    /// a positive, finite reliability weight for the label.
    #[error("gate observation `{candidate_id}` has gate_games_played <= 0: {value}")]
    NonPositiveGateWeight { candidate_id: String, value: f64 },

    /// Raised by [`crate::gate::GateModel::fit`]: a provided
    /// `actual_elo_stddev` must be a positive, finite measurement-noise
    /// stddev (it becomes a divisor in the inverse-variance reliability
    /// weight).
    #[error("gate observation `{candidate_id}` has actual_elo_stddev <= 0: {value}")]
    NonPositiveGateStddev { candidate_id: String, value: f64 },

    /// Raised by [`crate::gate::GateModel::fit`]: an observation must not
    /// specify both `actual_elo_stddev` and a complete `elo_ci_low`/
    /// `elo_ci_high` pair -- exactly one uncertainty source per observation,
    /// never a silent priority between them.
    #[error(
        "gate observation `{candidate_id}` specifies both actual_elo_stddev and an elo confidence interval -- provide exactly one"
    )]
    ConflictingGateUncertaintySources { candidate_id: String },

    /// Raised by [`crate::gate::GateModel::fit`]: `elo_ci_low`/`elo_ci_high`
    /// must be provided together, or neither at all.
    #[error(
        "gate observation `{candidate_id}` specifies only one of elo_ci_low/elo_ci_high -- both are required together"
    )]
    IncompleteGateConfidenceInterval { candidate_id: String },

    /// Raised by [`crate::gate::GateModel::fit`]: `gate_elo_delta` must lie
    /// within its own stated `elo_ci_low`/`elo_ci_high` bounds.
    #[error(
        "gate observation `{candidate_id}` has gate_elo_delta {gate_elo_delta} outside its own [elo_ci_low, elo_ci_high] = [{elo_ci_low}, {elo_ci_high}]"
    )]
    GateEloOutsideConfidenceInterval {
        candidate_id: String,
        gate_elo_delta: f64,
        elo_ci_low: f64,
        elo_ci_high: f64,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// A non-fatal issue skipped in non-strict mode. Carries enough detail to
/// report to the user without aborting the whole run.
#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `message` is the source error's own Display, which already
        // includes "line N: ..." -- don't prepend it again here.
        write!(f, "{}", self.message)
    }
}
