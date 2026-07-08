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
            ..BuildConfig::default()
        }
    }
}
