use crate::model::{BuildConfig, ScoringStrategy};

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
/// Negative utilities are shifted by the smallest finite value before
/// normalization, preserving their order without emitting negative priors.
/// Scaling by the largest adjusted value keeps the sum finite even when
/// callers use very large (but finite) utilities. Non-finite input falls
/// back conservatively: positive infinities share all mass, while NaN or an
/// all-negative-infinity vector becomes uniform.
pub fn normalize(raw_scores: &[f64]) -> Vec<f64> {
    if raw_scores.is_empty() {
        return Vec::new();
    }
    let uniform = || vec![1.0 / raw_scores.len() as f64; raw_scores.len()];
    if raw_scores.iter().any(|score| score.is_nan()) {
        return uniform();
    }

    let positive_infinities = raw_scores
        .iter()
        .filter(|score| **score == f64::INFINITY)
        .count();
    if positive_infinities > 0 {
        let mass = 1.0 / positive_infinities as f64;
        return raw_scores
            .iter()
            .map(|score| if *score == f64::INFINITY { mass } else { 0.0 })
            .collect();
    }

    let finite_min = raw_scores
        .iter()
        .copied()
        .filter(|score| score.is_finite())
        .reduce(f64::min);
    let Some(finite_min) = finite_min else {
        return uniform();
    };
    let finite_max = raw_scores
        .iter()
        .copied()
        .filter(|score| score.is_finite())
        .fold(f64::NEG_INFINITY, f64::max);
    let scale = finite_min.abs().max(finite_max.abs());
    if scale == 0.0 {
        return uniform();
    }
    let scaled_min = (finite_min / scale).min(0.0);
    let adjusted: Vec<f64> = raw_scores
        .iter()
        .map(|score| {
            if score.is_finite() {
                score / scale - scaled_min
            } else {
                0.0
            }
        })
        .collect();
    let max = adjusted.iter().copied().fold(0.0, f64::max);
    if max == 0.0 || !max.is_finite() {
        return uniform();
    }
    let scaled: Vec<f64> = adjusted.iter().map(|score| score / max).collect();
    let sum: f64 = scaled.iter().sum();
    scaled.into_iter().map(|score| score / sum).collect()
}

/// Stable softmax; subtracting the largest utility prevents overflow while
/// preserving the exact ranking and making low-temperature output useful.
pub fn softmax(raw_scores: &[f64], temperature: f64) -> Vec<f64> {
    if raw_scores.is_empty() {
        return Vec::new();
    }
    if raw_scores.iter().any(|score| score.is_nan()) || raw_scores.contains(&f64::INFINITY) {
        return normalize(raw_scores);
    }
    let Some(max) = raw_scores
        .iter()
        .copied()
        .filter(|score| score.is_finite())
        .reduce(f64::max)
    else {
        return normalize(raw_scores);
    };
    let values: Vec<f64> = raw_scores
        .iter()
        .map(|score| {
            if score.is_finite() {
                ((score - max) / temperature).exp()
            } else {
                0.0
            }
        })
        .collect();
    normalize(&values)
}

/// Selects the final normalization for a strategy. The build module owns
/// action statistics; this helper keeps strategy-independent distribution
/// mechanics public and testable.
pub fn normalize_for_strategy(
    raw_scores: &[f64],
    strategy: ScoringStrategy,
    temperature: f64,
) -> Vec<f64> {
    match strategy {
        ScoringStrategy::Softmax => softmax(raw_scores, temperature),
        _ => normalize(raw_scores),
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
        if sum_weights.abs() <= f64::MAX.sqrt() {
            sum_weights * sum_weights / sum_weights_squared
        } else {
            let scaled = sum_weights / sum_weights_squared.sqrt();
            scaled * scaled
        }
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

/// Exponential half-life decay: `1.0` at `age_days == 0`, `0.5` at one
/// half-life, `0.25` at two, etc. `age_days` is expected pre-clamped to
/// `>= 0` by the caller (a future-dated observation has age 0, not negative).
pub fn time_decay_multiplier(age_days: f64, half_life_days: f64) -> f64 {
    0.5_f64.powf(age_days / half_life_days)
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
    fn normalize_shifts_negative_utilities_without_emitting_negative_priors() {
        let priors = normalize(&[-1.0, 2.0]);
        assert_eq!(priors, vec![0.0, 1.0]);
    }

    #[test]
    fn normalize_handles_extreme_and_non_finite_utilities() {
        let huge = normalize(&[f64::MAX, f64::MAX]);
        assert_eq!(huge, vec![0.5, 0.5]);

        let opposite_extremes = normalize(&[f64::MIN, f64::MAX]);
        assert_eq!(opposite_extremes, vec![0.0, 1.0]);

        let infinite = normalize(&[1.0, f64::INFINITY, f64::INFINITY]);
        assert_eq!(infinite, vec![0.0, 0.5, 0.5]);

        let nan = normalize(&[1.0, f64::NAN]);
        assert_eq!(nan, vec![0.5, 0.5]);
    }

    #[test]
    fn softmax_is_normalized_and_temperature_sensitive() {
        let sharp = softmax(&[1.0, 2.0], 0.1);
        let flat = softmax(&[1.0, 2.0], 10.0);
        assert!((sharp.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        assert!(sharp[1] > flat[1]);
    }

    #[test]
    fn softmax_preserves_positive_infinity_as_dominant() {
        assert_eq!(softmax(&[1.0, f64::INFINITY], 1.0), vec![0.0, 1.0]);
        assert_eq!(softmax(&[f64::NEG_INFINITY; 2], 1.0), vec![0.5, 0.5]);
    }

    #[test]
    fn confidence_grows_toward_one_with_more_samples() {
        assert!((confidence(0.0, 20.0) - 0.0).abs() < 1e-9);
        assert!(confidence(1000.0, 20.0) > 0.98);
    }

    #[test]
    fn time_decay_multiplier_is_one_at_age_zero() {
        assert_eq!(time_decay_multiplier(0.0, 30.0), 1.0);
    }

    #[test]
    fn time_decay_multiplier_is_half_at_one_half_life() {
        assert!((time_decay_multiplier(30.0, 30.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn time_decay_multiplier_is_quarter_at_two_half_lives() {
        assert!((time_decay_multiplier(60.0, 30.0) - 0.25).abs() < 1e-9);
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
    fn effective_sample_size_avoids_intermediate_square_overflow() {
        let got = effective_sample_size(1e200, 1e300);
        assert!((got / 1e100 - 1.0).abs() < 1e-12);
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
