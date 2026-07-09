use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

/// One recorded step: from `state`, `action` was taken, with `outcome`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub sequence_id: String,
    pub step: u32,
    pub state: String,
    pub action: String,
    pub outcome: Outcome,
    pub score: Option<f64>,
    pub weight: f64,
    pub tags: Vec<String>,
    /// Wall-clock time the observation was recorded, for
    /// `BuildConfig::time_decay_half_life_days`. `None` is handled per
    /// `BuildConfig::missing_timestamp_policy` when decay is enabled, and
    /// ignored entirely otherwise.
    pub observed_at_unix_seconds: Option<i64>,
    /// Which data source produced this observation, for
    /// `BuildConfig::source_weights`. `None` (or an unrecognized value)
    /// falls back to `BuildConfig::default_source_weight`.
    pub source: Option<String>,
}

/// Result of taking `action` from `state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Success,
    Failure,
    Draw,
    #[default]
    Unknown,
}

impl Outcome {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "success" => Some(Outcome::Success),
            "failure" => Some(Outcome::Failure),
            "draw" => Some(Outcome::Draw),
            "unknown" => Some(Outcome::Unknown),
            _ => None,
        }
    }
}

/// We use k=20 so confidence grows slowly for low-sample actions.
/// This prevents one-off successes from dominating the prior.
pub const DEFAULT_CONFIDENCE_K: f64 = 20.0;

/// A draw is a genuine partial outcome in adversarial games (chess, shogi),
/// not a loss -- we default to crediting it as half a win rather than
/// scoring it identically to a failure.
pub const DEFAULT_DRAW_VALUE: f64 = 0.5;

/// z=1.96 is the two-sided-95% value conventionally used (Evan Miller /
/// Reddit-ranking style) as a *one-sided* Wilson lower bound -- slightly
/// more conservative than a strict one-sided 95% bound (which would use
/// ~1.64), which is the right direction for a "how much should I trust
/// this" score.
pub const DEFAULT_CONFIDENCE_Z: f64 = 1.96;

/// Multiplier applied to an observation whose `source` is `None` or not a
/// key in `BuildConfig::source_weights`. `1.0` means "trust unlabeled/unknown
/// sources same as any other" -- the backward-compatible default.
pub const DEFAULT_SOURCE_WEIGHT: f64 = 1.0;

/// What to do with an observation that has no `observed_at_unix_seconds`
/// when `BuildConfig::time_decay_half_life_days` is set. Inert when time
/// decay is disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingTimestampPolicy {
    /// Score it at its un-decayed `weight`, as if it were current.
    #[default]
    KeepBaseWeight,
    /// Exclude it entirely -- treat "unknown age" as "untrustworthy" rather
    /// than "trustworthy".
    Drop,
}

/// How [`PriorAction::confidence`] is computed. See [`crate::score::confidence`]
/// and [`crate::score::wilson_lower_bound`] for the underlying formulas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceMode {
    /// `weighted_count / (weighted_count + confidence_k)` -- a sample-size
    /// heuristic, blind to outcome. Not a statistical guarantee, but works
    /// even for datasets with no outcome/score data at all.
    #[default]
    Heuristic,
    /// Wilson score interval lower bound on the action's success rate. A
    /// real statistical lower bound, but requires outcome data -- falls
    /// back to [`ConfidenceMode::Heuristic`] for an action with none.
    WilsonLowerBound,
    /// `Heuristic * WilsonLowerBound`, so both low sample size *and* a
    /// weak success rate pull confidence down. Same fallback as
    /// `WilsonLowerBound` when there's no outcome data.
    Hybrid,
}

/// Tuning knobs for [`crate::build::build_prior_book`].
#[derive(Debug, Clone, Serialize)]
pub struct BuildConfig {
    pub min_count: u64,
    /// Minimum weighted count for an action to appear in the output.
    /// `0.0` means no filtering beyond `min_count`.
    pub min_weighted_count: f64,
    /// Minimum confidence (see [`DEFAULT_CONFIDENCE_K`]) for an action to
    /// appear in the output. `0.0` means no filtering.
    ///
    /// Which confidence this filters on depends on `confidence_mode`: under
    /// `Heuristic` it's a pure sample-size floor, blind to outcome. Under
    /// `WilsonLowerBound`/`Hybrid` it's success-rate-aware, so a high-count
    /// but mostly-failing action that used to pass this filter can now be
    /// dropped by it -- switching `confidence_mode` on an existing
    /// `min_confidence` threshold is a real behavior change, not just an
    /// additive one.
    pub min_confidence: f64,
    pub max_step: Option<u32>,
    pub smoothing_alpha: f64,
    pub score_weight: f64,
    pub success_weight: f64,
    pub count_weight: f64,
    pub max_actions_per_state: Option<usize>,
    pub confidence_k: f64,
    /// How `confidence` is computed. Defaults to [`ConfidenceMode::Heuristic`]
    /// for backward compatibility and for score-only datasets with no
    /// outcome labels.
    pub confidence_mode: ConfidenceMode,
    /// z-score for `ConfidenceMode::WilsonLowerBound`/`Hybrid`'s Wilson lower
    /// bound. Inert under `Heuristic`, but still folded into
    /// [`crate::build_config_fingerprint`] -- changing it (or upgrading to a
    /// lineprior version that adds it) changes the fingerprint even when the
    /// resulting `confidence` values don't, which is expected.
    pub confidence_z: f64,
    /// Success credit given to a `Draw` outcome, between 0.0 (scores like
    /// a failure) and 1.0 (scores like a win).
    pub draw_value: f64,
    /// Keep only observations carrying at least one of these tags.
    /// `None` means no tag filtering.
    pub tag_filter: Option<Vec<String>>,
    /// Half-life, in days, for exponential time decay of `weight`. `None`
    /// (the default) disables time decay entirely -- every observation
    /// counts at its full `weight`, regardless of `observed_at_unix_seconds`.
    pub time_decay_half_life_days: Option<f64>,
    /// "Now", for computing an observation's age in days. Required (and
    /// validated) whenever `time_decay_half_life_days` is `Some` -- there is
    /// no implicit wall-clock fallback, so that a given `BuildConfig`
    /// (and the fingerprint/output it produces) stays reproducible across
    /// repeated runs rather than drifting with real time.
    pub time_decay_reference_unix_seconds: Option<i64>,
    /// What to do with an observation that has no `observed_at_unix_seconds`
    /// when time decay is enabled. Inert when `time_decay_half_life_days`
    /// is `None`.
    pub missing_timestamp_policy: MissingTimestampPolicy,
    /// Per-source reliability multiplier, e.g. `{"engine_v012": 1.0, "human":
    /// 0.8}`. A `BTreeMap`, not a `HashMap`: `BuildConfig`'s fingerprint
    /// hashes its serde_json encoding, and `HashMap`'s randomized iteration
    /// order would make the same logical config fingerprint differently
    /// across runs.
    pub source_weights: std::collections::BTreeMap<String, f64>,
    /// Multiplier used for an observation whose `source` is `None` or not a
    /// key in `source_weights`.
    pub default_source_weight: f64,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            min_count: 1,
            min_weighted_count: 0.0,
            min_confidence: 0.0,
            max_step: None,
            smoothing_alpha: 5.0,
            score_weight: 1.0,
            success_weight: 1.0,
            count_weight: 1.0,
            max_actions_per_state: None,
            confidence_k: DEFAULT_CONFIDENCE_K,
            confidence_mode: ConfidenceMode::Heuristic,
            confidence_z: DEFAULT_CONFIDENCE_Z,
            draw_value: DEFAULT_DRAW_VALUE,
            tag_filter: None,
            time_decay_half_life_days: None,
            time_decay_reference_unix_seconds: None,
            missing_timestamp_policy: MissingTimestampPolicy::KeepBaseWeight,
            source_weights: std::collections::BTreeMap::new(),
            default_source_weight: DEFAULT_SOURCE_WEIGHT,
        }
    }
}

/// One candidate action ranked for a given state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriorAction {
    pub action: String,
    pub count: u64,
    pub weighted_count: f64,
    /// Raw (unsmoothed) rate: successes count as 1.0, draws count as
    /// `BuildConfig::draw_value` (default 0.5), failures as 0.0.
    pub success_rate: Option<f64>,
    pub mean_score: Option<f64>,
    pub prior: f64,
    pub confidence: f64,
}

/// One line of prior-book output: a state and its ranked actions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriorEntry {
    pub state: String,
    pub actions: Vec<PriorAction>,
}

/// In-memory prior book: state -> ranked candidate actions.
#[derive(Debug, Clone, Default)]
pub struct PriorBook {
    pub entries: HashMap<String, Vec<PriorAction>>,
}

/// Deterministic action ordering: descending prior, tie-broken by action
/// string. Shared by build (emit) and query so both agree on ranking.
fn sort_actions(actions: &mut [PriorAction]) {
    actions.sort_by(|a, b| {
        b.prior
            .partial_cmp(&a.prior)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.action.cmp(&b.action))
    });
}

impl PriorBook {
    /// All entries, states sorted lexicographically and each state's
    /// actions sorted by descending prior. This is the canonical order
    /// used for JSONL output and for query results.
    pub fn entries_sorted(&self) -> Vec<PriorEntry> {
        let mut states: Vec<&String> = self.entries.keys().collect();
        states.sort();
        states
            .into_iter()
            .map(|state| {
                let mut actions = self.entries[state].clone();
                sort_actions(&mut actions);
                PriorEntry {
                    state: state.clone(),
                    actions,
                }
            })
            .collect()
    }

    /// Ranked candidates for `state`. An unseen state yields an empty
    /// vec, never an error and never an invented action.
    pub fn query(&self, state: &str, top_k: Option<usize>) -> Vec<PriorAction> {
        let Some(actions) = self.entries.get(state) else {
            return Vec::new();
        };
        let mut actions = actions.clone();
        sort_actions(&mut actions);
        if let Some(k) = top_k {
            actions.truncate(k);
        }
        actions
    }

    /// Flat, deterministically-ordered `(state, action)` candidates across
    /// the whole book -- for callers that want to filter or sample raw
    /// candidates directly (e.g. building a domain-specific "opening
    /// suite") instead of working through the nested per-state structure
    /// `entries_sorted` returns. Same ordering guarantee as `entries_sorted`.
    pub fn candidates(&self) -> Vec<(String, PriorAction)> {
        self.entries_sorted()
            .into_iter()
            .flat_map(|entry| {
                let state = entry.state;
                entry
                    .actions
                    .into_iter()
                    .map(move |action| (state.clone(), action))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(name: &str, count: u64, prior: f64, confidence: f64) -> PriorAction {
        PriorAction {
            action: name.to_string(),
            count,
            weighted_count: count as f64,
            success_rate: None,
            mean_score: None,
            prior,
            confidence,
        }
    }

    #[test]
    fn candidates_flattens_every_state_action_pair_in_entries_sorted_order() {
        let mut entries = HashMap::new();
        entries.insert(
            "s2".to_string(),
            vec![action("y", 3, 0.6, 0.3), action("z", 1, 0.4, 0.1)],
        );
        entries.insert("s1".to_string(), vec![action("x", 5, 1.0, 0.5)]);
        let book = PriorBook { entries };

        let expected: Vec<(String, PriorAction)> = book
            .entries_sorted()
            .into_iter()
            .flat_map(|entry| {
                entry
                    .actions
                    .into_iter()
                    .map(move |action| (entry.state.clone(), action))
            })
            .collect();

        assert_eq!(book.candidates(), expected);
        assert_eq!(book.candidates().len(), 3);
        assert_eq!(
            book.candidates()[0],
            ("s1".to_string(), action("x", 5, 1.0, 0.5))
        );
    }
}
