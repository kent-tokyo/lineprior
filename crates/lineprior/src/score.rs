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
}
