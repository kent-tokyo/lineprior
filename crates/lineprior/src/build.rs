use crate::error::{Error, Result};
use crate::model::{BuildConfig, Observation, Outcome, PriorAction, PriorBook};
use crate::score::{confidence, normalize, ratio, raw_score, shrink_toward};
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::HashMap;

/// Running totals for one `(state, action)` pair.
#[derive(Debug, Default, Clone)]
struct ActionStats {
    count: u64,
    weighted_count: f64,
    /// Weighted successes / weighted trials, where a "trial" is any
    /// observation with a decisive (non-unknown) outcome.
    weighted_successes: f64,
    weighted_trials: f64,
    weighted_score_sum: f64,
    weighted_score_count: f64,
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
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct BuildStats {
    /// Observations folded into the accumulator (survived `max_step` /
    /// `tag_filter`).
    pub observations_kept: u64,
    /// Observations dropped by `max_step` or `tag_filter` before ever
    /// contributing to any `(state, action)` pair's statistics.
    pub observations_dropped_by_step_or_tag_filter: u64,
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
    global_weighted_successes: f64,
    global_weighted_trials: f64,
    global_weighted_score_sum: f64,
    global_weighted_score_count: f64,
    observations_kept: u64,
    observations_dropped_by_step_or_tag_filter: u64,
    config: &'a BuildConfig,
}

impl<'a> PriorAccumulator<'a> {
    pub(crate) fn new(config: &'a BuildConfig) -> Self {
        Self {
            stats: HashMap::new(),
            global_weighted_successes: 0.0,
            global_weighted_trials: 0.0,
            global_weighted_score_sum: 0.0,
            global_weighted_score_count: 0.0,
            observations_kept: 0,
            observations_dropped_by_step_or_tag_filter: 0,
            config,
        }
    }

    pub(crate) fn observe(&mut self, obs: &Observation) {
        if let Some(max_step) = self.config.max_step
            && obs.step > max_step
        {
            self.observations_dropped_by_step_or_tag_filter += 1;
            return;
        }
        if let Some(required_tags) = &self.config.tag_filter
            && !required_tags.iter().any(|tag| obs.tags.contains(tag))
        {
            self.observations_dropped_by_step_or_tag_filter += 1;
            return;
        }
        self.observations_kept += 1;

        let entry = self
            .stats
            .entry(obs.state.clone())
            .or_default()
            .entry(obs.action.clone())
            .or_default();
        entry.count += 1;
        entry.weighted_count += obs.weight;

        if obs.outcome != Outcome::Unknown {
            entry.weighted_trials += obs.weight;
            self.global_weighted_trials += obs.weight;
            // A draw earns partial credit rather than scoring like a loss;
            // both the per-action and dataset-wide totals must move
            // together since the latter is the smoothing target the
            // former shrinks toward.
            let success_credit = match obs.outcome {
                Outcome::Success => 1.0,
                Outcome::Draw => self.config.draw_value,
                Outcome::Failure | Outcome::Unknown => 0.0,
            };
            entry.weighted_successes += success_credit * obs.weight;
            self.global_weighted_successes += success_credit * obs.weight;
        }

        if let Some(score) = obs.score {
            entry.weighted_score_sum += obs.weight * score;
            entry.weighted_score_count += obs.weight;
            self.global_weighted_score_sum += obs.weight * score;
            self.global_weighted_score_count += obs.weight;
        }
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
        // signal.
        let global_success_rate =
            ratio(self.global_weighted_successes, self.global_weighted_trials);
        let global_mean_score = ratio(
            self.global_weighted_score_sum,
            self.global_weighted_score_count,
        );

        let mut entries: HashMap<String, Vec<PriorAction>> = HashMap::new();
        let mut candidates_before_filtering: u64 = 0;
        let mut candidates_dropped_by_min_count: u64 = 0;
        let mut candidates_dropped_by_min_weighted_count: u64 = 0;
        let mut candidates_dropped_by_min_confidence: u64 = 0;
        let mut candidates_dropped_by_max_actions_per_state: u64 = 0;

        for (state, actions) in self.stats {
            candidates_before_filtering += actions.len() as u64;

            // Explicit classify-loop rather than three chained `.filter()`
            // calls: behaviorally identical (a chained filter already
            // short-circuits per item at the first failing predicate), but
            // this lets each dropped candidate be counted against the
            // specific threshold that rejected it.
            let mut kept: Vec<(String, ActionStats)> = Vec::new();
            for (action, stat) in actions {
                if stat.count < self.config.min_count {
                    candidates_dropped_by_min_count += 1;
                    continue;
                }
                if stat.weighted_count < self.config.min_weighted_count {
                    candidates_dropped_by_min_weighted_count += 1;
                    continue;
                }
                if confidence(stat.weighted_count, self.config.confidence_k)
                    < self.config.min_confidence
                {
                    candidates_dropped_by_min_confidence += 1;
                    continue;
                }
                kept.push((action, stat));
            }
            if kept.is_empty() {
                continue;
            }

            let raw_scores: Vec<f64> = kept
                .iter()
                .map(|(_, stat)| {
                    let smoothed_success = global_success_rate.map(|global| {
                        shrink_toward(
                            stat.weighted_successes,
                            stat.weighted_trials,
                            self.config.smoothing_alpha,
                            global,
                        )
                    });
                    let smoothed_score = global_mean_score.map(|global| {
                        shrink_toward(
                            stat.weighted_score_sum,
                            stat.weighted_score_count,
                            self.config.smoothing_alpha,
                            global,
                        )
                    });
                    raw_score(
                        stat.weighted_count,
                        smoothed_success,
                        smoothed_score,
                        self.config,
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
                    confidence: confidence(stat.weighted_count, self.config.confidence_k),
                })
                .collect();

            if let Some(max_actions) = self.config.max_actions_per_state {
                actions_out.sort_by(|a, b| {
                    b.prior
                        .partial_cmp(&a.prior)
                        .unwrap_or(Ordering::Equal)
                        .then_with(|| a.action.cmp(&b.action))
                });
                if actions_out.len() > max_actions {
                    candidates_dropped_by_max_actions_per_state +=
                        (actions_out.len() - max_actions) as u64;
                }
                actions_out.truncate(max_actions);
            }

            entries.insert(state, actions_out);
        }

        let candidates_kept: u64 = entries.values().map(|v| v.len() as u64).sum();
        let stats = BuildStats {
            observations_kept: self.observations_kept,
            observations_dropped_by_step_or_tag_filter: self
                .observations_dropped_by_step_or_tag_filter,
            candidates_before_filtering,
            candidates_dropped_by_min_count,
            candidates_dropped_by_min_weighted_count,
            candidates_dropped_by_min_confidence,
            candidates_dropped_by_max_actions_per_state,
            candidates_kept,
        };

        (PriorBook { entries }, stats)
    }
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

    let mut acc = PriorAccumulator::new(config);
    for obs in observations {
        acc.observe(obs);
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

        let mut acc = PriorAccumulator::new(&config);
        for o in &observations {
            acc.observe(o);
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
}
