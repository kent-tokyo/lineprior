pub mod build;
pub mod eval;
pub mod query;
pub mod summary;
pub mod validate;

use clap::{Args, ValueEnum};
use lineprior::BuildConfig;
use std::process::ExitCode;

/// Maps a lineprior parse/build error to its documented exit code:
/// `NoObservations` is "no usable data" (2); a bare I/O fault reading an
/// already-opened stream is an internal error (4); everything else is bad
/// input (invalid JSON, missing/invalid fields) (3).
pub fn exit_code_for_lineprior_error(err: &lineprior::Error) -> ExitCode {
    match err {
        lineprior::Error::NoObservations => ExitCode::from(2),
        lineprior::Error::Io(_) => ExitCode::from(4),
        _ => ExitCode::from(3),
    }
}

/// CLI-facing mirror of [`lineprior::ConfidenceMode`] -- kept separate so the
/// core lib stays clap-free (same pattern as `eval.rs`'s `SplitBy`).
#[derive(Clone, Copy, ValueEnum)]
pub enum ConfidenceModeArg {
    Heuristic,
    WilsonLowerBound,
    Hybrid,
}

impl From<ConfidenceModeArg> for lineprior::ConfidenceMode {
    fn from(arg: ConfidenceModeArg) -> Self {
        match arg {
            ConfidenceModeArg::Heuristic => lineprior::ConfidenceMode::Heuristic,
            ConfidenceModeArg::WilsonLowerBound => lineprior::ConfidenceMode::WilsonLowerBound,
            ConfidenceModeArg::Hybrid => lineprior::ConfidenceMode::Hybrid,
        }
    }
}

/// CLI-facing mirror of [`lineprior::MissingTimestampPolicy`] -- same
/// clap-free-core rationale as `ConfidenceModeArg`.
#[derive(Clone, Copy, ValueEnum)]
pub enum MissingTimestampPolicyArg {
    KeepBaseWeight,
    Drop,
}

impl From<MissingTimestampPolicyArg> for lineprior::MissingTimestampPolicy {
    fn from(arg: MissingTimestampPolicyArg) -> Self {
        match arg {
            MissingTimestampPolicyArg::KeepBaseWeight => {
                lineprior::MissingTimestampPolicy::KeepBaseWeight
            }
            MissingTimestampPolicyArg::Drop => lineprior::MissingTimestampPolicy::Drop,
        }
    }
}

/// Parses one `name=weight` entry of `--source-weights`.
fn parse_source_weight_entry(s: &str) -> Result<(String, f64), String> {
    let (name, weight) = s
        .split_once('=')
        .ok_or_else(|| format!("invalid source weight {s:?}, expected `name=weight`"))?;
    let weight: f64 = weight
        .parse()
        .map_err(|_| format!("invalid weight in {s:?}: {weight:?} is not a number"))?;
    Ok((name.to_string(), weight))
}

/// The `BuildConfig`-affecting flags shared by `build` and `eval` (`eval`
/// needs to build its train-side prior under the same knobs a real `build`
/// run would use). Flattened into each command's own `Args` struct via
/// `#[command(flatten)]` rather than duplicated.
#[derive(Args)]
pub struct BuildConfigArgs {
    /// Minimum observation count for an action to appear in the output.
    #[arg(long, default_value_t = 1)]
    pub min_count: u64,

    /// Minimum weighted count for an action to appear in the output.
    #[arg(long, default_value_t = 0.0)]
    pub min_weighted_count: f64,

    /// Minimum confidence (see --confidence-k) for an action to appear in the output.
    #[arg(long, default_value_t = 0.0)]
    pub min_confidence: f64,

    /// Drop observations with `step` greater than this value.
    #[arg(long)]
    pub max_step: Option<u32>,

    /// Smoothing strength: higher values pull low-sample rates further
    /// toward the dataset-wide rate.
    #[arg(long, default_value_t = 5.0)]
    pub smoothing_alpha: f64,

    /// Keep only the top N ranked actions per state.
    #[arg(long)]
    pub max_actions_per_state: Option<usize>,

    /// Confidence half-life `k` in `confidence = weighted_count / (weighted_count + k)`.
    #[arg(long, default_value_t = lineprior::DEFAULT_CONFIDENCE_K)]
    pub confidence_k: f64,

    /// How `confidence` is computed: `heuristic` (sample-size only, default),
    /// `wilson-lower-bound` (statistical lower bound on success rate), or
    /// `hybrid` (heuristic * wilson-lower-bound).
    #[arg(long, value_enum, default_value_t = ConfidenceModeArg::Heuristic)]
    pub confidence_mode: ConfidenceModeArg,

    /// z-score for the Wilson lower bound used by `--confidence-mode
    /// wilson-lower-bound`/`hybrid`. Ignored under `heuristic`.
    #[arg(long, default_value_t = lineprior::DEFAULT_CONFIDENCE_Z)]
    pub confidence_z: f64,

    /// Keep only observations carrying at least one of these tags (comma-separated).
    #[arg(long, value_delimiter = ',')]
    pub tags: Vec<String>,

    /// Success credit for a Draw outcome (0.0 scores like a loss, 1.0 like a win).
    #[arg(long, default_value_t = lineprior::DEFAULT_DRAW_VALUE)]
    pub draw_value: f64,

    /// Half-life in days for exponential time decay of observation weight.
    /// Omit to disable time decay entirely (default). Requires
    /// --time-decay-reference-unix-seconds when set.
    #[arg(long)]
    pub time_decay_half_life_days: Option<f64>,

    /// "Now", as a Unix timestamp, for computing an observation's age in
    /// days. Required whenever --time-decay-half-life-days is set.
    #[arg(long)]
    pub time_decay_reference_unix_seconds: Option<i64>,

    /// What to do with an observation that has no observed_at_unix_seconds
    /// when time decay is enabled. Ignored when time decay is disabled.
    #[arg(long, value_enum, default_value_t = MissingTimestampPolicyArg::KeepBaseWeight)]
    pub missing_timestamp_policy: MissingTimestampPolicyArg,

    /// Per-source reliability multiplier, e.g.
    /// `engine_v012=1.0,engine_v010=0.6,human=0.8` (comma-separated).
    #[arg(long, value_delimiter = ',', value_parser = parse_source_weight_entry)]
    pub source_weights: Vec<(String, f64)>,

    /// Multiplier for an observation whose source is absent or not a key in
    /// --source-weights.
    #[arg(long, default_value_t = lineprior::DEFAULT_SOURCE_WEIGHT)]
    pub default_source_weight: f64,
}

impl BuildConfigArgs {
    pub fn into_build_config(self) -> BuildConfig {
        BuildConfig {
            min_count: self.min_count,
            min_weighted_count: self.min_weighted_count,
            min_confidence: self.min_confidence,
            max_step: self.max_step,
            smoothing_alpha: self.smoothing_alpha,
            max_actions_per_state: self.max_actions_per_state,
            confidence_k: self.confidence_k,
            confidence_mode: self.confidence_mode.into(),
            confidence_z: self.confidence_z,
            draw_value: self.draw_value,
            tag_filter: if self.tags.is_empty() {
                None
            } else {
                Some(self.tags)
            },
            time_decay_half_life_days: self.time_decay_half_life_days,
            time_decay_reference_unix_seconds: self.time_decay_reference_unix_seconds,
            missing_timestamp_policy: self.missing_timestamp_policy.into(),
            source_weights: self.source_weights.into_iter().collect(),
            default_source_weight: self.default_source_weight,
            ..BuildConfig::default()
        }
    }
}
