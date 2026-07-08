use crate::build::PriorAccumulator;
use crate::error::{Result, Warning};
use crate::input::parse_line;
use crate::model::{BuildConfig, Observation, PriorBook};
use crate::score::ratio;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::model::{Outcome, PriorAction};

    fn obs(state: &str, action: &str) -> Observation {
        Observation {
            sequence_id: "seq".to_string(),
            step: 0,
            state: state.to_string(),
            action: action.to_string(),
            outcome: Outcome::Success,
            score: None,
            weight: 1.0,
            tags: Vec::new(),
        }
    }

    /// State "s" ranks a (highest prior) > b > c; state "t" is unused by
    /// the tests below except to confirm it doesn't interfere.
    fn sample_book() -> PriorBook {
        let mut entries = HashMap::new();
        entries.insert(
            "s".to_string(),
            vec![
                PriorAction {
                    action: "a".into(),
                    count: 10,
                    weighted_count: 10.0,
                    success_rate: Some(0.9),
                    mean_score: None,
                    prior: 0.6,
                    confidence: 0.5,
                },
                PriorAction {
                    action: "b".into(),
                    count: 5,
                    weighted_count: 5.0,
                    success_rate: Some(0.5),
                    mean_score: None,
                    prior: 0.3,
                    confidence: 0.3,
                },
                PriorAction {
                    action: "c".into(),
                    count: 2,
                    weighted_count: 2.0,
                    success_rate: Some(0.2),
                    mean_score: None,
                    prior: 0.1,
                    confidence: 0.1,
                },
            ],
        );
        PriorBook { entries }
    }

    #[test]
    fn is_train_is_stable_for_a_fixed_id() {
        assert_eq!(is_train("seq-1", 0.8), is_train("seq-1", 0.8));
    }

    #[test]
    fn is_train_ratio_zero_is_always_false() {
        for i in 0..50 {
            assert!(!is_train(&format!("seq-{i}"), 0.0));
        }
    }

    #[test]
    fn is_train_ratio_one_is_always_true() {
        for i in 0..50 {
            assert!(is_train(&format!("seq-{i}"), 1.0));
        }
    }

    #[test]
    fn is_train_splits_roughly_by_ratio() {
        // Statistical sanity check on a deterministic hash, not an exact
        // count -- generous band avoids flakiness while still catching a
        // badly broken hash/bucketing (e.g. always-train or always-test).
        let train_count = (0..1000)
            .filter(|i| is_train(&format!("seq-{i}"), 0.8))
            .count();
        assert!(
            (700..=900).contains(&train_count),
            "train_count = {train_count}, expected roughly 800/1000"
        );
    }

    #[test]
    fn topk_hit_rate_and_mrr_match_hand_computed_ranks() {
        let book = sample_book();
        let top_k = vec![1, 2, 3];
        let mut acc = EvalAccumulator::new(&top_k, 0, &[]);

        acc.observe(&book, &obs("s", "a")); // rank 1
        acc.observe(&book, &obs("s", "b")); // rank 2
        acc.observe(&book, &obs("s", "c")); // rank 3
        acc.observe(&book, &obs("s", "z")); // not found among candidates

        let report = acc.finish(0);
        assert_eq!(report.num_evaluated_observations, 4);
        assert_eq!(report.top1_hit_rate, Some(0.25));
        assert_eq!(
            report.topk_hit_rate,
            vec![
                TopKHitRate {
                    k: 1,
                    hit_rate: Some(0.25)
                },
                TopKHitRate {
                    k: 2,
                    hit_rate: Some(0.5)
                },
                TopKHitRate {
                    k: 3,
                    hit_rate: Some(0.75)
                },
            ]
        );
        let expected_mrr = (1.0 + 0.5 + 1.0 / 3.0) / 4.0;
        assert!((report.mean_reciprocal_rank.unwrap() - expected_mrr).abs() < 1e-9);
        assert_eq!(report.avg_rank_when_found, Some(2.0)); // (1 + 2 + 3) / 3
    }

    #[test]
    fn unseen_state_counts_as_fallback_not_evaluated() {
        let book = sample_book(); // only has state "s"
        let top_k = vec![1];
        let mut acc = EvalAccumulator::new(&top_k, 0, &[]);

        acc.observe(&book, &obs("unseen_state", "a"));

        let report = acc.finish(0);
        assert_eq!(report.num_test_states, 1);
        assert_eq!(report.num_fallback_observations, 1);
        assert_eq!(report.num_evaluated_observations, 0);
        assert_eq!(report.num_test_states_with_candidates, 0);
        assert_eq!(report.coverage, Some(0.0));
        assert_eq!(report.fallback_rate, Some(1.0));
        assert_eq!(report.top1_hit_rate, None);
    }

    #[test]
    fn evaluate_end_to_end_matches_hand_derived_expectations() {
        let train_ratio = 0.5;
        // Partition sequence ids using the same deterministic split
        // evaluate() itself uses, so expectations below are derived from
        // the actual split rather than guessed at.
        let candidate_ids: Vec<String> = (0..40).map(|i| format!("seq-{i}")).collect();
        let train_ids: Vec<&String> = candidate_ids
            .iter()
            .filter(|id| is_train(id, train_ratio))
            .collect();
        let test_ids: Vec<&String> = candidate_ids
            .iter()
            .filter(|id| !is_train(id, train_ratio))
            .collect();
        assert!(!train_ids.is_empty(), "need at least one train sequence");
        assert!(test_ids.len() >= 2, "need at least two test sequences");

        // Train: state "s" always leads to action "a".
        let mut jsonl = String::new();
        for id in &train_ids {
            jsonl.push_str(&format!(
                "{{\"sequence_id\":\"{id}\",\"step\":0,\"state\":\"s\",\"action\":\"a\",\"outcome\":\"success\"}}\n"
            ));
        }
        // Test: even-indexed sequences repeat "a" (should hit rank 1),
        // odd-indexed take a never-before-seen action "z" (miss, but still
        // evaluated since state "s" has candidates).
        for (i, id) in test_ids.iter().enumerate() {
            let action = if i % 2 == 0 { "a" } else { "z" };
            jsonl.push_str(&format!(
                "{{\"sequence_id\":\"{id}\",\"step\":0,\"state\":\"s\",\"action\":\"{action}\",\"outcome\":\"success\"}}\n"
            ));
        }

        let hits = test_ids
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == 0)
            .count();
        let expected_confidence = train_ids.len() as f64 / (train_ids.len() as f64 + 20.0);

        let eval_config = EvalConfig {
            train_ratio,
            top_k: vec![1],
            ..EvalConfig::default()
        };
        let output = evaluate(
            jsonl.as_bytes(),
            jsonl.as_bytes(),
            true,
            &BuildConfig::default(),
            &eval_config,
        )
        .unwrap();

        assert_eq!(output.report.num_train_observations, train_ids.len() as u64);
        assert_eq!(output.report.num_test_observations, test_ids.len() as u64);
        assert_eq!(output.report.num_test_states, 1);
        assert_eq!(
            output.report.num_evaluated_observations,
            test_ids.len() as u64
        );
        assert_eq!(output.report.num_fallback_observations, 0);
        assert_eq!(output.report.coverage, Some(1.0));
        assert_eq!(output.report.fallback_rate, Some(0.0));
        let expected_rate = Some(hits as f64 / test_ids.len() as f64);
        assert_eq!(output.report.top1_hit_rate, expected_rate);
        assert_eq!(
            output.report.topk_hit_rate,
            vec![TopKHitRate {
                k: 1,
                hit_rate: expected_rate
            }]
        );
        assert_eq!(output.report.mean_reciprocal_rank, expected_rate);
        assert_eq!(output.report.avg_rank_when_found, Some(1.0));
        assert_eq!(
            output.report.avg_confidence_on_hit,
            Some(expected_confidence)
        );
        assert_eq!(
            output.report.avg_confidence_on_miss,
            Some(expected_confidence)
        );
        assert_eq!(output.report.score_lift, None); // no `score` field anywhere
        assert!(output.warnings.is_empty());
    }

    /// Three single-candidate states so each observation's #1 confidence is
    /// fully controlled: "s1" 0.05 (bin 0 of 10), "s2" 0.55 (bin 5), "s3"
    /// 1.0 (edge case -- must clamp into the last bin, not overflow it).
    fn calibration_fixture_book() -> PriorBook {
        let action_with_confidence = |confidence: f64| PriorAction {
            action: "a".into(),
            count: 1,
            weighted_count: 1.0,
            success_rate: None,
            mean_score: None,
            prior: 1.0,
            confidence,
        };
        let mut entries = HashMap::new();
        entries.insert("s1".to_string(), vec![action_with_confidence(0.05)]);
        entries.insert("s2".to_string(), vec![action_with_confidence(0.55)]);
        entries.insert("s3".to_string(), vec![action_with_confidence(1.0)]);
        PriorBook { entries }
    }

    #[test]
    fn calibration_bins_are_deterministic_length_and_bucketed_correctly() {
        let book = calibration_fixture_book();
        let top_k = vec![1];
        let mut acc = EvalAccumulator::new(&top_k, 10, &[]);

        acc.observe(&book, &obs("s1", "a")); // hit, confidence 0.05 -> bin 0
        acc.observe(&book, &obs("s2", "b")); // miss, confidence 0.55 -> bin 5
        acc.observe(&book, &obs("s3", "a")); // hit, confidence 1.0 -> clamped into last bin 9

        let report = acc.finish(0);
        assert_eq!(report.confidence_calibration.len(), 10); // always calibration_bins entries

        let bin0 = &report.confidence_calibration[0];
        assert!((bin0.min_confidence - 0.0).abs() < 1e-9);
        assert!((bin0.max_confidence - 0.1).abs() < 1e-9);
        assert_eq!(bin0.num_evaluated, 1);
        assert_eq!(bin0.top1_hit_rate, Some(1.0));
        assert_eq!(bin0.mean_reciprocal_rank, Some(1.0));

        let bin5 = &report.confidence_calibration[5];
        assert_eq!(bin5.num_evaluated, 1);
        assert_eq!(bin5.top1_hit_rate, Some(0.0));
        assert_eq!(bin5.mean_reciprocal_rank, Some(0.0));

        let bin9 = &report.confidence_calibration[9]; // confidence == 1.0 lands here, not out of bounds
        assert_eq!(bin9.num_evaluated, 1);
        assert_eq!(bin9.top1_hit_rate, Some(1.0));

        let bin1 = &report.confidence_calibration[1];
        assert_eq!(bin1.num_evaluated, 0);
        assert_eq!(bin1.top1_hit_rate, None);
        assert_eq!(bin1.mean_reciprocal_rank, None);
    }

    #[test]
    fn threshold_sweep_matches_hand_computed_fixture() {
        let book = calibration_fixture_book();
        let top_k = vec![1];
        let thresholds = vec![0.1, 0.6];
        let mut acc = EvalAccumulator::new(&top_k, 0, &thresholds);

        acc.observe(&book, &obs("s1", "a")); // confidence 0.05: below both thresholds
        acc.observe(&book, &obs("s2", "b")); // confidence 0.55: covers 0.1 only, miss
        acc.observe(&book, &obs("s3", "a")); // confidence 1.0: covers both, hit

        let report = acc.finish(0);
        assert_eq!(report.threshold_sweep.len(), 2); // always thresholds.len() entries, in request order

        let at_0_1 = &report.threshold_sweep[0];
        assert_eq!(at_0_1.min_confidence, 0.1);
        assert!((at_0_1.covered_fraction - 2.0 / 3.0).abs() < 1e-9); // s2, s3
        assert!((at_0_1.abstained_fraction - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(at_0_1.top1_hit_rate, Some(0.5)); // 1 hit (s3) of 2 covered
        assert_eq!(at_0_1.mean_reciprocal_rank, Some(0.5)); // (0.0 + 1.0) / 2

        let at_0_6 = &report.threshold_sweep[1];
        assert_eq!(at_0_6.min_confidence, 0.6);
        assert!((at_0_6.covered_fraction - 1.0 / 3.0).abs() < 1e-9); // s3 only
        assert!((at_0_6.abstained_fraction - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(at_0_6.top1_hit_rate, Some(1.0));
        assert_eq!(at_0_6.mean_reciprocal_rank, Some(1.0));
    }

    #[test]
    fn calibration_and_threshold_sweep_are_empty_when_not_requested() {
        // Default EvalConfig (calibration_bins: None, thresholds: empty) --
        // backward compat for existing callers.
        let train = "{\"sequence_id\":\"x\",\"step\":0,\"state\":\"s\",\"action\":\"a\",\"outcome\":\"success\"}\n";
        let output = evaluate(
            train.as_bytes(),
            train.as_bytes(),
            true,
            &BuildConfig::default(),
            &EvalConfig::default(),
        )
        .unwrap();
        assert!(output.report.confidence_calibration.is_empty());
        assert!(output.report.threshold_sweep.is_empty());
    }

    #[test]
    fn strict_mode_aborts_on_invalid_record_in_train_pass() {
        let train = "{\"sequence_id\":\"x\",\"step\":0,\"state\":\"\",\"action\":\"a\"}\n";
        let err = evaluate(
            train.as_bytes(),
            "".as_bytes(),
            true,
            &BuildConfig::default(),
            &EvalConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::EmptyState { line: 1 }));
    }

    #[test]
    fn strict_mode_aborts_on_invalid_record_in_test_pass() {
        let train = "{\"sequence_id\":\"x\",\"step\":0,\"state\":\"s\",\"action\":\"a\"}\n";
        let test = "{\"sequence_id\":\"y\",\"step\":0,\"state\":\"\",\"action\":\"a\"}\n";
        let err = evaluate(
            train.as_bytes(),
            test.as_bytes(),
            true,
            &BuildConfig::default(),
            &EvalConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(err, Error::EmptyState { line: 1 }));
    }

    #[test]
    fn non_strict_mode_skips_invalid_records_without_duplicating_test_pass_warnings() {
        let train = "{\"sequence_id\":\"x\",\"step\":0,\"state\":\"s\",\"action\":\"a\"}\n{\"state\":\"\",\"action\":\"a\",\"sequence_id\":\"bad\",\"step\":0}\n";
        let test = "{\"sequence_id\":\"y\",\"step\":0,\"state\":\"\",\"action\":\"a\"}\n";
        let output = evaluate(
            train.as_bytes(),
            test.as_bytes(),
            false,
            &BuildConfig::default(),
            &EvalConfig::default(),
        )
        .unwrap();
        // Only the train pass's invalid line (2) is reported -- the test
        // pass's invalid line is skipped silently (decision 3).
        assert_eq!(output.warnings.len(), 1);
        assert_eq!(output.warnings[0].line, 2);
    }
}

/// Tuning knobs for [`evaluate`].
#[derive(Debug, Clone)]
pub struct EvalConfig {
    /// Fraction of sequences assigned to the train split (the rest go to
    /// test). See [`evaluate`]'s doc comment for how the split is decided.
    pub train_ratio: f64,
    /// Which top-k hit rates to report, e.g. `[1, 3, 5]`.
    pub top_k: Vec<usize>,
    /// Number of equal-width bins over `[0, 1]` for `confidence_calibration`.
    /// `None` (or `Some(0)`) skips calibration reporting (`confidence_calibration`
    /// comes back empty).
    pub calibration_bins: Option<usize>,
    /// Confidence thresholds to sweep for `threshold_sweep`. Empty skips it.
    pub thresholds: Vec<f64>,
}

impl Default for EvalConfig {
    fn default() -> Self {
        Self {
            train_ratio: 0.8,
            top_k: vec![1, 3, 5],
            calibration_bins: None,
            thresholds: Vec::new(),
        }
    }
}

/// Hit rate for one requested `k`. A `Vec` (not a map) so JSON output has a
/// deterministic order matching the order `top_k` was requested in.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopKHitRate {
    pub k: usize,
    pub hit_rate: Option<f64>,
}

/// Ranking quality for one equal-width confidence bin of the #1 candidate,
/// among evaluated test observations whose top1 confidence fell in
/// `[min_confidence, max_confidence)` (the last bin is closed on both ends).
/// Always exactly `EvalConfig::calibration_bins` entries, in ascending bin
/// order, regardless of whether a given bin saw any observations.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationBin {
    pub min_confidence: f64,
    pub max_confidence: f64,
    pub num_evaluated: u64,
    pub top1_hit_rate: Option<f64>,
    pub mean_reciprocal_rank: Option<f64>,
}

/// Selective-prediction metrics at one confidence threshold: if the caller
/// only acted when the #1 candidate's confidence was `>= min_confidence`,
/// how often would they have predicted at all, and how good were those
/// predictions? Always exactly `EvalConfig::thresholds.len()` entries, in
/// the requested order.
///
/// `covered_fraction`/`abstained_fraction` are a *different* weighting
/// convention than [`EvalReport::coverage`]/[`EvalReport::fallback_rate`]:
/// both are observation-weighted here and sum to 1 by construction
/// (`abstained_fraction = 1.0 - covered_fraction`), whereas the top-level
/// fields deliberately don't (state- vs. observation-weighted). Named
/// differently on purpose so the two pairs are never confused for each other
/// in the same JSON report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThresholdSweepEntry {
    pub min_confidence: f64,
    /// Fraction of *all* test observations where the state had a candidate
    /// and its #1 confidence was `>= min_confidence`.
    pub covered_fraction: f64,
    /// `1.0 - covered_fraction`.
    pub abstained_fraction: f64,
    /// Accuracy among covered observations only (`None` if none were covered).
    pub top1_hit_rate: Option<f64>,
    /// Mean reciprocal rank among covered observations only.
    pub mean_reciprocal_rank: Option<f64>,
}

/// Ranking-quality report produced by [`evaluate`].
///
/// `coverage` and `fallback_rate` intentionally do *not* sum to 1: `coverage`
/// is state-weighted (fraction of *distinct* test states for which the
/// prior returned any candidate), while `fallback_rate` is
/// observation-weighted (fraction of *test observations* whose state had no
/// candidates). One rarely-seen state with no candidates barely moves
/// `fallback_rate` but still costs a full point of `coverage`, and vice
/// versa. The raw counts below let you recompute either framing yourself.
///
/// `top1_hit_rate`, `topk_hit_rate`, `mean_reciprocal_rank`,
/// `avg_rank_when_found`, and the confidence/score-lift fields are all
/// conditioned on "evaluated" (the state had >=1 candidate) -- there is no
/// rank to score when there was no prediction at all. `coverage` /
/// `fallback_rate` already answer "did we even have a prediction?"
/// separately.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvalReport {
    pub num_train_observations: u64,
    pub num_test_observations: u64,
    /// Number of distinct states seen among test observations.
    pub num_test_states: u64,
    /// Test observations whose state had >=1 candidate in the prior.
    pub num_evaluated_observations: u64,
    /// Test observations whose state had zero candidates.
    pub num_fallback_observations: u64,
    /// Distinct test states for which the prior returned >=1 candidate.
    pub num_test_states_with_candidates: u64,
    /// State-weighted: `num_test_states_with_candidates / num_test_states`.
    pub coverage: Option<f64>,
    /// Observation-weighted: `num_fallback_observations / num_test_observations`.
    pub fallback_rate: Option<f64>,
    /// Among evaluated observations, fraction where the actual action was
    /// the prior's #1 ranked candidate.
    pub top1_hit_rate: Option<f64>,
    pub topk_hit_rate: Vec<TopKHitRate>,
    /// Mean of `1/rank` over evaluated observations (0 contribution when
    /// the actual action isn't among the candidates at all).
    pub mean_reciprocal_rank: Option<f64>,
    /// Mean rank of the actual action, over evaluated observations where it
    /// was found among the candidates (excludes not-found cases).
    pub avg_rank_when_found: Option<f64>,
    /// Mean confidence of the #1 candidate, restricted to evaluated
    /// observations where that #1 candidate matched the actual action.
    pub avg_confidence_on_hit: Option<f64>,
    /// Same, restricted to evaluated observations where it did not match.
    pub avg_confidence_on_miss: Option<f64>,
    /// `mean(observed score | #1 candidate matched actual action) -
    /// mean(observed score | it didn't)`, `None` unless both sides have at
    /// least one scored observation. Tests whether following the prior's
    /// top pick correlates with a better observed outcome.
    pub score_lift: Option<f64>,
    /// Ranking quality bucketed by the #1 candidate's confidence. Empty
    /// unless [`EvalConfig::calibration_bins`] was set.
    pub confidence_calibration: Vec<CalibrationBin>,
    /// Coverage/accuracy tradeoff at each requested confidence threshold.
    /// Empty unless [`EvalConfig::thresholds`] was non-empty.
    pub threshold_sweep: Vec<ThresholdSweepEntry>,
}

/// Result of [`evaluate`]: the report plus warnings from the train pass.
/// Warnings are collected from the train pass only -- when both readers
/// point at the same file (the only way the CLI uses this), a malformed
/// line is malformed identically in both passes, so a second collection
/// would just duplicate the first. The test pass still skips (non-strict)
/// or aborts (strict) on invalid records; it just doesn't re-report them.
#[derive(Debug)]
pub struct EvalOutput {
    pub report: EvalReport,
    pub warnings: Vec<Warning>,
}

/// Deterministically assigns `sequence_id` to the train split with
/// probability `train_ratio`, based purely on the id's own hash -- every
/// observation in the same sequence lands on the same side (no leakage)
/// without needing to look at the rest of the dataset, and streams fine
/// since each line can be classified independently. The train/test split
/// must stay reproducible if eval is re-run after a toolchain upgrade,
/// hence `crate::hash::fnv1a` rather than a stdlib hasher (see its doc
/// comment).
fn is_train(sequence_id: &str, train_ratio: f64) -> bool {
    let bucket = crate::hash::fnv1a(sequence_id.as_bytes()) % 100;
    let train_pct = (train_ratio * 100.0).round().clamp(0.0, 100.0) as u64;
    bucket < train_pct
}

/// Online per-bin totals for `confidence_calibration` -- sized to
/// `EvalConfig::calibration_bins`, never to the number of observations.
#[derive(Debug, Default, Clone, Copy)]
struct CalibrationBinAcc {
    num_evaluated: u64,
    hit_count: u64,
    reciprocal_rank_sum: f64,
}

/// Online per-threshold totals for `threshold_sweep` -- sized to
/// `EvalConfig::thresholds.len()`, never to the number of observations.
#[derive(Debug, Default, Clone, Copy)]
struct ThresholdAcc {
    covered_count: u64,
    hit_count: u64,
    reciprocal_rank_sum: f64,
}

/// Pass-2 bookkeeping: ranks each test observation's actual action against
/// the trained prior's candidates for its state, accumulating the sums
/// [`EvalReport`] is built from. Mirrors [`PriorAccumulator`]'s
/// new/observe/finish shape. Memory stays bounded by `top_k.len()` +
/// `calibration_bins` + `thresholds.len()`, never by the number of
/// observations -- calibration/threshold-sweep bucketing happens online,
/// the same way every other metric here does.
struct EvalAccumulator<'a> {
    top_k: &'a [usize],
    num_test_observations: u64,
    test_states_seen: HashSet<String>,
    states_with_candidates_count: u64,
    fallback_count: u64,
    evaluated_count: u64,
    top1_hit_count: u64,
    topk_hit_counts: HashMap<usize, u64>,
    reciprocal_rank_sum: f64,
    rank_sum_when_found: f64,
    found_count: u64,
    confidence_sum_on_hit: f64,
    confidence_count_on_hit: u64,
    confidence_sum_on_miss: f64,
    confidence_count_on_miss: u64,
    score_sum_on_hit: f64,
    score_count_on_hit: u64,
    score_sum_on_miss: f64,
    score_count_on_miss: u64,
    calibration_bin_width: f64,
    calibration: Vec<CalibrationBinAcc>,
    thresholds: &'a [f64],
    threshold_accs: Vec<ThresholdAcc>,
}

impl<'a> EvalAccumulator<'a> {
    fn new(top_k: &'a [usize], calibration_bins: usize, thresholds: &'a [f64]) -> Self {
        let calibration_bin_width = if calibration_bins > 0 {
            1.0 / calibration_bins as f64
        } else {
            0.0
        };
        Self {
            top_k,
            num_test_observations: 0,
            test_states_seen: HashSet::new(),
            states_with_candidates_count: 0,
            fallback_count: 0,
            evaluated_count: 0,
            top1_hit_count: 0,
            topk_hit_counts: HashMap::new(),
            reciprocal_rank_sum: 0.0,
            rank_sum_when_found: 0.0,
            found_count: 0,
            confidence_sum_on_hit: 0.0,
            confidence_count_on_hit: 0,
            confidence_sum_on_miss: 0.0,
            confidence_count_on_miss: 0,
            score_sum_on_hit: 0.0,
            score_count_on_hit: 0,
            score_sum_on_miss: 0.0,
            score_count_on_miss: 0,
            calibration_bin_width,
            calibration: vec![CalibrationBinAcc::default(); calibration_bins],
            thresholds,
            threshold_accs: vec![ThresholdAcc::default(); thresholds.len()],
        }
    }

    fn observe(&mut self, book: &PriorBook, obs: &Observation) {
        self.num_test_observations += 1;
        let is_new_state = self.test_states_seen.insert(obs.state.clone());
        let candidates = book.query(&obs.state, None);

        if candidates.is_empty() {
            self.fallback_count += 1;
            return;
        }
        if is_new_state {
            self.states_with_candidates_count += 1;
        }
        self.evaluated_count += 1;

        let top1 = &candidates[0];
        let is_hit = top1.action == obs.action;
        if is_hit {
            self.top1_hit_count += 1;
            self.confidence_sum_on_hit += top1.confidence;
            self.confidence_count_on_hit += 1;
            if let Some(score) = obs.score {
                self.score_sum_on_hit += score;
                self.score_count_on_hit += 1;
            }
        } else {
            self.confidence_sum_on_miss += top1.confidence;
            self.confidence_count_on_miss += 1;
            if let Some(score) = obs.score {
                self.score_sum_on_miss += score;
                self.score_count_on_miss += 1;
            }
        }

        let rank = candidates
            .iter()
            .position(|c| c.action == obs.action)
            .map(|index| index + 1);
        // Same convention `mean_reciprocal_rank` uses: 0 contribution when
        // the action wasn't found among the candidates at all.
        let reciprocal_rank = rank.map_or(0.0, |r| 1.0 / r as f64);
        if let Some(rank) = rank {
            self.found_count += 1;
            self.rank_sum_when_found += rank as f64;
            self.reciprocal_rank_sum += reciprocal_rank;
            for &k in self.top_k {
                if rank <= k {
                    *self.topk_hit_counts.entry(k).or_insert(0) += 1;
                }
            }
        }

        if !self.calibration.is_empty() {
            let bins = self.calibration.len();
            let idx = ((top1.confidence / self.calibration_bin_width) as usize).min(bins - 1);
            let bin = &mut self.calibration[idx];
            bin.num_evaluated += 1;
            if is_hit {
                bin.hit_count += 1;
            }
            bin.reciprocal_rank_sum += reciprocal_rank;
        }

        for (acc, &threshold) in self.threshold_accs.iter_mut().zip(self.thresholds) {
            if top1.confidence >= threshold {
                acc.covered_count += 1;
                if is_hit {
                    acc.hit_count += 1;
                }
                acc.reciprocal_rank_sum += reciprocal_rank;
            }
        }
    }

    fn finish(self, num_train_observations: u64) -> EvalReport {
        let num_test_states = self.test_states_seen.len() as u64;
        let coverage = ratio(
            self.states_with_candidates_count as f64,
            num_test_states as f64,
        );
        let fallback_rate = ratio(
            self.fallback_count as f64,
            self.num_test_observations as f64,
        );
        let evaluated = self.evaluated_count as f64;

        let topk_hit_rate = self
            .top_k
            .iter()
            .map(|&k| TopKHitRate {
                k,
                hit_rate: ratio(
                    *self.topk_hit_counts.get(&k).unwrap_or(&0) as f64,
                    evaluated,
                ),
            })
            .collect();

        let score_lift = match (
            ratio(self.score_sum_on_hit, self.score_count_on_hit as f64),
            ratio(self.score_sum_on_miss, self.score_count_on_miss as f64),
        ) {
            (Some(hit), Some(miss)) => Some(hit - miss),
            _ => None,
        };

        let confidence_calibration: Vec<CalibrationBin> = self
            .calibration
            .iter()
            .enumerate()
            .map(|(i, bin)| {
                let n = bin.num_evaluated as f64;
                CalibrationBin {
                    min_confidence: i as f64 * self.calibration_bin_width,
                    max_confidence: (i as f64 + 1.0) * self.calibration_bin_width,
                    num_evaluated: bin.num_evaluated,
                    top1_hit_rate: ratio(bin.hit_count as f64, n),
                    mean_reciprocal_rank: ratio(bin.reciprocal_rank_sum, n),
                }
            })
            .collect();

        let threshold_sweep: Vec<ThresholdSweepEntry> = self
            .thresholds
            .iter()
            .zip(self.threshold_accs.iter())
            .map(|(&threshold, acc)| {
                let covered_fraction =
                    ratio(acc.covered_count as f64, self.num_test_observations as f64)
                        .unwrap_or(0.0);
                ThresholdSweepEntry {
                    min_confidence: threshold,
                    covered_fraction,
                    abstained_fraction: 1.0 - covered_fraction,
                    top1_hit_rate: ratio(acc.hit_count as f64, acc.covered_count as f64),
                    mean_reciprocal_rank: ratio(acc.reciprocal_rank_sum, acc.covered_count as f64),
                }
            })
            .collect();

        EvalReport {
            num_train_observations,
            num_test_observations: self.num_test_observations,
            num_test_states,
            num_evaluated_observations: self.evaluated_count,
            num_fallback_observations: self.fallback_count,
            num_test_states_with_candidates: self.states_with_candidates_count,
            coverage,
            fallback_rate,
            top1_hit_rate: ratio(self.top1_hit_count as f64, evaluated),
            topk_hit_rate,
            mean_reciprocal_rank: ratio(self.reciprocal_rank_sum, evaluated),
            avg_rank_when_found: ratio(self.rank_sum_when_found, self.found_count as f64),
            avg_confidence_on_hit: ratio(
                self.confidence_sum_on_hit,
                self.confidence_count_on_hit as f64,
            ),
            avg_confidence_on_miss: ratio(
                self.confidence_sum_on_miss,
                self.confidence_count_on_miss as f64,
            ),
            score_lift,
            confidence_calibration,
            threshold_sweep,
        }
    }
}

/// Builds a prior from a sequence-held-out train split and reports how well
/// it ranks the actual action taken on the held-out test split.
///
/// Two streaming passes, each bounded by unique `(state, action)` pairs /
/// unique test states rather than total observation count, matching
/// [`crate::build_prior_book_from_reader`]'s memory profile:
///
/// 1. Read `train_reader`; observations whose `sequence_id` hashes into the
///    train bucket (see [`is_train`]) fold into a [`PriorAccumulator`].
/// 2. Read `test_reader`; observations whose `sequence_id` hashes into the
///    test bucket are ranked against the now-finished prior book.
///
/// Splitting by `sequence_id` (not by individual observation) keeps every
/// step of the same sequence on one side -- otherwise later steps could
/// leak information about earlier ones across the train/test boundary.
///
/// `train_reader` and `test_reader` are independent parameters (not a
/// single reader read twice) so the core stays IO-agnostic, matching
/// `build_prior_book_from_reader`'s precedent; the CLI opens the same file
/// path twice to get this shape.
///
/// Strict mode aborts on the first invalid record in *either* pass.
/// Non-strict mode skips invalid records in both passes; only the train
/// pass's skips are collected as [`Warning`]s (see [`EvalOutput`]'s doc
/// comment for why).
pub fn evaluate(
    train_reader: impl Read,
    test_reader: impl Read,
    strict: bool,
    build_config: &BuildConfig,
    eval_config: &EvalConfig,
) -> Result<EvalOutput> {
    let mut acc = PriorAccumulator::new(build_config);
    let mut warnings = Vec::new();
    let mut num_train_observations = 0u64;

    for (index, line) in BufReader::new(train_reader).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let line_no = index + 1;

        match parse_line(&line, line_no) {
            Ok(observation) => {
                if is_train(&observation.sequence_id, eval_config.train_ratio) {
                    acc.observe(&observation);
                    num_train_observations += 1;
                }
            }
            Err(err) if strict => return Err(err),
            Err(err) => warnings.push(Warning {
                line: line_no,
                message: err.to_string(),
            }),
        }
    }
    let book = acc.finish();

    let mut eval_acc = EvalAccumulator::new(
        &eval_config.top_k,
        eval_config.calibration_bins.unwrap_or(0),
        &eval_config.thresholds,
    );
    for (index, line) in BufReader::new(test_reader).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let line_no = index + 1;

        match parse_line(&line, line_no) {
            Ok(observation) => {
                if !is_train(&observation.sequence_id, eval_config.train_ratio) {
                    eval_acc.observe(&book, &observation);
                }
            }
            Err(err) if strict => return Err(err),
            Err(_) => {}
        }
    }

    Ok(EvalOutput {
        report: eval_acc.finish(num_train_observations),
        warnings,
    })
}
