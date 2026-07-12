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
