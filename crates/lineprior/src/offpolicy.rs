//! Minimal off-policy evaluation primitives.
//!
//! The caller supplies logging propensities and the evaluation policy's
//! probability for the action that was actually logged. This module does not
//! infer counterfactual rewards from a prior, and it stays separate from the
//! historical prior builder. Estimates are therefore only meaningful when
//! the caller can establish overlap and a valid logging policy.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// One logged decision used by IPS and doubly robust evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffPolicyObservation {
    pub state: String,
    pub action: String,
    pub reward: f64,
    /// Probability assigned by the policy that generated the log to the
    /// action that was actually taken.
    pub logging_propensity: f64,
    /// Probability the policy being evaluated assigns to that same action.
    /// Zero means the row has no support for the evaluated policy.
    pub evaluation_probability: f64,
    /// Reward-model estimate for the evaluated policy's expected reward at
    /// this state: `sum_a evaluation_policy(a|state) * model(state, a)`.
    #[serde(default)]
    pub reward_model_policy_value: Option<f64>,
    /// Reward-model estimate for the action that was actually logged.
    #[serde(default)]
    pub reward_model_logged_action: Option<f64>,
    /// Opaque caller-owned audit metadata, such as a split or policy id.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// Safety limits for importance sampling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffPolicyConfig {
    /// Rows whose importance ratio exceeds this value are excluded and
    /// counted as overlap failures. `None` disables the cap.
    pub max_importance_weight: Option<f64>,
    pub policy_name: String,
    pub policy_version: Option<String>,
}

/// Reproducible percentile-bootstrap controls for IPS estimates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BootstrapConfig {
    pub resamples: usize,
    pub seed: u64,
    /// Must be strictly between 0 and 1, e.g. `0.95`.
    pub confidence_level: f64,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            resamples: 1_000,
            seed: 0,
            confidence_level: 0.95,
        }
    }
}

impl Default for OffPolicyConfig {
    fn default() -> Self {
        Self {
            max_importance_weight: None,
            policy_name: "unspecified".to_string(),
            policy_version: None,
        }
    }
}

/// Errors for malformed propensity/reward data or invalid safety limits.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum OffPolicyError {
    #[error("max_importance_weight must be finite and > 0")]
    InvalidMaxImportanceWeight,
    #[error("observation {index} has a non-finite {field}")]
    NonFiniteValue { index: usize, field: &'static str },
    #[error("observation {index} has logging_propensity <= 0: {value}")]
    NonPositiveLoggingPropensity { index: usize, value: f64 },
    #[error("observation {index} has evaluation_probability outside [0, 1]: {value}")]
    InvalidEvaluationProbability { index: usize, value: f64 },
    #[error("observation {index} is missing {field} required for doubly robust evaluation")]
    MissingRewardModelValue { index: usize, field: &'static str },
    #[error("bootstrap resamples must be > 0 and confidence_level must be in (0, 1)")]
    InvalidBootstrapConfig,
}

/// Report from self-normalized inverse propensity scoring.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffPolicyReport {
    pub policy_name: String,
    pub policy_version: Option<String>,
    pub num_observations: usize,
    pub supported_observations: usize,
    pub overlap_failure_count: usize,
    /// Supported rows divided by all valid rows. This is not a causal
    /// guarantee; it is an explicit overlap diagnostic.
    pub support_fraction: f64,
    /// Ordinary IPS: `sum(importance_weight * reward) / N`.
    pub ips: Option<f64>,
    /// Self-normalized IPS: `sum(weight * reward) / sum(weight)`.
    pub self_normalized_ips: Option<f64>,
    /// Kish effective sample size over supported importance weights.
    pub effective_sample_size: Option<f64>,
    pub max_observed_importance_weight: Option<f64>,
}

/// Report from doubly robust evaluation using caller-supplied reward-model values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoublyRobustReport {
    pub policy_name: String,
    pub policy_version: Option<String>,
    pub num_observations: usize,
    pub supported_observations: usize,
    pub overlap_failure_count: usize,
    pub support_fraction: f64,
    /// Mean of `model_policy_value + importance_weight * (reward - model_logged_action)`.
    pub estimate: Option<f64>,
    pub effective_sample_size: Option<f64>,
    pub max_observed_importance_weight: Option<f64>,
}

/// Percentile interval for one bootstrap statistic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BootstrapInterval {
    pub lower: f64,
    pub upper: f64,
    pub successful_resamples: usize,
    pub skipped_resamples: usize,
}

/// Reproducible uncertainty report for IPS and self-normalized IPS.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffPolicyBootstrapReport {
    pub seed: u64,
    pub resamples: usize,
    pub confidence_level: f64,
    pub ips: Option<BootstrapInterval>,
    pub self_normalized_ips: Option<BootstrapInterval>,
}

/// Evaluate an explicitly supplied policy with ordinary and self-normalized IPS.
///
/// Rows with zero evaluation probability are valid but unsupported and are
/// reported rather than treated as zero-reward counterfactuals. Rows above
/// `max_importance_weight` are handled the same way, making the overlap trade-
/// off visible in the report. If no row is supported, both estimates are
/// `None`.
pub fn evaluate_self_normalized_ips(
    observations: &[OffPolicyObservation],
    config: &OffPolicyConfig,
) -> Result<OffPolicyReport, OffPolicyError> {
    if let Some(max) = config.max_importance_weight
        && (!max.is_finite() || max <= 0.0)
    {
        return Err(OffPolicyError::InvalidMaxImportanceWeight);
    }

    let mut weighted_reward_sum = 0.0;
    let mut importance_sum = 0.0;
    let mut importance_squared_sum = 0.0;
    let mut supported_observations = 0;
    let mut overlap_failure_count = 0;
    let mut max_observed_importance_weight: Option<f64> = None;

    for (index, observation) in observations.iter().enumerate() {
        validate_finite(index, observation.reward, "reward")?;
        validate_finite(index, observation.logging_propensity, "logging_propensity")?;
        validate_finite(
            index,
            observation.evaluation_probability,
            "evaluation_probability",
        )?;
        if observation.logging_propensity <= 0.0 {
            return Err(OffPolicyError::NonPositiveLoggingPropensity {
                index,
                value: observation.logging_propensity,
            });
        }
        if !(0.0..=1.0).contains(&observation.evaluation_probability) {
            return Err(OffPolicyError::InvalidEvaluationProbability {
                index,
                value: observation.evaluation_probability,
            });
        }
        if observation.evaluation_probability == 0.0 {
            overlap_failure_count += 1;
            continue;
        }

        let importance_weight = observation.evaluation_probability / observation.logging_propensity;
        max_observed_importance_weight = Some(
            max_observed_importance_weight
                .map_or(importance_weight, |max| max.max(importance_weight)),
        );
        if config
            .max_importance_weight
            .is_some_and(|max| importance_weight > max)
        {
            overlap_failure_count += 1;
            continue;
        }
        supported_observations += 1;
        importance_sum += importance_weight;
        importance_squared_sum += importance_weight * importance_weight;
        weighted_reward_sum += importance_weight * observation.reward;
    }

    let ips = if observations.is_empty() {
        None
    } else if supported_observations > 0 {
        Some(weighted_reward_sum / observations.len() as f64)
    } else {
        None
    };
    let self_normalized_ips =
        (importance_sum > 0.0).then_some(weighted_reward_sum / importance_sum);
    let effective_sample_size = (importance_squared_sum > 0.0)
        .then_some(importance_sum * importance_sum / importance_squared_sum);

    Ok(OffPolicyReport {
        policy_name: config.policy_name.clone(),
        policy_version: config.policy_version.clone(),
        num_observations: observations.len(),
        supported_observations,
        overlap_failure_count,
        support_fraction: if observations.is_empty() {
            0.0
        } else {
            supported_observations as f64 / observations.len() as f64
        },
        ips,
        self_normalized_ips,
        effective_sample_size,
        max_observed_importance_weight,
    })
}

/// Estimate percentile-bootstrap intervals for the two IPS statistics.
///
/// The resampling stream is a small deterministic xorshift generator owned by
/// this module, so identical inputs and [`BootstrapConfig`] values produce
/// identical intervals without adding a random dependency. A resample with
/// no supported rows is counted in `skipped_resamples`, never converted into
/// a fabricated zero reward.
pub fn bootstrap_self_normalized_ips(
    observations: &[OffPolicyObservation],
    policy_config: &OffPolicyConfig,
    bootstrap_config: BootstrapConfig,
) -> Result<OffPolicyBootstrapReport, OffPolicyError> {
    if bootstrap_config.resamples == 0
        || !bootstrap_config.confidence_level.is_finite()
        || !(0.0..1.0).contains(&bootstrap_config.confidence_level)
    {
        return Err(OffPolicyError::InvalidBootstrapConfig);
    }
    // Validate the original log once, before generating any resamples, so a
    // malformed row cannot be hidden by a particular random draw.
    evaluate_self_normalized_ips(observations, policy_config)?;
    if observations.is_empty() {
        return Ok(OffPolicyBootstrapReport {
            seed: bootstrap_config.seed,
            resamples: bootstrap_config.resamples,
            confidence_level: bootstrap_config.confidence_level,
            ips: None,
            self_normalized_ips: None,
        });
    }

    let mut rng = bootstrap_config.seed;
    let mut ips_values = Vec::with_capacity(bootstrap_config.resamples);
    let mut self_normalized_values = Vec::with_capacity(bootstrap_config.resamples);
    for _ in 0..bootstrap_config.resamples {
        let mut sample = Vec::with_capacity(observations.len());
        for _ in observations {
            sample.push(observations[next_index(&mut rng, observations.len())].clone());
        }
        let report = evaluate_self_normalized_ips(&sample, policy_config)?;
        if let Some(value) = report.ips {
            ips_values.push(value);
        }
        if let Some(value) = report.self_normalized_ips {
            self_normalized_values.push(value);
        }
    }
    let skipped_ips = bootstrap_config.resamples - ips_values.len();
    let skipped_self_normalized = bootstrap_config.resamples - self_normalized_values.len();
    Ok(OffPolicyBootstrapReport {
        seed: bootstrap_config.seed,
        resamples: bootstrap_config.resamples,
        confidence_level: bootstrap_config.confidence_level,
        ips: percentile_interval(
            &mut ips_values,
            bootstrap_config.confidence_level,
            skipped_ips,
        ),
        self_normalized_ips: percentile_interval(
            &mut self_normalized_values,
            bootstrap_config.confidence_level,
            skipped_self_normalized,
        ),
    })
}

fn next_index(state: &mut u64, length: usize) -> usize {
    // A nonzero odd increment makes a zero seed progress too. This is for
    // reproducible resampling, not cryptographic randomness.
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1);
    (*state as usize) % length
}

fn percentile_interval(
    values: &mut [f64],
    confidence_level: f64,
    skipped_resamples: usize,
) -> Option<BootstrapInterval> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let tail = (1.0 - confidence_level) / 2.0;
    Some(BootstrapInterval {
        lower: percentile(values, tail),
        upper: percentile(values, 1.0 - tail),
        successful_resamples: values.len(),
        skipped_resamples,
    })
}

fn percentile(values: &[f64], quantile: f64) -> f64 {
    let position = quantile * (values.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        values[lower]
    } else {
        values[lower] + (values[upper] - values[lower]) * (position - lower as f64)
    }
}

/// Evaluate a policy with the doubly robust estimator.
///
/// Every row must carry both reward-model values. Rows without overlap still
/// contribute the model policy value and are counted in the report; this is
/// valid algebraically but makes the result depend more heavily on the model,
/// so `support_fraction` must be reported alongside the estimate.
pub fn evaluate_doubly_robust(
    observations: &[OffPolicyObservation],
    config: &OffPolicyConfig,
) -> Result<DoublyRobustReport, OffPolicyError> {
    if let Some(max) = config.max_importance_weight
        && (!max.is_finite() || max <= 0.0)
    {
        return Err(OffPolicyError::InvalidMaxImportanceWeight);
    }
    let mut estimate_sum = 0.0;
    let mut importance_sum = 0.0;
    let mut importance_squared_sum = 0.0;
    let mut supported_observations = 0;
    let mut overlap_failure_count = 0;
    let mut max_observed_importance_weight: Option<f64> = None;

    for (index, observation) in observations.iter().enumerate() {
        validate_finite(index, observation.reward, "reward")?;
        validate_finite(index, observation.logging_propensity, "logging_propensity")?;
        validate_finite(
            index,
            observation.evaluation_probability,
            "evaluation_probability",
        )?;
        let model_policy_value = observation.reward_model_policy_value.ok_or(
            OffPolicyError::MissingRewardModelValue {
                index,
                field: "reward_model_policy_value",
            },
        )?;
        let model_logged_action = observation.reward_model_logged_action.ok_or(
            OffPolicyError::MissingRewardModelValue {
                index,
                field: "reward_model_logged_action",
            },
        )?;
        validate_finite(index, model_policy_value, "reward_model_policy_value")?;
        validate_finite(index, model_logged_action, "reward_model_logged_action")?;
        if observation.logging_propensity <= 0.0 {
            return Err(OffPolicyError::NonPositiveLoggingPropensity {
                index,
                value: observation.logging_propensity,
            });
        }
        if !(0.0..=1.0).contains(&observation.evaluation_probability) {
            return Err(OffPolicyError::InvalidEvaluationProbability {
                index,
                value: observation.evaluation_probability,
            });
        }

        let correction = if observation.evaluation_probability == 0.0 {
            overlap_failure_count += 1;
            0.0
        } else {
            let importance_weight =
                observation.evaluation_probability / observation.logging_propensity;
            max_observed_importance_weight = Some(
                max_observed_importance_weight
                    .map_or(importance_weight, |max| max.max(importance_weight)),
            );
            if config
                .max_importance_weight
                .is_some_and(|max| importance_weight > max)
            {
                overlap_failure_count += 1;
                0.0
            } else {
                supported_observations += 1;
                importance_sum += importance_weight;
                importance_squared_sum += importance_weight * importance_weight;
                importance_weight
            }
        };
        let doubly_robust_value =
            model_policy_value + correction * (observation.reward - model_logged_action);
        estimate_sum += doubly_robust_value;
    }

    Ok(DoublyRobustReport {
        policy_name: config.policy_name.clone(),
        policy_version: config.policy_version.clone(),
        num_observations: observations.len(),
        supported_observations,
        overlap_failure_count,
        support_fraction: if observations.is_empty() {
            0.0
        } else {
            supported_observations as f64 / observations.len() as f64
        },
        estimate: (!observations.is_empty()).then_some(estimate_sum / observations.len() as f64),
        effective_sample_size: (importance_squared_sum > 0.0)
            .then_some(importance_sum * importance_sum / importance_squared_sum),
        max_observed_importance_weight,
    })
}

fn validate_finite(index: usize, value: f64, field: &'static str) -> Result<(), OffPolicyError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(OffPolicyError::NonFiniteValue { index, field })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(reward: f64, logging: f64, evaluation: f64) -> OffPolicyObservation {
        OffPolicyObservation {
            state: "s".to_string(),
            action: "a".to_string(),
            reward,
            logging_propensity: logging,
            evaluation_probability: evaluation,
            reward_model_policy_value: None,
            reward_model_logged_action: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn computes_ips_snips_and_effective_sample_size() {
        let report = evaluate_self_normalized_ips(
            &[observation(1.0, 0.5, 0.5), observation(0.0, 0.25, 0.5)],
            &OffPolicyConfig {
                policy_name: "candidate".to_string(),
                policy_version: Some("v1".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report.supported_observations, 2);
        assert_eq!(report.overlap_failure_count, 0);
        assert_eq!(report.ips, Some(0.5));
        assert!((report.self_normalized_ips.unwrap() - 1.0 / 3.0).abs() < 1e-12);
        assert!((report.effective_sample_size.unwrap() - 1.8).abs() < 1e-12);
        assert_eq!(report.policy_version.as_deref(), Some("v1"));
    }

    #[test]
    fn unsupported_rows_are_reported_not_fabricated() {
        let report =
            evaluate_self_normalized_ips(&[observation(99.0, 1.0, 0.0)], &Default::default())
                .unwrap();
        assert_eq!(report.support_fraction, 0.0);
        assert_eq!(report.ips, None);
        assert_eq!(report.self_normalized_ips, None);
        assert_eq!(report.overlap_failure_count, 1);
    }

    #[test]
    fn importance_weight_cap_exposes_overlap_failures() {
        let report = evaluate_self_normalized_ips(
            &[observation(1.0, 0.01, 1.0), observation(0.5, 0.5, 0.5)],
            &OffPolicyConfig {
                max_importance_weight: Some(10.0),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report.supported_observations, 1);
        assert_eq!(report.overlap_failure_count, 1);
        assert_eq!(report.max_observed_importance_weight, Some(100.0));
        assert_eq!(report.ips, Some(0.25));
    }

    #[test]
    fn malformed_propensities_are_rejected() {
        assert!(matches!(
            evaluate_self_normalized_ips(&[observation(0.0, 0.0, 1.0)], &Default::default()),
            Err(OffPolicyError::NonPositiveLoggingPropensity { .. })
        ));
        assert!(matches!(
            evaluate_self_normalized_ips(&[observation(0.0, 1.0, 1.1)], &Default::default()),
            Err(OffPolicyError::InvalidEvaluationProbability { .. })
        ));
        assert!(matches!(
            evaluate_self_normalized_ips(&[observation(f64::NAN, 1.0, 1.0)], &Default::default()),
            Err(OffPolicyError::NonFiniteValue {
                field: "reward",
                ..
            })
        ));
        assert!(matches!(
            evaluate_self_normalized_ips(
                &[],
                &OffPolicyConfig {
                    max_importance_weight: Some(0.0),
                    ..Default::default()
                }
            ),
            Err(OffPolicyError::InvalidMaxImportanceWeight)
        ));
    }

    #[test]
    fn doubly_robust_uses_model_baseline_and_ips_correction() {
        let mut first = observation(1.0, 0.5, 0.5);
        first.reward_model_policy_value = Some(0.4);
        first.reward_model_logged_action = Some(0.4);
        let report = evaluate_doubly_robust(&[first], &Default::default()).unwrap();
        assert_eq!(report.estimate, Some(1.0));
        assert_eq!(report.supported_observations, 1);
    }

    #[test]
    fn doubly_robust_requires_both_model_values() {
        let mut row = observation(1.0, 1.0, 1.0);
        row.reward_model_policy_value = Some(0.5);
        assert!(matches!(
            evaluate_doubly_robust(&[row], &Default::default()),
            Err(OffPolicyError::MissingRewardModelValue {
                field: "reward_model_logged_action",
                ..
            })
        ));
    }

    #[test]
    fn bootstrap_is_seeded_and_reports_finite_intervals() {
        let observations = vec![
            observation(1.0, 0.5, 0.5),
            observation(0.0, 0.5, 0.5),
            observation(0.5, 0.5, 0.5),
        ];
        let config = BootstrapConfig {
            resamples: 64,
            seed: 42,
            confidence_level: 0.9,
        };
        let first =
            bootstrap_self_normalized_ips(&observations, &Default::default(), config).unwrap();
        let second =
            bootstrap_self_normalized_ips(&observations, &Default::default(), config).unwrap();
        assert_eq!(first, second);
        let interval = first.self_normalized_ips.unwrap();
        assert!(interval.lower.is_finite());
        assert!(interval.upper.is_finite());
        assert_eq!(interval.successful_resamples, 64);
        assert_eq!(interval.skipped_resamples, 0);
    }

    #[test]
    fn bootstrap_rejects_invalid_controls_and_counts_no_support() {
        assert!(matches!(
            bootstrap_self_normalized_ips(
                &[],
                &Default::default(),
                BootstrapConfig {
                    resamples: 0,
                    ..Default::default()
                }
            ),
            Err(OffPolicyError::InvalidBootstrapConfig)
        ));
        let report = bootstrap_self_normalized_ips(
            &[observation(1.0, 1.0, 0.0)],
            &Default::default(),
            BootstrapConfig {
                resamples: 16,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report.ips, None);
        assert_eq!(report.self_normalized_ips, None);
    }
}
