use crate::error::{Error, Result};
use crate::model::{
    BuildConfig, ConfidenceMode, MissingTimestampPolicy, Observation, Outcome, PriorAction,
    PriorBook, outcome_credit,
};
use crate::score::{
    confidence, effective_sample_size, normalize, ratio, raw_score, shrink_toward,
    time_decay_multiplier, wilson_lower_bound,
};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};

/// Running totals for one `(state, action)` pair.
#[derive(Debug, Default, Clone)]
struct ActionStats {
    count: u64,
    weighted_count: f64,
    /// Weighted successes / weighted trials, where a "trial" is any
    /// observation with a decisive (non-unknown) outcome.
    weighted_successes: f64,
    weighted_trials: f64,
    /// Sum of squared observation weights over decisive-outcome trials only
    /// -- feeds `effective_sample_size` for `ConfidenceMode::WilsonLowerBound`/`Hybrid`.
    weighted_trial_weight_sq_sum: f64,
    weighted_score_sum: f64,
    weighted_score_count: f64,
}

/// Tracks a sequence's own recent-action history while observations stream
/// in one at a time, for `BuildConfig::context_order > 0`. Requires input
/// grouped by `sequence_id` with strictly increasing `step` within each
/// group -- see `Error::SequenceNotSorted`.
///
/// A `sequence_id` change is always treated as a fresh sequence starting
/// (unconditional reset, no partial reuse of the previous window). This is
/// what makes an "A, B, A-resumed" interleave degrade safely to a short/
/// reset window rather than silently splicing two sequences' actions
/// together -- without needing to remember every `sequence_id` ever seen,
/// which would cost memory proportional to unique sequence count and
/// undermine the streaming-memory guarantee this whole feature must not
/// break.
pub(crate) struct SequenceContextTracker {
    order: usize,
    current_sequence_id: Option<String>,
    last_step: Option<u32>,
    window: VecDeque<String>,
}

impl SequenceContextTracker {
    pub(crate) fn new(order: usize) -> Self {
        Self {
            order,
            current_sequence_id: None,
            last_step: None,
            window: VecDeque::new(),
        }
    }

    /// Returns the window's contents *before* `obs` (oldest first -- what
    /// happened earlier in this sequence, never including `obs.action`
    /// itself), then folds `obs.action` into the window for the next call.
    /// A no-op when `order == 0`: always returns an empty window and never
    /// errors, so every existing caller is unaffected.
    pub(crate) fn advance(&mut self, obs: &Observation) -> Result<Vec<String>> {
        if self.order == 0 {
            return Ok(Vec::new());
        }

        match &self.current_sequence_id {
            Some(current) if current == &obs.sequence_id => {
                let last_step = self.last_step.expect("set alongside current_sequence_id");
                if obs.step <= last_step {
                    return Err(Error::SequenceNotSorted {
                        sequence_id: obs.sequence_id.clone(),
                        step: obs.step,
                        last_step,
                    });
                }
            }
            _ => {
                // New sequence (or the very first observation ever):
                // unconditional reset, no partial reuse of the old window.
                self.current_sequence_id = Some(obs.sequence_id.clone());
                self.window.clear();
            }
        }
        self.last_step = Some(obs.step);

        let window: Vec<String> = self.window.iter().cloned().collect();

        self.window.push_back(obs.action.clone());
        if self.window.len() > self.order {
            self.window.pop_front();
        }

        Ok(window)
    }
}

/// Wilson lower bound of `stat`'s success rate, or `None` if it has no
/// decisive-outcome observations at all (nothing to bound).
///
/// Note: Kish's effective sample size (`n_eff`, see [`effective_sample_size`])
/// is invariant to uniformly scaling every weight by the same factor -- so
/// when time decay (or a source-reliability multiplier) applies equally to
/// every one of an action's own observations, this bound doesn't move even
/// though `weighted_count` (and therefore `prior`) does. Decay always shrinks
/// the ranking score; it only shrinks *this* confidence number under
/// `ConfidenceMode::Heuristic`/`Hybrid`, where the heuristic factor carries
/// `weighted_count` directly.
fn wilson_confidence(stat: &ActionStats, z: f64) -> Option<f64> {
    let n_eff = effective_sample_size(stat.weighted_trials, stat.weighted_trial_weight_sq_sum);
    let p_hat = ratio(stat.weighted_successes, stat.weighted_trials)?;
    wilson_lower_bound(p_hat * n_eff, n_eff, z)
}

/// The `confidence` reported for one action, per `config.confidence_mode`.
/// Single source of truth for both the `min_confidence` filter and the
/// `PriorAction.confidence` field, so they can never drift apart.
fn action_confidence(stat: &ActionStats, config: &BuildConfig) -> f64 {
    let heuristic_val = confidence(stat.weighted_count, config.confidence_k);
    match config.confidence_mode {
        ConfidenceMode::Heuristic => heuristic_val,
        ConfidenceMode::WilsonLowerBound => {
            wilson_confidence(stat, config.confidence_z).unwrap_or(heuristic_val)
        }
        ConfidenceMode::Hybrid => wilson_confidence(stat, config.confidence_z)
            .map(|w| heuristic_val * w)
            .unwrap_or(heuristic_val),
    }
}

/// Rejects a `BuildConfig` that can't be applied consistently, before any
/// observation is folded in. Checked once per build/eval run, not per
/// observation.
fn validate_config(config: &BuildConfig) -> Result<()> {
    if let Some(half_life) = config.time_decay_half_life_days {
        if !(half_life.is_finite() && half_life > 0.0) {
            return Err(Error::InvalidConfig {
                message: format!("time_decay_half_life_days must be > 0, got {half_life}"),
            });
        }
        if config.time_decay_reference_unix_seconds.is_none() {
            return Err(Error::InvalidConfig {
                message: "time_decay_reference_unix_seconds is required when \
                          time_decay_half_life_days is set"
                    .to_string(),
            });
        }
    }
    for (name, weight) in &config.source_weights {
        if !weight.is_finite() || *weight < 0.0 {
            return Err(Error::InvalidConfig {
                message: format!("source_weights[{name:?}] must be finite and >= 0, got {weight}"),
            });
        }
    }
    if !config.default_source_weight.is_finite() || config.default_source_weight < 0.0 {
        return Err(Error::InvalidConfig {
            message: format!(
                "default_source_weight must be finite and >= 0, got {}",
                config.default_source_weight
            ),
        });
    }
    Ok(())
}

/// `obs.weight` after time-decay and source-reliability multipliers, or
/// `None` if `obs` should be dropped entirely (missing timestamp under
/// `MissingTimestampPolicy::Drop`, with time decay enabled).
fn effective_weight(obs: &Observation, config: &BuildConfig) -> Option<f64> {
    let mut weight = obs.weight;

    if let Some(half_life_days) = config.time_decay_half_life_days {
        let reference = config
            .time_decay_reference_unix_seconds
            .expect("validate_config requires this whenever time_decay_half_life_days is set");
        match obs.observed_at_unix_seconds {
            Some(observed_at) => {
                let age_days = ((reference - observed_at) as f64 / 86_400.0).max(0.0);
                weight *= time_decay_multiplier(age_days, half_life_days);
            }
            None => match config.missing_timestamp_policy {
                MissingTimestampPolicy::KeepBaseWeight => {}
                MissingTimestampPolicy::Drop => return None,
            },
        }
    }

    let source_multiplier = obs
        .source
        .as_deref()
        .and_then(|name| config.source_weights.get(name))
        .copied()
        .unwrap_or(config.default_source_weight);
    weight *= source_multiplier;

    Some(weight)
}

/// What a build actually did to the input, independent of the resulting
/// [`PriorBook`] -- lets a caller check whether their own pre-filtering
/// (e.g. a domain-specific ply/depth cutoff) combined with `BuildConfig`'s
/// thresholds behaved as expected, without re-deriving these numbers by
/// hand from the input.
///
/// Invariant:
///
/// ```text
/// candidates_before_filtering
///     == candidates_kept
///     + candidates_dropped_by_min_count
///     + candidates_dropped_by_min_weighted_count
///     + candidates_dropped_by_min_confidence
///     + candidates_dropped_by_max_actions_per_state
/// ```
///
/// A candidate failing more than one of the first three thresholds is
/// counted against whichever it hits first, in that order (matching the
/// order they're applied).
///
/// Separately, every observation read is accounted for by:
///
/// ```text
/// observations_kept
///     + observations_dropped_by_step_or_tag_filter
///     + observations_dropped_by_missing_timestamp
/// ```
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct BuildStats {
    /// Observations folded into the accumulator (survived `max_step` /
    /// `tag_filter` / the missing-timestamp policy below).
    pub observations_kept: u64,
    /// Observations dropped by `max_step` or `tag_filter` before ever
    /// contributing to any `(state, action)` pair's statistics.
    pub observations_dropped_by_step_or_tag_filter: u64,
    /// Observations dropped because they had no `observed_at_unix_seconds`
    /// under `MissingTimestampPolicy::Drop`. Always `0` when
    /// `time_decay_half_life_days` is unset -- timestamps are only
    /// consulted when time decay is enabled.
    pub observations_dropped_by_missing_timestamp: u64,
    /// Distinct `(state, action)` pairs that accumulated at least one
    /// observation, before `min_count` / `min_weighted_count` /
    /// `min_confidence` / `max_actions_per_state`.
    pub candidates_before_filtering: u64,
    pub candidates_dropped_by_min_count: u64,
    pub candidates_dropped_by_min_weighted_count: u64,
    pub candidates_dropped_by_min_confidence: u64,
    /// Candidates that passed every threshold above but were truncated by
    /// `max_actions_per_state`'s per-state cap.
    pub candidates_dropped_by_max_actions_per_state: u64,
    /// `(state, action)` pairs that made it into the final book.
    pub candidates_kept: u64,
}

/// Folds observations into a [`PriorBook`] one at a time, so memory stays
/// bounded by the number of unique `(state, action)` pairs rather than the
/// number of observations fed in. This is what lets [`crate::input::build_prior_book_from_reader`]
/// stream a JSONL file straight into a prior book without ever collecting
/// a `Vec<Observation>` -- the same accumulator backs both that streaming
/// entry point and the eager [`build_prior_book`] below, so they can never
/// drift apart in scoring behavior.
///
/// Smoothing needs the dataset-wide rate before any single action's score
/// can be finalized, so this is a two-phase object: [`Self::observe`] folds
/// in per-action totals *and* dataset-wide totals as observations arrive,
/// then [`Self::finish`] uses the now-complete dataset-wide totals as the
/// smoothing target for every action.
pub(crate) struct PriorAccumulator<'a> {
    stats: HashMap<String, HashMap<String, ActionStats>>,
    /// Order `1..=config.context_order` entries only -- order-0 stays
    /// exclusively in `stats`, never duplicated here. Keyed by
    /// `(context_window_slice, state)`.
    context_stats: HashMap<(Vec<String>, String), HashMap<String, ActionStats>>,
    context_tracker: SequenceContextTracker,
    global_weighted_successes: f64,
    global_weighted_trials: f64,
    global_weighted_score_sum: f64,
    global_weighted_score_count: f64,
    observations_kept: u64,
    observations_dropped_by_step_or_tag_filter: u64,
    observations_dropped_by_missing_timestamp: u64,
    config: &'a BuildConfig,
}

impl<'a> PriorAccumulator<'a> {
    pub(crate) fn new(config: &'a BuildConfig) -> Result<Self> {
        validate_config(config)?;
        Ok(Self {
            stats: HashMap::new(),
            context_stats: HashMap::new(),
            context_tracker: SequenceContextTracker::new(config.context_order),
            global_weighted_successes: 0.0,
            global_weighted_trials: 0.0,
            global_weighted_score_sum: 0.0,
            global_weighted_score_count: 0.0,
            observations_kept: 0,
            observations_dropped_by_step_or_tag_filter: 0,
            observations_dropped_by_missing_timestamp: 0,
            config,
        })
    }

    pub(crate) fn observe(&mut self, obs: &Observation) -> Result<()> {
        // Validated (and the window advanced) unconditionally, ahead of any
        // filter below: sortedness is a structural property of the whole
        // input, and the window must reflect every action actually taken in
        // the sequence, not just the ones that happen to survive filtering.
        let window = self.context_tracker.advance(obs)?;

        if let Some(max_step) = self.config.max_step
            && obs.step > max_step
        {
            self.observations_dropped_by_step_or_tag_filter += 1;
            return Ok(());
        }
        if let Some(required_tags) = &self.config.tag_filter
            && !required_tags.iter().any(|tag| obs.tags.contains(tag))
        {
            self.observations_dropped_by_step_or_tag_filter += 1;
            return Ok(());
        }
        let Some(effective_weight) = effective_weight(obs, self.config) else {
            self.observations_dropped_by_missing_timestamp += 1;
            return Ok(());
        };
        self.observations_kept += 1;

        let entry = self
            .stats
            .entry(obs.state.clone())
            .or_default()
            .entry(obs.action.clone())
            .or_default();
        entry.count += 1;
        entry.weighted_count += effective_weight;

        if obs.outcome != Outcome::Unknown {
            entry.weighted_trials += effective_weight;
            entry.weighted_trial_weight_sq_sum += effective_weight * effective_weight;
            self.global_weighted_trials += effective_weight;
            // A draw earns partial credit rather than scoring like a loss;
            // both the per-action and dataset-wide totals must move
            // together since the latter is the smoothing target the
            // former shrinks toward.
            let success_credit = outcome_credit(obs.outcome, self.config.draw_value);
            entry.weighted_successes += success_credit * effective_weight;
            self.global_weighted_successes += success_credit * effective_weight;
        }

        if let Some(score) = obs.score {
            entry.weighted_score_sum += effective_weight * score;
            entry.weighted_score_count += effective_weight;
            self.global_weighted_score_sum += effective_weight * score;
            self.global_weighted_score_count += effective_weight;
        }

        // Order 1..=window.len() (already bounded to config.context_order by
        // the tracker). These fold into the *same* per-action totals shape
        // as order-0 above, but never touch the global_* sums -- the global
        // smoothing target is dataset-wide and must be counted exactly once
        // per observation, not once per context order.
        for len in 1..=window.len() {
            let context = window[window.len() - len..].to_vec();
            let entry = self
                .context_stats
                .entry((context, obs.state.clone()))
                .or_default()
                .entry(obs.action.clone())
                .or_default();
            entry.count += 1;
            entry.weighted_count += effective_weight;

            if obs.outcome != Outcome::Unknown {
                entry.weighted_trials += effective_weight;
                entry.weighted_trial_weight_sq_sum += effective_weight * effective_weight;
                let success_credit = outcome_credit(obs.outcome, self.config.draw_value);
                entry.weighted_successes += success_credit * effective_weight;
            }

            if let Some(score) = obs.score {
                entry.weighted_score_sum += effective_weight * score;
                entry.weighted_score_count += effective_weight;
            }
        }

        Ok(())
    }

    /// Always succeeds -- an accumulator that never observed anything (or
    /// whose observations were entirely filtered out) simply yields an
    /// empty [`PriorBook`]. Whether "empty" should be treated as an error
    /// is a decision for each caller, not this shared core: the eager
    /// [`build_prior_book`] below turns it into [`Error::NoObservations`]
    /// for backward compatibility, while the streaming
    /// [`crate::input::build_prior_book_from_reader`] returns it as-is so
    /// that any warnings collected during parsing are never discarded
    /// just because the result happened to end up empty.
    ///
    /// Thin wrapper over [`Self::finish_with_stats`] for callers that
    /// don't need [`BuildStats`], so there's one real implementation.
    pub(crate) fn finish(self) -> PriorBook {
        self.finish_with_stats().0
    }

    /// Like [`Self::finish`], but also reports what got dropped and why
    /// (see [`BuildStats`]).
    pub(crate) fn finish_with_stats(self) -> (PriorBook, BuildStats) {
        // `None` here means "this dataset has no outcome/score data at
        // all", which drops that scoring term for every action rather
        // than treating a single action's missing data as a bad (zero)
        // signal. Every context order (1..=config.context_order) shrinks
        // toward this *same* dataset-wide target, not a per-order or
        // hierarchical one -- stupid backoff was chosen over interpolation
        // smoothing specifically for transparency, so every level answers
        // to the same flat baseline.
        let global_success_rate =
            ratio(self.global_weighted_successes, self.global_weighted_trials);
        let global_mean_score = ratio(
            self.global_weighted_score_sum,
            self.global_weighted_score_count,
        );

        let mut counters = FinalizeCounters::default();

        let mut entries: HashMap<String, Vec<PriorAction>> = HashMap::new();
        for (state, actions) in self.stats {
            if let Some(actions_out) = finalize_actions(
                actions,
                self.config,
                global_success_rate,
                global_mean_score,
                &mut counters,
            ) {
                entries.insert(state, actions_out);
            }
        }

        let mut context_entries: HashMap<(Vec<String>, String), Vec<PriorAction>> = HashMap::new();
        for (key, actions) in self.context_stats {
            if let Some(actions_out) = finalize_actions(
                actions,
                self.config,
                global_success_rate,
                global_mean_score,
                &mut counters,
            ) {
                context_entries.insert(key, actions_out);
            }
        }

        let candidates_kept: u64 = entries.values().map(|v| v.len() as u64).sum::<u64>()
            + context_entries
                .values()
                .map(|v| v.len() as u64)
                .sum::<u64>();
        let stats = BuildStats {
            observations_kept: self.observations_kept,
            observations_dropped_by_step_or_tag_filter: self
                .observations_dropped_by_step_or_tag_filter,
            observations_dropped_by_missing_timestamp: self
                .observations_dropped_by_missing_timestamp,
            candidates_before_filtering: counters.candidates_before_filtering,
            candidates_dropped_by_min_count: counters.candidates_dropped_by_min_count,
            candidates_dropped_by_min_weighted_count: counters
                .candidates_dropped_by_min_weighted_count,
            candidates_dropped_by_min_confidence: counters.candidates_dropped_by_min_confidence,
            candidates_dropped_by_max_actions_per_state: counters
                .candidates_dropped_by_max_actions_per_state,
            candidates_kept,
        };

        (
            PriorBook {
                entries,
                context_entries,
            },
            stats,
        )
    }
}

/// Candidate-level counter deltas threaded through [`finalize_actions`] --
/// shared by the order-0 pass over `stats` and every context-order pass
/// over `context_stats`, so [`BuildStats`] reports one rolled-up total
/// across every order rather than duplicating a whole counter set per
/// order. Inert (all-zero) whenever `context_order == 0`, since
/// `context_stats` is empty in that case.
#[derive(Default)]
struct FinalizeCounters {
    candidates_before_filtering: u64,
    candidates_dropped_by_min_count: u64,
    candidates_dropped_by_min_weighted_count: u64,
    candidates_dropped_by_min_confidence: u64,
    candidates_dropped_by_max_actions_per_state: u64,
}

/// Filters, smooths, normalizes, and ranks one key's (a state, or a
/// `(context, state)` pair) `action -> ActionStats` map into ranked
/// [`PriorAction`]s, against the given dataset-wide smoothing targets.
/// Returns `None` if every candidate was filtered out. This is the single
/// implementation [`PriorAccumulator::finish_with_stats`] calls once per
/// state (order-0) and once per context-order key, so the filter/smooth/
/// normalize logic never has to be duplicated per order.
fn finalize_actions(
    actions: HashMap<String, ActionStats>,
    config: &BuildConfig,
    global_success_rate: Option<f64>,
    global_mean_score: Option<f64>,
    counters: &mut FinalizeCounters,
) -> Option<Vec<PriorAction>> {
    counters.candidates_before_filtering += actions.len() as u64;

    // Explicit classify-loop rather than three chained `.filter()` calls:
    // behaviorally identical (a chained filter already short-circuits per
    // item at the first failing predicate), but this lets each dropped
    // candidate be counted against the specific threshold that rejected it.
    let mut kept: Vec<(String, ActionStats)> = Vec::new();
    for (action, stat) in actions {
        if stat.count < config.min_count {
            counters.candidates_dropped_by_min_count += 1;
            continue;
        }
        if stat.weighted_count < config.min_weighted_count {
            counters.candidates_dropped_by_min_weighted_count += 1;
            continue;
        }
        if action_confidence(&stat, config) < config.min_confidence {
            counters.candidates_dropped_by_min_confidence += 1;
            continue;
        }
        kept.push((action, stat));
    }
    if kept.is_empty() {
        return None;
    }

    let raw_scores: Vec<f64> = kept
        .iter()
        .map(|(_, stat)| {
            let smoothed_success = global_success_rate.map(|global| {
                shrink_toward(
                    stat.weighted_successes,
                    stat.weighted_trials,
                    config.smoothing_alpha,
                    global,
                )
            });
            let smoothed_score = global_mean_score.map(|global| {
                shrink_toward(
                    stat.weighted_score_sum,
                    stat.weighted_score_count,
                    config.smoothing_alpha,
                    global,
                )
            });
            raw_score(
                stat.weighted_count,
                smoothed_success,
                smoothed_score,
                config,
            )
        })
        .collect();

    let priors = normalize(&raw_scores);

    let mut actions_out: Vec<PriorAction> = kept
        .into_iter()
        .zip(priors)
        .map(|((action, stat), prior)| PriorAction {
            action,
            count: stat.count,
            weighted_count: stat.weighted_count,
            success_rate: ratio(stat.weighted_successes, stat.weighted_trials),
            mean_score: ratio(stat.weighted_score_sum, stat.weighted_score_count),
            prior,
            confidence: action_confidence(&stat, config),
        })
        .collect();

    if let Some(max_actions) = config.max_actions_per_state {
        actions_out.sort_by(|a, b| {
            b.prior
                .partial_cmp(&a.prior)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.action.cmp(&b.action))
        });
        if actions_out.len() > max_actions {
            counters.candidates_dropped_by_max_actions_per_state +=
                (actions_out.len() - max_actions) as u64;
        }
        actions_out.truncate(max_actions);
    }

    Some(actions_out)
}

/// Aggregates observations into a [`PriorBook`], applying filters,
/// smoothing, normalization, and ranking per AGENTS.md's scoring model.
///
/// This eager form takes an already-collected slice; prefer
/// [`crate::input::build_prior_book_from_reader`] when reading directly
/// from a JSONL source, since it folds each observation in as it's parsed
/// instead of holding them all in memory first.
pub fn build_prior_book(observations: &[Observation], config: &BuildConfig) -> Result<PriorBook> {
    if observations.is_empty() {
        return Err(Error::NoObservations);
    }

    let mut acc = PriorAccumulator::new(config)?;
    for obs in observations {
        acc.observe(obs)?;
    }
    let book = acc.finish();

    if book.entries.is_empty() {
        return Err(Error::NoObservations);
    }

    Ok(book)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test-only helper; a builder would be overkill for fixture data.
    #[allow(clippy::too_many_arguments)]
    fn obs(
        sequence_id: &str,
        step: u32,
        state: &str,
        action: &str,
        outcome: Outcome,
        score: Option<f64>,
        weight: f64,
        tags: Vec<&str>,
    ) -> Observation {
        Observation {
            sequence_id: sequence_id.to_string(),
            step,
            state: state.to_string(),
            action: action.to_string(),
            outcome,
            score,
            weight,
            tags: tags.into_iter().map(str::to_string).collect(),
            observed_at_unix_seconds: None,
            source: None,
        }
    }

    fn action<'a>(book: &'a PriorBook, state: &str, action: &str) -> &'a PriorAction {
        book.entries[state]
            .iter()
            .find(|a| a.action == action)
            .unwrap()
    }

    #[test]
    fn aggregates_counts_and_weighted_counts() {
        let observations = vec![
            obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![]),
            obs("c2", 0, "s", "a", Outcome::Unknown, None, 2.5, vec![]),
        ];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        let a = action(&book, "s", "a");
        assert_eq!(a.count, 2);
        assert_eq!(a.weighted_count, 3.5);
    }

    #[test]
    fn computes_success_rate_and_mean_score() {
        let observations = vec![
            obs("c1", 0, "s", "a", Outcome::Success, Some(1.0), 1.0, vec![]),
            obs("c2", 0, "s", "a", Outcome::Failure, Some(0.0), 1.0, vec![]),
        ];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        let a = action(&book, "s", "a");
        assert_eq!(a.success_rate, Some(0.5));
        assert_eq!(a.mean_score, Some(0.5));
    }

    #[test]
    fn applies_min_count_filter() {
        let observations = vec![
            obs("c1", 0, "s", "rare", Outcome::Unknown, None, 1.0, vec![]),
            obs("c2", 0, "s", "common", Outcome::Unknown, None, 1.0, vec![]),
            obs("c3", 0, "s", "common", Outcome::Unknown, None, 1.0, vec![]),
        ];
        let config = BuildConfig {
            min_count: 2,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        assert!(book.entries["s"].iter().all(|a| a.action != "rare"));
        assert!(book.entries["s"].iter().any(|a| a.action == "common"));
    }

    #[test]
    fn applies_min_weighted_count_filter_independent_of_raw_count() {
        let observations = vec![
            obs(
                "c1",
                0,
                "s",
                "many_tiny",
                Outcome::Unknown,
                None,
                0.01,
                vec![],
            ),
            obs(
                "c2",
                0,
                "s",
                "many_tiny",
                Outcome::Unknown,
                None,
                0.01,
                vec![],
            ),
            obs(
                "c3",
                0,
                "s",
                "few_heavy",
                Outcome::Unknown,
                None,
                10.0,
                vec![],
            ),
        ];
        let config = BuildConfig {
            min_weighted_count: 1.0,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        assert!(book.entries["s"].iter().all(|a| a.action != "many_tiny"));
        assert!(book.entries["s"].iter().any(|a| a.action == "few_heavy"));
    }

    #[test]
    fn applies_min_confidence_filter() {
        let observations = vec![
            obs(
                "c1",
                0,
                "s",
                "unproven",
                Outcome::Unknown,
                None,
                1.0,
                vec![],
            ),
            obs(
                "c2",
                0,
                "s",
                "proven",
                Outcome::Unknown,
                None,
                100.0,
                vec![],
            ),
        ];
        let config = BuildConfig {
            min_confidence: 0.5,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        assert!(book.entries["s"].iter().all(|a| a.action != "unproven"));
        assert!(book.entries["s"].iter().any(|a| a.action == "proven"));
    }

    #[test]
    fn confidence_mode_heuristic_matches_pre_existing_formula() {
        // BuildConfig::default() is ConfidenceMode::Heuristic -- confirms
        // this feature is purely additive for existing callers.
        let observations = vec![
            obs("c1", 0, "s", "a", Outcome::Success, None, 1.0, vec![]),
            obs("c2", 0, "s", "a", Outcome::Failure, None, 1.0, vec![]),
        ];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        let got = action(&book, "s", "a").confidence;
        assert_eq!(got, confidence(2.0, 20.0)); // 20.0 == DEFAULT_CONFIDENCE_K
    }

    #[test]
    fn confidence_mode_wilson_lower_bound_ranks_by_success_rate_at_equal_count() {
        let mut observations = Vec::new();
        for i in 0..18 {
            observations.push(obs(
                &format!("good_s_{i}"),
                0,
                "s",
                "good",
                Outcome::Success,
                None,
                1.0,
                vec![],
            ));
        }
        for i in 0..2 {
            observations.push(obs(
                &format!("good_f_{i}"),
                0,
                "s",
                "good",
                Outcome::Failure,
                None,
                1.0,
                vec![],
            ));
        }
        for i in 0..2 {
            observations.push(obs(
                &format!("bad_s_{i}"),
                0,
                "s",
                "bad",
                Outcome::Success,
                None,
                1.0,
                vec![],
            ));
        }
        for i in 0..18 {
            observations.push(obs(
                &format!("bad_f_{i}"),
                0,
                "s",
                "bad",
                Outcome::Failure,
                None,
                1.0,
                vec![],
            ));
        }

        // Both actions have identical count/weighted_count (20), so under
        // Heuristic mode their confidence is identical -- any difference
        // seen under WilsonLowerBound must come from success rate alone.
        let heuristic_book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert_eq!(
            action(&heuristic_book, "s", "good").confidence,
            action(&heuristic_book, "s", "bad").confidence
        );

        let wilson_config = BuildConfig {
            confidence_mode: ConfidenceMode::WilsonLowerBound,
            ..Default::default()
        };
        let wilson_book = build_prior_book(&observations, &wilson_config).unwrap();
        let good = action(&wilson_book, "s", "good").confidence;
        let bad = action(&wilson_book, "s", "bad").confidence;
        assert!(
            good > bad,
            "good={good} should outrank bad={bad} under WilsonLowerBound"
        );
    }

    #[test]
    fn confidence_mode_hybrid_multiplies_heuristic_and_wilson() {
        let mut observations = Vec::new();
        for i in 0..18 {
            observations.push(obs(
                &format!("s{i}"),
                0,
                "s",
                "a",
                Outcome::Success,
                None,
                1.0,
                vec![],
            ));
        }
        for i in 0..2 {
            observations.push(obs(
                &format!("f{i}"),
                0,
                "s",
                "a",
                Outcome::Failure,
                None,
                1.0,
                vec![],
            ));
        }
        let config = BuildConfig {
            confidence_mode: ConfidenceMode::Hybrid,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        let got = action(&book, "s", "a").confidence;

        // Uniform weight=1.0, 20 trials: n_eff == 20, p_hat == 18/20 == 0.9.
        let heuristic_val = confidence(20.0, config.confidence_k);
        let wilson_val = wilson_lower_bound(18.0, 20.0, config.confidence_z).unwrap();
        assert!((got - heuristic_val * wilson_val).abs() < 1e-9);
    }

    #[test]
    fn min_confidence_filter_behavior_depends_on_confidence_mode() {
        // "risky": count=20, mostly failing. "safe": count=20, mostly
        // succeeding. Both have weighted_count=20 -> identical heuristic
        // confidence (0.5 at the default k=20), but very different Wilson
        // lower bounds.
        let mut observations = Vec::new();
        for i in 0..2 {
            observations.push(obs(
                &format!("rs{i}"),
                0,
                "s",
                "risky",
                Outcome::Success,
                None,
                1.0,
                vec![],
            ));
        }
        for i in 0..18 {
            observations.push(obs(
                &format!("rf{i}"),
                0,
                "s",
                "risky",
                Outcome::Failure,
                None,
                1.0,
                vec![],
            ));
        }
        for i in 0..18 {
            observations.push(obs(
                &format!("ss{i}"),
                0,
                "s",
                "safe",
                Outcome::Success,
                None,
                1.0,
                vec![],
            ));
        }
        for i in 0..2 {
            observations.push(obs(
                &format!("sf{i}"),
                0,
                "s",
                "safe",
                Outcome::Failure,
                None,
                1.0,
                vec![],
            ));
        }

        let heuristic_config = BuildConfig {
            min_confidence: 0.5,
            ..Default::default()
        };
        let heuristic_book = build_prior_book(&observations, &heuristic_config).unwrap();
        assert!(
            heuristic_book.entries["s"]
                .iter()
                .any(|a| a.action == "risky"),
            "heuristic min_confidence is blind to outcome, so risky should survive"
        );

        let wilson_config = BuildConfig {
            min_confidence: 0.5,
            confidence_mode: ConfidenceMode::WilsonLowerBound,
            ..Default::default()
        };
        let wilson_book = build_prior_book(&observations, &wilson_config).unwrap();
        assert!(
            wilson_book.entries["s"].iter().all(|a| a.action != "risky"),
            "wilson-lower-bound min_confidence should drop a mostly-failing action at the same threshold"
        );
        assert!(wilson_book.entries["s"].iter().any(|a| a.action == "safe"));
    }

    #[test]
    fn draw_value_contributes_fractional_success_under_wilson_mode() {
        // 18 draws + 2 failures, weight 1 each; default draw_value=0.5
        // credits draws as half-success, so p_hat = (18*0.5)/20 = 0.45.
        let mut observations = Vec::new();
        for i in 0..18 {
            observations.push(obs(
                &format!("d{i}"),
                0,
                "s",
                "a",
                Outcome::Draw,
                None,
                1.0,
                vec![],
            ));
        }
        for i in 0..2 {
            observations.push(obs(
                &format!("f{i}"),
                0,
                "s",
                "a",
                Outcome::Failure,
                None,
                1.0,
                vec![],
            ));
        }
        let config = BuildConfig {
            confidence_mode: ConfidenceMode::WilsonLowerBound,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        let got = action(&book, "s", "a").confidence;
        let expected = wilson_lower_bound(9.0, 20.0, config.confidence_z).unwrap();
        assert!((got - expected).abs() < 1e-9);
    }

    #[test]
    fn action_confidence_does_not_produce_nan_when_draw_value_exceeds_one() {
        let observations = vec![
            obs("d1", 0, "s", "a", Outcome::Draw, None, 1.0, vec![]),
            obs("d2", 0, "s", "a", Outcome::Draw, None, 1.0, vec![]),
        ];
        let config = BuildConfig {
            draw_value: 1.5, // out of the documented [0.0, 1.0] range
            confidence_mode: ConfidenceMode::WilsonLowerBound,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        let got = action(&book, "s", "a").confidence;
        assert!(got.is_finite());
        assert!((0.0..=1.0).contains(&got));
    }

    #[test]
    fn draw_earns_partial_success_credit_via_draw_value() {
        let observations = vec![
            obs("c1", 0, "s", "a", Outcome::Success, None, 1.0, vec![]),
            obs("c2", 0, "s", "a", Outcome::Draw, None, 1.0, vec![]),
        ];
        // Default draw_value = 0.5: one success + one draw over 2 trials
        // is credited as (1.0 + 0.5) / 2 = 0.75, not 0.5 (draw-as-loss).
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert_eq!(action(&book, "s", "a").success_rate, Some(0.75));
    }

    #[test]
    fn draw_value_zero_reproduces_draw_as_failure_behavior() {
        let observations = vec![
            obs("c1", 0, "s", "a", Outcome::Success, None, 1.0, vec![]),
            obs("c2", 0, "s", "a", Outcome::Draw, None, 1.0, vec![]),
        ];
        let config = BuildConfig {
            draw_value: 0.0,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        assert_eq!(action(&book, "s", "a").success_rate, Some(0.5));
    }

    #[test]
    fn draw_credit_also_shifts_the_shared_global_smoothing_rate() {
        // "steady" has a plain 1-success/1-failure record untouched by
        // draw_value directly. "drawer" has a single draw. Both actions
        // share the same state, so their priors are normalized against
        // each other -- if draw credit only applied locally to "drawer"
        // and never reached the *global* rate, "steady"'s prior share
        // would stay fixed as draw_value changes. It shouldn't.
        let observations = vec![
            obs("c1", 0, "s", "steady", Outcome::Success, None, 1.0, vec![]),
            obs("c2", 0, "s", "steady", Outcome::Failure, None, 1.0, vec![]),
            obs("c3", 0, "s", "drawer", Outcome::Draw, None, 1.0, vec![]),
        ];
        let base = BuildConfig {
            count_weight: 0.0,
            score_weight: 0.0,
            success_weight: 1.0,
            smoothing_alpha: 5.0,
            ..Default::default()
        };

        let credited_as_failure = build_prior_book(
            &observations,
            &BuildConfig {
                draw_value: 0.0,
                ..base.clone()
            },
        )
        .unwrap();
        let credited_as_partial = build_prior_book(
            &observations,
            &BuildConfig {
                draw_value: 0.5,
                ..base
            },
        )
        .unwrap();

        // "steady" never has a draw of its own, so if draw credit didn't
        // reach the global accumulator, its prior would be identical in
        // both runs. It isn't: expected 48/83 with draw_value=0 vs 0.5
        // with draw_value=0.5 (worked out from the smoothing formula).
        let steady_prior_failure = action(&credited_as_failure, "s", "steady").prior;
        let steady_prior_partial = action(&credited_as_partial, "s", "steady").prior;
        assert!((steady_prior_failure - 48.0 / 83.0).abs() < 1e-6);
        assert!((steady_prior_partial - 0.5).abs() < 1e-6);
        assert!((steady_prior_failure - steady_prior_partial).abs() > 0.05);
    }

    #[test]
    fn smoothing_pulls_a_lone_success_toward_the_global_rate() {
        let observations = vec![
            obs("c1", 0, "s", "lucky", Outcome::Success, None, 1.0, vec![]),
            obs("c2", 0, "s", "steady", Outcome::Success, None, 1.0, vec![]),
            obs("c3", 0, "s", "steady", Outcome::Failure, None, 1.0, vec![]),
            obs("c4", 0, "s", "steady", Outcome::Success, None, 1.0, vec![]),
            obs("c5", 0, "s", "steady", Outcome::Failure, None, 1.0, vec![]),
        ];
        let base = BuildConfig {
            count_weight: 0.0,
            score_weight: 0.0,
            success_weight: 1.0,
            min_count: 1,
            ..Default::default()
        };

        let unsmoothed = build_prior_book(
            &observations,
            &BuildConfig {
                smoothing_alpha: 0.0,
                ..base.clone()
            },
        )
        .unwrap();
        let heavily_smoothed = build_prior_book(
            &observations,
            &BuildConfig {
                smoothing_alpha: 50.0,
                ..base
            },
        )
        .unwrap();

        let lucky_unsmoothed = action(&unsmoothed, "s", "lucky").prior;
        let lucky_smoothed = action(&heavily_smoothed, "s", "lucky").prior;
        assert!(
            lucky_smoothed < lucky_unsmoothed,
            "smoothing should temper a lone perfect record: {lucky_smoothed} should be < {lucky_unsmoothed}"
        );
    }

    #[test]
    fn normalizes_priors_to_sum_to_one_per_state() {
        let observations = vec![
            obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![]),
            obs("c2", 0, "s", "b", Outcome::Unknown, None, 3.0, vec![]),
        ];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        let sum: f64 = book.entries["s"].iter().map(|a| a.prior).sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn deterministic_output_ordering_is_stable_across_runs() {
        let observations = vec![
            obs("c1", 0, "s2", "a", Outcome::Unknown, None, 1.0, vec![]),
            obs("c2", 0, "s1", "b", Outcome::Unknown, None, 5.0, vec![]),
            obs("c3", 0, "s1", "a", Outcome::Unknown, None, 1.0, vec![]),
        ];
        let config = BuildConfig::default();
        let first = build_prior_book(&observations, &config).unwrap();
        let second = build_prior_book(&observations, &config).unwrap();

        let first_json = serde_json::to_string(&first.entries_sorted()).unwrap();
        let second_json = serde_json::to_string(&second.entries_sorted()).unwrap();
        assert_eq!(first_json, second_json);

        let sorted = first.entries_sorted();
        assert_eq!(sorted[0].state, "s1");
        assert_eq!(sorted[1].state, "s2");
        // Within s1, "b" (weight 5) outranks "a" (weight 1).
        assert_eq!(sorted[0].actions[0].action, "b");
    }

    #[test]
    fn query_unseen_state_returns_no_candidates() {
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert!(book.query("nonexistent", None).is_empty());
    }

    #[test]
    fn query_known_state_returns_candidates() {
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert_eq!(book.query("s", None).len(), 1);
    }

    #[test]
    fn empty_input_is_an_error() {
        let err = build_prior_book(&[], &BuildConfig::default()).unwrap_err();
        assert!(matches!(err, Error::NoObservations));
    }

    #[test]
    fn all_unknown_outcomes_drops_success_rate_entirely() {
        let observations = vec![
            obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![]),
            obs("c2", 0, "s", "b", Outcome::Unknown, None, 1.0, vec![]),
        ];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert!(book.entries["s"].iter().all(|a| a.success_rate.is_none()));
    }

    #[test]
    fn all_failures_reports_zero_success_rate_not_none() {
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Failure, None, 1.0, vec![])];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert_eq!(action(&book, "s", "a").success_rate, Some(0.0));
    }

    #[test]
    fn all_successes_reports_full_success_rate() {
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Success, None, 1.0, vec![])];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert_eq!(action(&book, "s", "a").success_rate, Some(1.0));
    }

    #[test]
    fn one_observation_builds_successfully() {
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert_eq!(action(&book, "s", "a").count, 1);
    }

    #[test]
    fn extremely_large_counts_do_not_overflow_or_panic() {
        let observations: Vec<Observation> = (0..50_000)
            .map(|i| {
                obs(
                    &format!("c{i}"),
                    0,
                    "s",
                    "a",
                    Outcome::Success,
                    None,
                    1.0,
                    vec![],
                )
            })
            .collect();
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        let a = action(&book, "s", "a");
        assert_eq!(a.count, 50_000);
        assert!(a.confidence > 0.999);
    }

    #[test]
    fn duplicate_sequence_ids_are_all_counted() {
        let observations = vec![
            obs("dup", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![]),
            obs("dup", 1, "s", "a", Outcome::Unknown, None, 1.0, vec![]),
        ];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert_eq!(action(&book, "s", "a").count, 2);
    }

    #[test]
    fn multiple_actions_per_state_all_appear() {
        let observations = vec![
            obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![]),
            obs("c2", 0, "s", "b", Outcome::Unknown, None, 1.0, vec![]),
            obs("c3", 0, "s", "c", Outcome::Unknown, None, 1.0, vec![]),
        ];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert_eq!(book.entries["s"].len(), 3);
    }

    #[test]
    fn max_step_filters_out_later_steps() {
        let observations = vec![
            obs("c1", 0, "s", "early", Outcome::Unknown, None, 1.0, vec![]),
            obs("c2", 99, "s", "late", Outcome::Unknown, None, 1.0, vec![]),
        ];
        let config = BuildConfig {
            max_step: Some(10),
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        assert!(book.entries["s"].iter().all(|a| a.action != "late"));
    }

    #[test]
    fn tag_filter_keeps_only_matching_observations() {
        let observations = vec![
            obs(
                "c1",
                0,
                "s",
                "trusted",
                Outcome::Unknown,
                None,
                1.0,
                vec!["trusted"],
            ),
            obs(
                "c2",
                0,
                "s",
                "untrusted",
                Outcome::Unknown,
                None,
                1.0,
                vec![],
            ),
        ];
        let config = BuildConfig {
            tag_filter: Some(vec!["trusted".to_string()]),
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        assert!(book.entries["s"].iter().all(|a| a.action != "untrusted"));
    }

    #[test]
    fn no_observations_survive_filtering_is_an_error() {
        let observations = vec![obs("c1", 99, "s", "a", Outcome::Unknown, None, 1.0, vec![])];
        let config = BuildConfig {
            max_step: Some(1),
            ..Default::default()
        };
        let err = build_prior_book(&observations, &config).unwrap_err();
        assert!(matches!(err, Error::NoObservations));
    }

    #[test]
    fn build_stats_invariant_holds_and_every_drop_bucket_is_exact() {
        let mut observations = vec![
            // Dropped by max_step before ever becoming a candidate at all.
            obs(
                "late",
                99,
                "other_state",
                "whatever",
                Outcome::Unknown,
                None,
                1.0,
                vec![],
            ),
            // count=1 < min_count=2.
            obs(
                "r1",
                0,
                "s",
                "too_rare",
                Outcome::Unknown,
                None,
                1.0,
                vec![],
            ),
        ];
        // count=3 (passes min_count), weighted_count=1.5 < min_weighted_count=5.0.
        for i in 0..3 {
            observations.push(obs(
                &format!("l{i}"),
                0,
                "s",
                "too_light",
                Outcome::Unknown,
                None,
                0.5,
                vec![],
            ));
        }
        // count=3, weighted_count=6.0 (passes min_count/min_weighted_count),
        // confidence = 6/(6+20) ~= 0.23 < min_confidence=0.3.
        for i in 0..3 {
            observations.push(obs(
                &format!("u{i}"),
                0,
                "s",
                "unproven",
                Outcome::Unknown,
                None,
                2.0,
                vec![],
            ));
        }
        // Three candidates that survive every threshold; max_actions_per_state=2
        // truncates exactly one of them.
        for name in ["surv_a", "surv_b", "surv_c"] {
            for i in 0..10 {
                observations.push(obs(
                    &format!("{name}_{i}"),
                    0,
                    "s",
                    name,
                    Outcome::Unknown,
                    None,
                    10.0,
                    vec![],
                ));
            }
        }

        let config = BuildConfig {
            min_count: 2,
            min_weighted_count: 5.0,
            min_confidence: 0.3,
            max_step: Some(10),
            max_actions_per_state: Some(2),
            ..Default::default()
        };

        let mut acc = PriorAccumulator::new(&config).unwrap();
        for o in &observations {
            acc.observe(o).unwrap();
        }
        let (book, stats) = acc.finish_with_stats();

        assert_eq!(stats.observations_dropped_by_step_or_tag_filter, 1);
        assert_eq!(stats.observations_kept, observations.len() as u64 - 1);

        assert_eq!(stats.candidates_before_filtering, 6);
        assert_eq!(stats.candidates_dropped_by_min_count, 1);
        assert_eq!(stats.candidates_dropped_by_min_weighted_count, 1);
        assert_eq!(stats.candidates_dropped_by_min_confidence, 1);
        assert_eq!(stats.candidates_dropped_by_max_actions_per_state, 1);
        assert_eq!(stats.candidates_kept, 2);
        assert_eq!(book.entries["s"].len(), 2);

        assert_eq!(
            stats.candidates_before_filtering,
            stats.candidates_kept
                + stats.candidates_dropped_by_min_count
                + stats.candidates_dropped_by_min_weighted_count
                + stats.candidates_dropped_by_min_confidence
                + stats.candidates_dropped_by_max_actions_per_state
        );
    }

    fn decay_config(half_life_days: f64, reference_unix_seconds: i64) -> BuildConfig {
        BuildConfig {
            time_decay_half_life_days: Some(half_life_days),
            time_decay_reference_unix_seconds: Some(reference_unix_seconds),
            ..Default::default()
        }
    }

    #[test]
    fn time_decay_disabled_by_default_matches_pre_existing_weight() {
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])];
        let book = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        assert_eq!(action(&book, "s", "a").weighted_count, 1.0);
    }

    #[test]
    fn observation_one_half_life_old_has_half_weight() {
        let reference = 1_000_000;
        let observed_at = reference - 10 * 86_400; // exactly one half-life (10 days) old
        let observations = vec![Observation {
            observed_at_unix_seconds: Some(observed_at),
            ..obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])
        }];
        let config = decay_config(10.0, reference);
        let book = build_prior_book(&observations, &config).unwrap();
        assert!((action(&book, "s", "a").weighted_count - 0.5).abs() < 1e-9);
    }

    #[test]
    fn observation_two_half_lives_old_has_quarter_weight() {
        let reference = 1_000_000;
        let observed_at = reference - 20 * 86_400; // two half-lives (10 days each) old
        let observations = vec![Observation {
            observed_at_unix_seconds: Some(observed_at),
            ..obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])
        }];
        let config = decay_config(10.0, reference);
        let book = build_prior_book(&observations, &config).unwrap();
        assert!((action(&book, "s", "a").weighted_count - 0.25).abs() < 1e-9);
    }

    #[test]
    fn future_timestamp_clamps_to_full_weight() {
        let reference = 1_000_000;
        let observed_at = reference + 10 * 86_400; // "in the future" relative to reference
        let observations = vec![Observation {
            observed_at_unix_seconds: Some(observed_at),
            ..obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])
        }];
        let config = decay_config(10.0, reference);
        let book = build_prior_book(&observations, &config).unwrap();
        assert_eq!(action(&book, "s", "a").weighted_count, 1.0);
    }

    #[test]
    fn missing_timestamp_keep_base_weight_keeps_observation_at_full_weight() {
        // observed_at_unix_seconds is None on this observation; default
        // MissingTimestampPolicy::KeepBaseWeight should score it as current.
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])];
        let config = decay_config(10.0, 1_000_000);
        let mut acc = PriorAccumulator::new(&config).unwrap();
        for o in &observations {
            acc.observe(o).unwrap();
        }
        let (book, stats) = acc.finish_with_stats();
        assert_eq!(stats.observations_kept, 1);
        assert_eq!(stats.observations_dropped_by_missing_timestamp, 0);
        assert_eq!(action(&book, "s", "a").weighted_count, 1.0);
    }

    #[test]
    fn missing_timestamp_drop_excludes_observation() {
        let observations = vec![
            obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![]), // no timestamp
            Observation {
                observed_at_unix_seconds: Some(1_000_000),
                ..obs("c2", 0, "s", "b", Outcome::Unknown, None, 1.0, vec![])
            },
        ];
        let config = BuildConfig {
            missing_timestamp_policy: MissingTimestampPolicy::Drop,
            ..decay_config(10.0, 1_000_000)
        };
        let mut acc = PriorAccumulator::new(&config).unwrap();
        for o in &observations {
            acc.observe(o).unwrap();
        }
        let (book, stats) = acc.finish_with_stats();
        assert_eq!(stats.observations_dropped_by_missing_timestamp, 1);
        assert_eq!(stats.observations_kept, 1);
        assert!(book.entries["s"].iter().all(|a| a.action != "a"));
        assert!(book.entries["s"].iter().any(|a| a.action == "b"));
    }

    #[test]
    fn source_weight_applies_only_to_matching_source() {
        let observations = vec![
            Observation {
                source: Some("human".to_string()),
                ..obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])
            },
            Observation {
                source: Some("engine".to_string()),
                ..obs("c2", 0, "s", "b", Outcome::Unknown, None, 1.0, vec![])
            },
        ];
        let config = BuildConfig {
            source_weights: std::collections::BTreeMap::from([("human".to_string(), 0.5)]),
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        assert_eq!(action(&book, "s", "a").weighted_count, 0.5);
        assert_eq!(action(&book, "s", "b").weighted_count, 1.0);
    }

    #[test]
    fn unknown_source_uses_default_source_weight() {
        let observations = vec![Observation {
            source: Some("never_configured".to_string()),
            ..obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])
        }];
        let config = BuildConfig {
            source_weights: std::collections::BTreeMap::from([("human".to_string(), 0.5)]),
            default_source_weight: 0.25,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        assert_eq!(action(&book, "s", "a").weighted_count, 0.25);
    }

    #[test]
    fn source_weight_zero_makes_action_invisible_under_min_weighted_count() {
        let observations = vec![
            Observation {
                source: Some("bad".to_string()),
                ..obs(
                    "c1",
                    0,
                    "s",
                    "bad_action",
                    Outcome::Unknown,
                    None,
                    1.0,
                    vec![],
                )
            },
            obs("c2", 0, "s", "keep", Outcome::Unknown, None, 1.0, vec![]),
        ];
        let config = BuildConfig {
            source_weights: std::collections::BTreeMap::from([("bad".to_string(), 0.0)]),
            min_weighted_count: 0.5,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();
        assert!(book.entries["s"].iter().all(|a| a.action != "bad_action"));
        assert!(book.entries["s"].iter().any(|a| a.action == "keep"));
    }

    /// 20 observations per action, all `Success`, differing only in age --
    /// `fresh` sits at `reference` (age 0), `stale` 10 half-lives back.
    fn fresh_and_stale_observations(reference: i64, half_life_days: f64) -> Vec<Observation> {
        let stale_age_days = 10.0 * half_life_days;
        let mut observations = Vec::new();
        for i in 0..20 {
            observations.push(Observation {
                observed_at_unix_seconds: Some(reference),
                ..obs(
                    &format!("fresh_{i}"),
                    0,
                    "s",
                    "fresh",
                    Outcome::Success,
                    None,
                    1.0,
                    vec![],
                )
            });
        }
        for i in 0..20 {
            observations.push(Observation {
                observed_at_unix_seconds: Some(reference - (stale_age_days * 86_400.0) as i64),
                ..obs(
                    &format!("stale_{i}"),
                    0,
                    "s",
                    "stale",
                    Outcome::Success,
                    None,
                    1.0,
                    vec![],
                )
            });
        }
        observations
    }

    /// Kish's effective sample size (`sum(w)^2 / sum(w^2)`) is invariant to
    /// uniformly scaling every weight for an action by the same factor --
    /// so when every one of an action's observations shares the same age,
    /// pure `WilsonLowerBound` confidence does NOT reflect time decay at
    /// all (same success rate, same evenness of trials). Decay still moves
    /// the *ranking* (`prior`, via `weighted_count`) even though it doesn't
    /// move this particular confidence number -- callers who want the
    /// confidence field itself to drop with age need `heuristic` (default)
    /// or `hybrid`, see the next test.
    #[test]
    fn wilson_confidence_is_invariant_to_uniform_time_decay_but_ranking_still_reflects_it() {
        let reference = 1_000_000;
        let half_life_days = 10.0;
        let observations = fresh_and_stale_observations(reference, half_life_days);
        let config = BuildConfig {
            confidence_mode: ConfidenceMode::WilsonLowerBound,
            ..decay_config(half_life_days, reference)
        };
        let book = build_prior_book(&observations, &config).unwrap();
        let fresh = action(&book, "s", "fresh");
        let stale = action(&book, "s", "stale");
        assert_eq!(fresh.confidence, stale.confidence);
        assert!(
            fresh.prior > stale.prior,
            "fresh.prior={} should still outrank stale.prior={}: decay must shrink weighted_count \
             even when it leaves this Wilson confidence number unchanged",
            fresh.prior,
            stale.prior
        );
    }

    #[test]
    fn hybrid_confidence_drops_with_time_decay() {
        let reference = 1_000_000;
        let half_life_days = 10.0;
        let observations = fresh_and_stale_observations(reference, half_life_days);
        let config = BuildConfig {
            confidence_mode: ConfidenceMode::Hybrid,
            ..decay_config(half_life_days, reference)
        };
        let book = build_prior_book(&observations, &config).unwrap();
        let fresh = action(&book, "s", "fresh").confidence;
        let stale = action(&book, "s", "stale").confidence;
        assert!(
            fresh > stale,
            "fresh={fresh} should outrank stale={stale}: the heuristic factor in Hybrid \
             carries weighted_count, so decay must lower confidence here"
        );
    }

    #[test]
    fn validate_config_rejects_non_positive_half_life() {
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])];
        let config = decay_config(0.0, 1_000_000);
        let err = build_prior_book(&observations, &config).unwrap_err();
        assert!(matches!(err, Error::InvalidConfig { .. }));
    }

    #[test]
    fn validate_config_rejects_half_life_without_reference() {
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])];
        let config = BuildConfig {
            time_decay_half_life_days: Some(10.0),
            time_decay_reference_unix_seconds: None,
            ..Default::default()
        };
        let err = build_prior_book(&observations, &config).unwrap_err();
        assert!(matches!(err, Error::InvalidConfig { .. }));
    }

    #[test]
    fn validate_config_rejects_negative_source_weight() {
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])];
        let config = BuildConfig {
            source_weights: std::collections::BTreeMap::from([("x".to_string(), -1.0)]),
            ..Default::default()
        };
        let err = build_prior_book(&observations, &config).unwrap_err();
        assert!(matches!(err, Error::InvalidConfig { .. }));
    }

    #[test]
    fn validate_config_rejects_negative_default_source_weight() {
        let observations = vec![obs("c1", 0, "s", "a", Outcome::Unknown, None, 1.0, vec![])];
        let config = BuildConfig {
            default_source_weight: -1.0,
            ..Default::default()
        };
        let err = build_prior_book(&observations, &config).unwrap_err();
        assert!(matches!(err, Error::InvalidConfig { .. }));
    }

    #[test]
    fn build_config_fingerprint_changes_with_decay_and_source_config() {
        let default_fp = crate::query::build_config_fingerprint(&BuildConfig::default());
        let decay_fp = crate::query::build_config_fingerprint(&decay_config(10.0, 1_000_000));
        let source_fp = crate::query::build_config_fingerprint(&BuildConfig {
            source_weights: std::collections::BTreeMap::from([("human".to_string(), 0.5)]),
            ..Default::default()
        });
        assert_ne!(default_fp, decay_fp);
        assert_ne!(default_fp, source_fp);
        assert_ne!(decay_fp, source_fp);
    }

    fn ctx_obs(sequence_id: &str, step: u32, action: &str) -> Observation {
        obs(
            sequence_id,
            step,
            "s",
            action,
            Outcome::Success,
            None,
            1.0,
            vec![],
        )
    }

    #[test]
    fn context_tracker_order_zero_is_always_a_noop() {
        let mut tracker = SequenceContextTracker::new(0);
        assert_eq!(
            tracker.advance(&ctx_obs("g1", 0, "a")).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            tracker.advance(&ctx_obs("g1", 1, "b")).unwrap(),
            Vec::<String>::new()
        );
        // Even a non-monotonic step never errors when order is 0.
        assert_eq!(
            tracker.advance(&ctx_obs("g1", 0, "c")).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn context_tracker_builds_up_window_within_a_sequence() {
        let mut tracker = SequenceContextTracker::new(3);
        assert_eq!(
            tracker.advance(&ctx_obs("g1", 0, "a")).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(tracker.advance(&ctx_obs("g1", 1, "b")).unwrap(), vec!["a"]);
        assert_eq!(
            tracker.advance(&ctx_obs("g1", 2, "c")).unwrap(),
            vec!["a", "b"]
        );
        assert_eq!(
            tracker.advance(&ctx_obs("g1", 3, "d")).unwrap(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn context_tracker_window_is_bounded_to_order() {
        let mut tracker = SequenceContextTracker::new(2);
        tracker.advance(&ctx_obs("g1", 0, "a")).unwrap();
        tracker.advance(&ctx_obs("g1", 1, "b")).unwrap();
        // Window is capped at 2: "a" has already fallen off by now.
        assert_eq!(
            tracker.advance(&ctx_obs("g1", 2, "c")).unwrap(),
            vec!["a", "b"]
        );
        assert_eq!(
            tracker.advance(&ctx_obs("g1", 3, "d")).unwrap(),
            vec!["b", "c"]
        );
    }

    #[test]
    fn context_tracker_errors_on_non_monotonic_step_within_same_sequence() {
        let mut tracker = SequenceContextTracker::new(2);
        tracker.advance(&ctx_obs("g1", 5, "a")).unwrap();
        let err = tracker.advance(&ctx_obs("g1", 5, "b")).unwrap_err();
        assert!(matches!(
            err,
            Error::SequenceNotSorted {
                step: 5,
                last_step: 5,
                ..
            }
        ));
        // A step going backward is caught the same way.
        let mut tracker = SequenceContextTracker::new(2);
        tracker.advance(&ctx_obs("g1", 5, "a")).unwrap();
        let err = tracker.advance(&ctx_obs("g1", 3, "b")).unwrap_err();
        assert!(matches!(err, Error::SequenceNotSorted { .. }));
    }

    #[test]
    fn context_tracker_resets_window_on_sequence_change() {
        let mut tracker = SequenceContextTracker::new(2);
        tracker.advance(&ctx_obs("g1", 0, "a")).unwrap();
        tracker.advance(&ctx_obs("g1", 1, "b")).unwrap();
        // New sequence_id: unconditional reset, no leftover "a"/"b".
        assert_eq!(
            tracker.advance(&ctx_obs("g2", 0, "x")).unwrap(),
            Vec::<String>::new()
        );
    }

    /// The proof for the sortedness design: an "A, B, A-resumed" interleave
    /// must degrade to a safe reset, never to a corrupted window that
    /// silently mixes two sequences' actions together.
    #[test]
    fn context_tracker_a_b_a_resumed_degrades_safely_not_corrupted() {
        let mut tracker = SequenceContextTracker::new(2);
        tracker.advance(&ctx_obs("A", 0, "a0")).unwrap();
        tracker.advance(&ctx_obs("A", 1, "a1")).unwrap();
        tracker.advance(&ctx_obs("B", 0, "b0")).unwrap();
        // "A" resumes with a step number that would have been valid forward
        // progress for A's own numbering -- must not be treated as if A's
        // window survived the B interruption.
        let window = tracker.advance(&ctx_obs("A", 2, "a2")).unwrap();
        assert_eq!(
            window,
            Vec::<String>::new(),
            "resumed A must start from an empty window, not a1/a0"
        );
        // Subsequent A steps build up A's own fresh window, uncontaminated by B.
        let window = tracker.advance(&ctx_obs("A", 3, "a3")).unwrap();
        assert_eq!(window, vec!["a2"]);
    }

    #[test]
    fn context_order_learns_per_context_priors_distinct_from_order_zero() {
        // Two repeated (context, state) patterns: "x" is always followed by
        // "A" from state "s", "y" always by "B" -- order-0 alone can't tell
        // them apart (both actions are equally common from "s"), but order-1
        // context can.
        let observations = vec![
            obs("g1", 0, "s0", "x", Outcome::Success, None, 1.0, vec![]),
            obs("g1", 1, "s", "A", Outcome::Success, None, 1.0, vec![]),
            obs("g2", 0, "s0", "y", Outcome::Success, None, 1.0, vec![]),
            obs("g2", 1, "s", "B", Outcome::Success, None, 1.0, vec![]),
            obs("g3", 0, "s0", "x", Outcome::Success, None, 1.0, vec![]),
            obs("g3", 1, "s", "A", Outcome::Success, None, 1.0, vec![]),
            obs("g4", 0, "s0", "y", Outcome::Success, None, 1.0, vec![]),
            obs("g4", 1, "s", "B", Outcome::Success, None, 1.0, vec![]),
        ];
        let config = BuildConfig {
            context_order: 1,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();

        // Order-0: both A and B tied at state "s".
        let order0 = book.query("s", None);
        assert_eq!(order0.len(), 2);

        // After "x", context-aware query narrows to just "A".
        let after_x = book.query_with_context("s", &["x".to_string()], None);
        assert_eq!(after_x.matched_order, 1);
        assert_eq!(after_x.candidates.len(), 1);
        assert_eq!(after_x.candidates[0].action, "A");

        // After "y", just "B".
        let after_y = book.query_with_context("s", &["y".to_string()], None);
        assert_eq!(after_y.matched_order, 1);
        assert_eq!(after_y.candidates[0].action, "B");

        // A never-seen context backs off to order-0 (both A and B).
        let after_unseen = book.query_with_context("s", &["z".to_string()], None);
        assert_eq!(after_unseen.matched_order, 0);
        assert_eq!(after_unseen.candidates.len(), 2);
    }

    #[test]
    fn context_order_shorter_than_a_sequence_degrades_without_padding() {
        // context_order=3, but every sequence is only 2 steps long -- no
        // order-2 or order-3 entries should ever be learned, only order-1.
        let observations = vec![
            obs("g1", 0, "s0", "x", Outcome::Success, None, 1.0, vec![]),
            obs("g1", 1, "s", "A", Outcome::Success, None, 1.0, vec![]),
            obs("g2", 0, "s0", "x", Outcome::Success, None, 1.0, vec![]),
            obs("g2", 1, "s", "A", Outcome::Success, None, 1.0, vec![]),
        ];
        let config = BuildConfig {
            context_order: 3,
            ..Default::default()
        };
        let book = build_prior_book(&observations, &config).unwrap();

        assert!(
            book.context_entries
                .keys()
                .all(|(context, _)| context.len() == 1),
            "no context longer than 1 should exist: {:?}",
            book.context_entries.keys().collect::<Vec<_>>()
        );
        let matched = book.query_with_context("s", &["x".to_string()], None);
        assert_eq!(matched.matched_order, 1);
    }

    #[test]
    fn context_order_never_changes_order_zero_output() {
        // Order-0 entries -- and the global smoothing target behind them --
        // must be byte-identical regardless of context_order: the global
        // target is counted exactly once per observation (in the order-0
        // pass), never once per context level too.
        let observations = vec![
            obs("g1", 0, "s0", "x", Outcome::Success, Some(0.9), 1.0, vec![]),
            obs("g1", 1, "s", "A", Outcome::Draw, Some(0.5), 1.0, vec![]),
            obs("g2", 0, "s0", "y", Outcome::Failure, Some(0.1), 1.0, vec![]),
            obs("g2", 1, "s", "B", Outcome::Success, Some(0.8), 1.0, vec![]),
            obs("g3", 0, "s0", "x", Outcome::Success, None, 1.0, vec![]),
            obs("g3", 1, "s", "A", Outcome::Success, None, 1.0, vec![]),
        ];
        let book_order0 = build_prior_book(&observations, &BuildConfig::default()).unwrap();
        let book_order2 = build_prior_book(
            &observations,
            &BuildConfig {
                context_order: 2,
                ..Default::default()
            },
        )
        .unwrap();

        // Compare via entries_sorted(), not the raw `entries` map: the Vec
        // stored per state has no guaranteed internal order (only
        // entries_sorted()/query() sort it), so a raw comparison can spuriously
        // fail on ordering alone even when the logical content is identical.
        assert_eq!(book_order0.entries_sorted(), book_order2.entries_sorted());
        assert!(!book_order2.context_entries.is_empty());
    }

    #[test]
    fn sequence_not_sorted_is_a_hard_error_from_the_eager_build_path() {
        let observations = vec![
            obs("g1", 0, "s", "a", Outcome::Success, None, 1.0, vec![]),
            obs("g1", 0, "s", "b", Outcome::Success, None, 1.0, vec![]), // step doesn't increase
        ];
        let config = BuildConfig {
            context_order: 1,
            ..Default::default()
        };
        let err = build_prior_book(&observations, &config).unwrap_err();
        assert!(matches!(err, Error::SequenceNotSorted { .. }));
    }
}
