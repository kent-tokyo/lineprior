use crate::eval::EvalReport;
use crate::model::{BuildConfig, ConfidenceMode};
use serde::Serialize;

/// One `--param key=v1,v2,...` sweep. Each variant already carries its
/// values pre-parsed to the right type, so applying it to a [`BuildConfig`]
/// can never mismatch a key against the wrong value type -- unlike a
/// generic `(key, values: Vec<AnyValue>)` pair, this is checked by the
/// compiler, not at grid-expansion time.
#[derive(Debug, Clone)]
pub enum TuneParam {
    ConfidenceMode(Vec<ConfidenceMode>),
    MinConfidence(Vec<f64>),
    SmoothingAlpha(Vec<f64>),
    ConfidenceK(Vec<f64>),
    ConfidenceZ(Vec<f64>),
    MinCount(Vec<u64>),
    MinWeightedCount(Vec<f64>),
    DrawValue(Vec<f64>),
    TimeDecayHalfLifeDays(Vec<Option<f64>>),
    DefaultSourceWeight(Vec<f64>),
}

fn expand_one(configs: &[BuildConfig], param: &TuneParam) -> Vec<BuildConfig> {
    match param {
        TuneParam::ConfidenceMode(values) => configs
            .iter()
            .flat_map(|c| {
                values.iter().map(move |v| BuildConfig {
                    confidence_mode: *v,
                    ..c.clone()
                })
            })
            .collect(),
        TuneParam::MinConfidence(values) => configs
            .iter()
            .flat_map(|c| {
                values.iter().map(move |v| BuildConfig {
                    min_confidence: *v,
                    ..c.clone()
                })
            })
            .collect(),
        TuneParam::SmoothingAlpha(values) => configs
            .iter()
            .flat_map(|c| {
                values.iter().map(move |v| BuildConfig {
                    smoothing_alpha: *v,
                    ..c.clone()
                })
            })
            .collect(),
        TuneParam::ConfidenceK(values) => configs
            .iter()
            .flat_map(|c| {
                values.iter().map(move |v| BuildConfig {
                    confidence_k: *v,
                    ..c.clone()
                })
            })
            .collect(),
        TuneParam::ConfidenceZ(values) => configs
            .iter()
            .flat_map(|c| {
                values.iter().map(move |v| BuildConfig {
                    confidence_z: *v,
                    ..c.clone()
                })
            })
            .collect(),
        TuneParam::MinCount(values) => configs
            .iter()
            .flat_map(|c| {
                values.iter().map(move |v| BuildConfig {
                    min_count: *v,
                    ..c.clone()
                })
            })
            .collect(),
        TuneParam::MinWeightedCount(values) => configs
            .iter()
            .flat_map(|c| {
                values.iter().map(move |v| BuildConfig {
                    min_weighted_count: *v,
                    ..c.clone()
                })
            })
            .collect(),
        TuneParam::DrawValue(values) => configs
            .iter()
            .flat_map(|c| {
                values.iter().map(move |v| BuildConfig {
                    draw_value: *v,
                    ..c.clone()
                })
            })
            .collect(),
        TuneParam::TimeDecayHalfLifeDays(values) => configs
            .iter()
            .flat_map(|c| {
                values.iter().map(move |v| BuildConfig {
                    time_decay_half_life_days: *v,
                    ..c.clone()
                })
            })
            .collect(),
        TuneParam::DefaultSourceWeight(values) => configs
            .iter()
            .flat_map(|c| {
                values.iter().map(move |v| BuildConfig {
                    default_source_weight: *v,
                    ..c.clone()
                })
            })
            .collect(),
    }
}

/// Deterministic Cartesian product over `params`, in the given param and
/// value order, applied on top of `base`. `config_id`s are `"cfg_001"`,
/// `"cfg_002"`, ... in generation order. No params -> a single candidate
/// (`base` itself, as `"cfg_001"`).
pub fn expand_grid(base: &BuildConfig, params: &[TuneParam]) -> Vec<(String, BuildConfig)> {
    let mut configs = vec![base.clone()];
    for param in params {
        configs = expand_one(&configs, param);
    }
    configs
        .into_iter()
        .enumerate()
        .map(|(i, config)| (format!("cfg_{:03}", i + 1), config))
        .collect()
}

/// Which held-out metric `tune` ranks candidates by. See each variant's
/// doc comment; `CoveredMrr` is the recommended default -- optimizing `Mrr`
/// alone tends to pick configs that abstain (report no candidate) except
/// when very confident, while optimizing coverage alone tolerates a sloppy
/// prior. `CoveredMrr` penalizes both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TuneObjective {
    /// `mean_reciprocal_rank`, among evaluated (covered) test observations only.
    Mrr,
    /// `top1_hit_rate`, among evaluated (covered) test observations only.
    Top1,
    /// `covered_fraction * mean_reciprocal_rank` -- MRR averaged across
    /// *all* test observations, treating an uncovered one as contributing 0.
    CoveredMrr,
    /// Identical to `Top1`; choosing this objective requires
    /// `TuneConstraints::min_covered_fraction` to also be set, so the
    /// coverage floor lives in one place (constraints) instead of being
    /// duplicated inside the objective itself.
    Top1AtMinCoverage,
}

/// Candidates failing a constraint stay in [`TuneOutput::all_results`] (so
/// the caller can see why they were excluded) but are never [`select_best`]-
/// eligible. `None` on any field means "no floor/ceiling on this metric."
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct TuneConstraints {
    pub min_covered_fraction: Option<f64>,
    pub max_fallback_rate: Option<f64>,
    pub min_top1_hit_rate: Option<f64>,
}

/// Observation-weighted fraction of test observations whose state had at
/// least one candidate. `1.0 - fallback_rate`, not [`EvalReport::coverage`]
/// (that field is state-weighted and deliberately doesn't complement to 1
/// with `fallback_rate` -- see its doc comment). Defaults to `0.0` (no
/// coverage) on the degenerate `fallback_rate: None` case (zero test
/// observations), matching the "can't confirm coverage" reading.
pub fn covered_fraction(report: &EvalReport) -> f64 {
    1.0 - report.fallback_rate.unwrap_or(1.0)
}

/// The value `tune` ranks a candidate by. A metric field of `None` (valid
/// config, but its book covers nothing to measure -- e.g. `min_confidence`
/// filtered every candidate away) scores `0.0` here rather than being
/// treated as an error; only a config `evaluate()` itself rejects (an
/// `Err`) is excluded before this is ever called (see [`crate::tune`]'s
/// module doc).
pub fn objective_value(objective: TuneObjective, report: &EvalReport) -> f64 {
    match objective {
        TuneObjective::Mrr => report.mean_reciprocal_rank.unwrap_or(0.0),
        TuneObjective::Top1 | TuneObjective::Top1AtMinCoverage => {
            report.top1_hit_rate.unwrap_or(0.0)
        }
        TuneObjective::CoveredMrr => {
            covered_fraction(report) * report.mean_reciprocal_rank.unwrap_or(0.0)
        }
    }
}

/// Whether `report` clears every floor/ceiling set in `constraints`. A
/// missing metric (`None`) is treated as failing a floor and failing a
/// ceiling check is skipped only when the ceiling itself is unset --
/// i.e. "no data" never counts as "constraint satisfied."
pub fn meets_constraints(report: &EvalReport, constraints: &TuneConstraints) -> bool {
    if let Some(min) = constraints.min_covered_fraction
        && covered_fraction(report) < min
    {
        return false;
    }
    if let Some(max) = constraints.max_fallback_rate
        && report.fallback_rate.unwrap_or(1.0) > max
    {
        return false;
    }
    if let Some(min) = constraints.min_top1_hit_rate
        && report.top1_hit_rate.unwrap_or(0.0) < min
    {
        return false;
    }
    true
}

/// The subset of [`EvalReport`] that matters for comparing candidates.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TuneMetrics {
    pub top1_hit_rate: Option<f64>,
    pub mean_reciprocal_rank: Option<f64>,
    pub covered_fraction: f64,
    pub fallback_rate: Option<f64>,
}

/// One evaluated grid candidate's outcome. Note the field is
/// `objective_value` (a number), not `objective` -- that name is reserved
/// at the [`TuneOutput`] level for which [`TuneObjective`] was chosen.
#[derive(Debug, Clone, Serialize)]
pub struct TuneCandidateResult {
    pub config_id: String,
    pub build_config: BuildConfig,
    pub metrics: TuneMetrics,
    pub objective_value: f64,
    pub meets_constraints: bool,
}

/// Builds one [`TuneCandidateResult`] from an already-evaluated candidate.
/// Does not call `evaluate()` itself -- callers (e.g. the CLI's grid loop)
/// own opening/parsing the input and only reach this on `Ok`.
pub fn build_candidate_result(
    config_id: String,
    build_config: BuildConfig,
    report: &EvalReport,
    objective: TuneObjective,
    constraints: &TuneConstraints,
) -> TuneCandidateResult {
    TuneCandidateResult {
        config_id,
        objective_value: objective_value(objective, report),
        meets_constraints: meets_constraints(report, constraints),
        metrics: TuneMetrics {
            top1_hit_rate: report.top1_hit_rate,
            mean_reciprocal_rank: report.mean_reciprocal_rank,
            covered_fraction: covered_fraction(report),
            fallback_rate: report.fallback_rate,
        },
        build_config,
    }
}

/// One point on the coverage/MRR tradeoff curve.
#[derive(Debug, Clone, Serialize)]
pub struct ParetoEntry {
    pub config_id: String,
    pub mrr: f64,
    pub covered_fraction: f64,
}

/// Highest `objective_value` among `meets_constraints == true` entries in
/// `results`, ties broken by earliest `config_id` (grid generation order).
/// `None` if `results` is empty or nothing satisfies its constraints.
pub fn select_best(results: &[TuneCandidateResult]) -> Option<TuneCandidateResult> {
    results
        .iter()
        .filter(|r| r.meets_constraints)
        .max_by(|a, b| {
            // Reversed config_id comparison: on an objective_value tie, the
            // *earlier* (lexicographically smaller) config_id should win,
            // and `Iterator::max_by` returns the *last* maximal element --
            // so "earlier id sorts as greater" makes it the one returned.
            a.objective_value
                .partial_cmp(&b.objective_value)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.config_id.cmp(&a.config_id))
        })
        .cloned()
}

/// Non-dominated set over `(mrr, covered_fraction)`, both maximized: a
/// candidate is excluded if another candidate is at least as good on both
/// dimensions and strictly better on at least one. Unlike [`select_best`],
/// this ignores `meets_constraints` -- it shows the whole tradeoff space so
/// a caller can override "best" with their own pick. O(n^2); fine at the
/// grid sizes `tune` targets (tens to low hundreds of candidates).
pub fn pareto_front(results: &[TuneCandidateResult]) -> Vec<ParetoEntry> {
    let entries: Vec<ParetoEntry> = results
        .iter()
        .map(|r| ParetoEntry {
            config_id: r.config_id.clone(),
            mrr: r.metrics.mean_reciprocal_rank.unwrap_or(0.0),
            covered_fraction: r.metrics.covered_fraction,
        })
        .collect();

    fn dominates(a: &ParetoEntry, b: &ParetoEntry) -> bool {
        a.mrr >= b.mrr
            && a.covered_fraction >= b.covered_fraction
            && (a.mrr > b.mrr || a.covered_fraction > b.covered_fraction)
    }

    let mut front: Vec<ParetoEntry> = entries
        .iter()
        .filter(|candidate| !entries.iter().any(|other| dominates(other, candidate)))
        .cloned()
        .collect();
    front.sort_by(|a, b| a.config_id.cmp(&b.config_id));
    front
}

/// Full result of a `tune` run.
#[derive(Debug, Clone, Serialize)]
pub struct TuneOutput {
    pub best: Option<TuneCandidateResult>,
    pub all_results: Vec<TuneCandidateResult>,
    pub pareto_front: Vec<ParetoEntry>,
    pub objective: TuneObjective,
    pub constraints: TuneConstraints,
    pub evaluated_config_count: usize,
    pub skipped_config_count: usize,
    pub warnings: Vec<String>,
}

impl TuneOutput {
    /// Assembles the final report from every successfully-evaluated
    /// candidate: computes `best`/`pareto_front`, and appends a warning if
    /// no candidate satisfied `constraints`. `skipped_config_count` and any
    /// per-candidate error messages are the caller's own bookkeeping (the
    /// CLI's grid loop pushes one warning string per `evaluate()` `Err`).
    pub fn from_results(
        all_results: Vec<TuneCandidateResult>,
        objective: TuneObjective,
        constraints: TuneConstraints,
        skipped_config_count: usize,
        mut warnings: Vec<String>,
    ) -> TuneOutput {
        let best = select_best(&all_results);
        let pareto_front = pareto_front(&all_results);
        if best.is_none() {
            warnings.push("no candidate configuration satisfied the given constraints".to_string());
        }
        TuneOutput {
            evaluated_config_count: all_results.len(),
            best,
            all_results,
            pareto_front,
            objective,
            constraints,
            skipped_config_count,
            warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(
        top1_hit_rate: Option<f64>,
        mean_reciprocal_rank: Option<f64>,
        fallback_rate: Option<f64>,
    ) -> EvalReport {
        EvalReport {
            num_train_observations: 10,
            num_test_observations: 10,
            num_test_states: 1,
            num_evaluated_observations: 10,
            num_fallback_observations: 0,
            num_test_states_with_candidates: 1,
            coverage: Some(1.0),
            fallback_rate,
            top1_hit_rate,
            topk_hit_rate: Vec::new(),
            mean_reciprocal_rank,
            avg_rank_when_found: None,
            avg_confidence_on_hit: None,
            avg_confidence_on_miss: None,
            score_lift: None,
            confidence_calibration: Vec::new(),
            threshold_sweep: Vec::new(),
        }
    }

    fn candidate(id: &str, objective_value: f64, meets_constraints: bool) -> TuneCandidateResult {
        TuneCandidateResult {
            config_id: id.to_string(),
            build_config: BuildConfig::default(),
            metrics: TuneMetrics {
                top1_hit_rate: Some(objective_value),
                mean_reciprocal_rank: Some(objective_value),
                covered_fraction: objective_value,
                fallback_rate: Some(1.0 - objective_value),
            },
            objective_value,
            meets_constraints,
        }
    }

    #[test]
    fn expand_grid_with_no_params_yields_one_candidate() {
        let grid = expand_grid(&BuildConfig::default(), &[]);
        assert_eq!(grid.len(), 1);
        assert_eq!(grid[0].0, "cfg_001");
    }

    #[test]
    fn expand_grid_is_a_deterministic_cartesian_product_in_generation_order() {
        let params = vec![
            TuneParam::MinCount(vec![1, 2]),
            TuneParam::SmoothingAlpha(vec![1.0, 5.0, 10.0]),
        ];
        let grid = expand_grid(&BuildConfig::default(), &params);
        assert_eq!(grid.len(), 6);
        let ids: Vec<&str> = grid.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "cfg_001", "cfg_002", "cfg_003", "cfg_004", "cfg_005", "cfg_006"
            ]
        );
        // First param varies slower: (1,1.0) (1,5.0) (1,10.0) (2,1.0) (2,5.0) (2,10.0).
        let combos: Vec<(u64, f64)> = grid
            .iter()
            .map(|(_, c)| (c.min_count, c.smoothing_alpha))
            .collect();
        assert_eq!(
            combos,
            vec![(1, 1.0), (1, 5.0), (1, 10.0), (2, 1.0), (2, 5.0), (2, 10.0),]
        );
        // Running it again produces byte-identical config_ids and values.
        assert_eq!(expand_grid(&BuildConfig::default(), &params).len(), 6);
    }

    #[test]
    fn expand_grid_applies_confidence_mode_and_optional_time_decay() {
        let params = vec![
            TuneParam::ConfidenceMode(vec![ConfidenceMode::Hybrid]),
            TuneParam::TimeDecayHalfLifeDays(vec![None, Some(30.0)]),
        ];
        let grid = expand_grid(&BuildConfig::default(), &params);
        assert_eq!(grid.len(), 2);
        assert_eq!(grid[0].1.confidence_mode, ConfidenceMode::Hybrid);
        assert_eq!(grid[0].1.time_decay_half_life_days, None);
        assert_eq!(grid[1].1.time_decay_half_life_days, Some(30.0));
    }

    #[test]
    fn objective_value_treats_missing_metric_as_zero_not_a_panic() {
        let empty = report(None, None, None);
        assert_eq!(objective_value(TuneObjective::Mrr, &empty), 0.0);
        assert_eq!(objective_value(TuneObjective::Top1, &empty), 0.0);
        assert_eq!(objective_value(TuneObjective::CoveredMrr, &empty), 0.0);
    }

    #[test]
    fn covered_mrr_multiplies_coverage_and_mrr() {
        let r = report(Some(0.5), Some(0.6), Some(0.3)); // covered_fraction = 0.7
        let got = objective_value(TuneObjective::CoveredMrr, &r);
        assert!((got - 0.7 * 0.6).abs() < 1e-9);
    }

    #[test]
    fn meets_constraints_rejects_below_floor_and_above_ceiling() {
        let r = report(Some(0.5), Some(0.5), Some(0.5)); // covered_fraction = 0.5
        assert!(meets_constraints(&r, &TuneConstraints::default()));
        assert!(!meets_constraints(
            &r,
            &TuneConstraints {
                min_covered_fraction: Some(0.6),
                ..Default::default()
            }
        ));
        assert!(!meets_constraints(
            &r,
            &TuneConstraints {
                max_fallback_rate: Some(0.4),
                ..Default::default()
            }
        ));
        assert!(!meets_constraints(
            &r,
            &TuneConstraints {
                min_top1_hit_rate: Some(0.6),
                ..Default::default()
            }
        ));
    }

    #[test]
    fn select_best_picks_highest_objective_value_among_eligible() {
        let results = vec![
            candidate("cfg_001", 0.3, true),
            candidate("cfg_002", 0.9, false), // higher value, but disqualified
            candidate("cfg_003", 0.6, true),
        ];
        let best = select_best(&results).unwrap();
        assert_eq!(best.config_id, "cfg_003");
    }

    #[test]
    fn select_best_breaks_ties_by_earliest_config_id() {
        let results = vec![
            candidate("cfg_002", 0.5, true),
            candidate("cfg_001", 0.5, true),
        ];
        assert_eq!(select_best(&results).unwrap().config_id, "cfg_001");
    }

    #[test]
    fn select_best_is_none_when_nothing_meets_constraints() {
        let results = vec![candidate("cfg_001", 0.9, false)];
        assert!(select_best(&results).is_none());
    }

    #[test]
    fn pareto_front_excludes_dominated_points_and_is_sorted_by_config_id() {
        let results = vec![
            candidate("cfg_003", 0.0, true), // mrr=covered=0.0 -- irrelevant, overridden below
        ];
        let mut results = results;
        results[0].metrics = TuneMetrics {
            top1_hit_rate: None,
            mean_reciprocal_rank: Some(0.5),
            covered_fraction: 0.5,
            fallback_rate: Some(0.5),
        };
        results.push(TuneCandidateResult {
            config_id: "cfg_001".to_string(),
            build_config: BuildConfig::default(),
            metrics: TuneMetrics {
                top1_hit_rate: None,
                mean_reciprocal_rank: Some(0.6),
                covered_fraction: 0.7,
                fallback_rate: Some(0.3),
            },
            objective_value: 0.0,
            meets_constraints: true,
        }); // dominates cfg_003 on both dimensions
        results.push(TuneCandidateResult {
            config_id: "cfg_002".to_string(),
            build_config: BuildConfig::default(),
            metrics: TuneMetrics {
                top1_hit_rate: None,
                mean_reciprocal_rank: Some(0.9),
                covered_fraction: 0.2,
                fallback_rate: Some(0.8),
            },
            objective_value: 0.0,
            meets_constraints: true,
        }); // higher mrr, lower coverage -- non-dominated tradeoff point

        let front = pareto_front(&results);
        let ids: Vec<&str> = front.iter().map(|e| e.config_id.as_str()).collect();
        assert_eq!(ids, vec!["cfg_001", "cfg_002"]); // cfg_003 dominated by cfg_001
    }

    #[test]
    fn tune_output_from_results_warns_when_best_is_none() {
        let results = vec![candidate("cfg_001", 0.9, false)];
        let output = TuneOutput::from_results(
            results,
            TuneObjective::Mrr,
            TuneConstraints::default(),
            0,
            Vec::new(),
        );
        assert!(output.best.is_none());
        assert!(
            output
                .warnings
                .iter()
                .any(|w| w.contains("no candidate configuration satisfied"))
        );
        assert_eq!(output.evaluated_config_count, 1);
    }
}
