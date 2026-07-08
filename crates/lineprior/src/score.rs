use crate::model::BuildConfig;

/// `numerator / denominator`, or `None` when nothing was observed.
/// Used for the raw (unsmoothed) `success_rate` / `mean_score` fields
/// reported on each [`crate::model::PriorAction`].
pub fn ratio(numerator: f64, denominator: f64) -> Option<f64> {
    if denominator > 0.0 {
        Some(numerator / denominator)
    } else {
        None
    }
}

/// Shrinks an observed rate toward `global_mean` by `alpha` pseudo-observations.
///
/// We apply smoothing because rare actions can otherwise get a misleading
/// 100% success rate from one lucky observation. An action with zero of its
/// own trials shrinks all the way to `global_mean`, which is a reasonable
/// fallback rather than an error.
pub fn shrink_toward(observed_sum: f64, observed_weight: f64, alpha: f64, global_mean: f64) -> f64 {
    (observed_sum + alpha * global_mean) / (observed_weight + alpha)
}

/// Combines the count, success, and score signals into one comparable score.
///
/// A `None` component (no outcome data, or no score data, anywhere in the
/// build) drops that term entirely rather than treating it as zero, per
/// lineprior's fallback rules: absent data must not silently score as "bad".
pub fn raw_score(
    weighted_count: f64,
    smoothed_success_rate: Option<f64>,
    smoothed_mean_score: Option<f64>,
    config: &BuildConfig,
) -> f64 {
    let mut score = config.count_weight * (1.0 + weighted_count).ln();
    if let Some(success) = smoothed_success_rate {
        score += config.success_weight * success;
    }
    if let Some(mean_score) = smoothed_mean_score {
        score += config.score_weight * mean_score;
    }
    score
}

/// Normalizes raw scores into a probability-like prior distribution.
///
/// Falls back to a uniform distribution when every raw score is zero
/// (e.g. all weights configured to zero) so callers never divide by zero.
pub fn normalize(raw_scores: &[f64]) -> Vec<f64> {
    let sum: f64 = raw_scores.iter().sum();
    if sum > 0.0 {
        raw_scores.iter().map(|s| s / sum).collect()
    } else if raw_scores.is_empty() {
        Vec::new()
    } else {
        vec![1.0 / raw_scores.len() as f64; raw_scores.len()]
    }
}

/// Heuristic confidence: approaches 1 as `weighted_count` grows past `k`.
/// This is not a statistical guarantee, just a sample-size heuristic.
pub fn confidence(weighted_count: f64, k: f64) -> f64 {
    weighted_count / (weighted_count + k)
}

/// Kish's effective sample size for a sum of (possibly unequal) trial
/// weights: `sum_weights^2 / sum_weights_squared`. For uniform weight=1.0
/// trials this equals the plain count; uneven weights reduce it, since a
/// few heavily-weighted trials carry less *information* than the same sum
/// spread evenly across many trials.
pub fn effective_sample_size(sum_weights: f64, sum_weights_squared: f64) -> f64 {
    if sum_weights_squared > 0.0 {
        sum_weights * sum_weights / sum_weights_squared
    } else {
        0.0
    }
}

/// Wilson score interval lower bound for proportion `successes /
/// effective_trials`. `effective_trials` need not be an integer count --
/// see [`effective_sample_size`] for the weighted/fractional-outcome case,
/// an engineering approximation rather than an exact interval in that case.
/// Returns `None` when `effective_trials <= 0.0` (no data to bound).
pub fn wilson_lower_bound(successes: f64, effective_trials: f64, z: f64) -> Option<f64> {
    if effective_trials <= 0.0 {
        return None;
    }
    // Clamped defensively: an out-of-[0,1] `p_hat` (e.g. from a
    // misconfigured draw_value) would otherwise make the radicand negative
    // and NaN silently slip past min_confidence filtering.
    let p_hat = (successes / effective_trials).clamp(0.0, 1.0);
    let z2 = z * z;
    let denom = 1.0 + z2 / effective_trials;
    let center = p_hat + z2 / (2.0 * effective_trials);
    let margin = z
        * ((p_hat * (1.0 - p_hat) / effective_trials)
            + z2 / (4.0 * effective_trials * effective_trials))
            .sqrt();
    Some(((center - margin) / denom).clamp(0.0, 1.0))
}

/// Shannon entropy (in bits) of a discrete distribution. Callers use this
/// to gauge whether one action dominates (low entropy, prior is useful) or
/// many actions compete (high entropy, fallback search may be safer).
pub fn entropy_bits(distribution: &[f64]) -> f64 {
    distribution
        .iter()
        .filter(|&&p| p > 0.0)
        .map(|&p| -p * p.log2())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_toward_pulls_low_sample_rate_to_global_mean() {
        // One success out of one trial looks perfect, but alpha=5 should
        // pull it far toward a 0.3 global rate rather than reporting 1.0.
        let smoothed = shrink_toward(1.0, 1.0, 5.0, 0.3);
        assert!((smoothed - 0.4166).abs() < 1e-3);
    }

    #[test]
    fn shrink_toward_zero_trials_falls_back_to_global_mean() {
        assert_eq!(shrink_toward(0.0, 0.0, 5.0, 0.3), 0.3);
    }

    #[test]
    fn raw_score_drops_missing_components() {
        let config = BuildConfig::default();
        let count_only = raw_score(10.0, None, None, &config);
        assert_eq!(count_only, config.count_weight * 11f64.ln());
    }

    #[test]
    fn normalize_sums_to_one() {
        let priors = normalize(&[1.0, 3.0]);
        assert!((priors.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!((priors[1] - 0.75).abs() < 1e-9);
    }

    #[test]
    fn normalize_falls_back_to_uniform_when_all_zero() {
        let priors = normalize(&[0.0, 0.0, 0.0]);
        assert_eq!(priors, vec![1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0]);
    }

    #[test]
    fn confidence_grows_toward_one_with_more_samples() {
        assert!((confidence(0.0, 20.0) - 0.0).abs() < 1e-9);
        assert!(confidence(1000.0, 20.0) > 0.98);
    }

    #[test]
    fn entropy_is_zero_for_a_single_dominant_action() {
        assert_eq!(entropy_bits(&[1.0, 0.0]), 0.0);
    }

    #[test]
    fn entropy_is_maximal_for_a_uniform_two_way_split() {
        assert!((entropy_bits(&[0.5, 0.5]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn effective_sample_size_uniform_weights_equals_count() {
        // 5 trials of weight 1.0 each: sum_w=5, sum_w_sq=5, n_eff=25/5=5.
        assert_eq!(effective_sample_size(5.0, 5.0), 5.0);
    }

    #[test]
    fn effective_sample_size_nonuniform_weights_is_smaller_than_count() {
        // Weights [10, 1, 1, 1, 1]: sum_w=14, sum_w_sq=104, n_eff=196/104.
        let n_eff = effective_sample_size(14.0, 104.0);
        assert!(
            n_eff < 5.0,
            "n_eff={n_eff} should be well below the raw count 5"
        );
        assert!((n_eff - 196.0 / 104.0).abs() < 1e-9);
    }

    #[test]
    fn effective_sample_size_zero_weight_is_zero() {
        assert_eq!(effective_sample_size(0.0, 0.0), 0.0);
    }

    #[test]
    fn wilson_lower_bound_is_conservative_for_one_of_one() {
        // A single success shouldn't read as near-certain.
        let bound = wilson_lower_bound(1.0, 1.0, 1.96).unwrap();
        assert!((bound - 0.2065).abs() < 1e-3);
    }

    #[test]
    fn wilson_lower_bound_grows_with_more_consistent_data() {
        let lucky = wilson_lower_bound(1.0, 1.0, 1.96).unwrap();
        let proven = wilson_lower_bound(90.0, 100.0, 1.96).unwrap();
        assert!(proven > lucky);
    }

    #[test]
    fn wilson_lower_bound_all_failures_is_near_zero() {
        let bound = wilson_lower_bound(0.0, 100.0, 1.96).unwrap();
        assert!(bound < 0.05);
    }

    #[test]
    fn wilson_lower_bound_none_when_no_effective_trials() {
        assert_eq!(wilson_lower_bound(0.0, 0.0, 1.96), None);
    }

    #[test]
    fn wilson_lower_bound_clamps_out_of_range_p_hat_instead_of_nan() {
        // successes > effective_trials shouldn't happen with well-formed
        // input, but a misconfigured draw_value could produce it -- must
        // clamp rather than propagate NaN through sqrt() of a negative.
        let bound = wilson_lower_bound(2.0, 1.0, 1.96).unwrap();
        assert!(bound.is_finite());
        assert!((0.0..=1.0).contains(&bound));
    }
}
