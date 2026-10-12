//! Minimal off-policy evaluation primitives.
//!
//! The caller supplies logging propensities and the evaluation policy's
//! probability for the action that was actually logged. This module does not
//! infer counterfactual rewards from a prior, and it stays separate from the
//! historical prior builder. Estimates are therefore only meaningful when
//! the caller can establish overlap and a valid logging policy.

use crate::score::effective_sample_size;
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
    /// Must be in `1..=MAX_BOOTSTRAP_RESAMPLES`.
    pub resamples: usize,
    pub seed: u64,
    /// Must be strictly between 0 and 1, e.g. `0.95`.
    pub confidence_level: f64,
}

/// Resource-safety limit for [`BootstrapConfig::resamples`].
pub const MAX_BOOTSTRAP_RESAMPLES: usize = 1_000_000;

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
    #[error("observation {index} produces a non-finite {field}")]
    NonFiniteDerivedValue { index: usize, field: &'static str },
    #[error("observation {index} has logging_propensity <= 0: {value}")]
    NonPositiveLoggingPropensity { index: usize, value: f64 },
    #[error("observation {index} has logging_propensity > 1: {value}")]
    LoggingPropensityAboveOne { index: usize, value: f64 },
    #[error("observation {index} has evaluation_probability outside [0, 1]: {value}")]
    InvalidEvaluationProbability { index: usize, value: f64 },
    #[error("observation {index} has an empty state")]
    EmptyState { index: usize },
    #[error("observation {index} has an empty action")]
    EmptyAction { index: usize },
    #[error("observation {index} is missing {field} required for doubly robust evaluation")]
    MissingRewardModelValue { index: usize, field: &'static str },
    #[error("bootstrap resamples must be in 1..=1000000 and confidence_level must be in (0, 1)")]
    InvalidBootstrapConfig,
}

/// Validate the row-level contract shared by every off-policy estimator.
///
/// This deliberately does not require reward-model fields: plain IPS does
/// not need them, while [`evaluate_doubly_robust`] reports their absence when
/// DR is explicitly requested. Propensities must be probabilities supplied
/// by the caller at decision time; this function never derives them from a
/// score or prior.
pub fn validate_offpolicy_observations(
    observations: &[OffPolicyObservation],
) -> Result<(), OffPolicyError> {
    for (index, observation) in observations.iter().enumerate() {
        if observation.state.is_empty() {
            return Err(OffPolicyError::EmptyState { index });
        }
        if observation.action.is_empty() {
            return Err(OffPolicyError::EmptyAction { index });
        }
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
        if observation.logging_propensity > 1.0 {
            return Err(OffPolicyError::LoggingPropensityAboveOne {
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
        for (field, value) in [
            (
                "reward_model_policy_value",
                observation.reward_model_policy_value,
            ),
            (
                "reward_model_logged_action",
                observation.reward_model_logged_action,
            ),
        ] {
            if let Some(value) = value {
                validate_finite(index, value, field)?;
            }
        }
    }
    Ok(())
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
    validate_offpolicy_observations(observations)?;

    evaluate_validated_ips(observations.iter(), observations.len(), config)
}

#[derive(Debug, Default)]
struct StableMean {
    value: f64,
    count: usize,
}

impl StableMean {
    fn observe(&mut self, value: f64) {
        self.count += 1;
        let alpha = 1.0 / self.count as f64;
        self.value = convex_combination(self.value, value, alpha);
    }

    fn value(&self) -> Option<f64> {
        (self.count > 0).then_some(self.value)
    }
}

/// Scale-free sums for Kish ESS. Keeping every weight relative to the
/// largest observed supported weight avoids overflowing `weight * weight`
/// while preserving the exact ESS ratio.
#[derive(Debug, Default)]
struct StableImportanceStats {
    max_weight: f64,
    scaled_weight_sum: f64,
    scaled_weight_squared_sum: f64,
}

impl StableImportanceStats {
    /// Adds one positive finite weight and returns its normalized share of
    /// the new total. That share can update a weighted mean without ever
    /// materializing the potentially overflowing raw weight sum.
    fn observe(&mut self, weight: f64) -> f64 {
        if self.scaled_weight_sum == 0.0 {
            self.max_weight = weight;
            self.scaled_weight_sum = 1.0;
            self.scaled_weight_squared_sum = 1.0;
            return 1.0;
        }

        if weight > self.max_weight {
            let scale = self.max_weight / weight;
            self.scaled_weight_sum *= scale;
            self.scaled_weight_squared_sum *= scale * scale;
            self.max_weight = weight;
        }
        let scaled_weight = weight / self.max_weight;
        let new_sum = self.scaled_weight_sum + scaled_weight;
        let share = scaled_weight / new_sum;
        self.scaled_weight_sum = new_sum;
        self.scaled_weight_squared_sum += scaled_weight * scaled_weight;
        share
    }

    fn effective_sample_size(&self) -> Option<f64> {
        (self.scaled_weight_squared_sum > 0.0)
            .then(|| effective_sample_size(self.scaled_weight_sum, self.scaled_weight_squared_sum))
    }
}

fn convex_combination(current: f64, next: f64, next_share: f64) -> f64 {
    if current == 0.0 || current.is_sign_positive() == next.is_sign_positive() {
        current + next_share * (next - current)
    } else {
        current * (1.0 - next_share) + next * next_share
    }
}

fn finite_derived(index: usize, value: f64, field: &'static str) -> Result<f64, OffPolicyError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(OffPolicyError::NonFiniteDerivedValue { index, field })
    }
}

/// Shared arithmetic for validated original rows and bootstrap references.
/// Keeping resamples as references avoids cloning caller-owned strings and
/// metadata while preserving the exact sampling order and formulas.
fn evaluate_validated_ips<'a>(
    observations: impl IntoIterator<Item = &'a OffPolicyObservation>,
    num_observations: usize,
    config: &OffPolicyConfig,
) -> Result<OffPolicyReport, OffPolicyError> {
    let mut ips_mean = StableMean::default();
    let mut importance_stats = StableImportanceStats::default();
    let mut self_normalized_ips = None;
    let mut supported_observations = 0;
    let mut overlap_failure_count = 0;
    let mut max_observed_importance_weight: Option<f64> = None;

    for (index, observation) in observations.into_iter().enumerate() {
        let mut ips_contribution = 0.0;
        if observation.evaluation_probability == 0.0 {
            overlap_failure_count += 1;
        } else {
            let importance_weight = finite_derived(
                index,
                observation.evaluation_probability / observation.logging_propensity,
                "importance weight",
            )?;
            max_observed_importance_weight = Some(
                max_observed_importance_weight
                    .map_or(importance_weight, |max| max.max(importance_weight)),
            );
            if config
                .max_importance_weight
                .is_some_and(|max| importance_weight > max)
            {
                overlap_failure_count += 1;
            } else {
                supported_observations += 1;
                ips_contribution = finite_derived(
                    index,
                    importance_weight * observation.reward,
                    "importance-weighted reward",
                )?;
                let share = importance_stats.observe(importance_weight);
                self_normalized_ips = Some(match self_normalized_ips {
                    Some(current) => convex_combination(current, observation.reward, share),
                    None => observation.reward,
                });
            }
        }
        ips_mean.observe(ips_contribution);
    }

    let ips = if num_observations == 0 {
        None
    } else if supported_observations > 0 {
        ips_mean.value()
    } else {
        None
    };

    Ok(OffPolicyReport {
        policy_name: config.policy_name.clone(),
        policy_version: config.policy_version.clone(),
        num_observations,
        supported_observations,
        overlap_failure_count,
        support_fraction: if num_observations == 0 {
            0.0
        } else {
            supported_observations as f64 / num_observations as f64
        },
        ips,
        self_normalized_ips,
        effective_sample_size: importance_stats.effective_sample_size(),
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
        || bootstrap_config.resamples > MAX_BOOTSTRAP_RESAMPLES
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
        let sample = (0..observations.len())
            .map(|_| &observations[next_index(&mut rng, observations.len())]);
        let report = evaluate_validated_ips(sample, observations.len(), policy_config)?;
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
    // Reduce the full 64-bit state before narrowing so a fixed seed produces
    // the same index stream on wasm32 and 64-bit native targets.
    (*state % length as u64) as usize
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
    validate_offpolicy_observations(observations)?;
    let mut estimate_mean = StableMean::default();
    let mut importance_stats = StableImportanceStats::default();
    let mut supported_observations = 0;
    let mut overlap_failure_count = 0;
    let mut max_observed_importance_weight: Option<f64> = None;

    for (index, observation) in observations.iter().enumerate() {
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

        let correction = if observation.evaluation_probability == 0.0 {
            overlap_failure_count += 1;
            None
        } else {
            let importance_weight = finite_derived(
                index,
                observation.evaluation_probability / observation.logging_propensity,
                "importance weight",
            )?;
            max_observed_importance_weight = Some(
                max_observed_importance_weight
                    .map_or(importance_weight, |max| max.max(importance_weight)),
            );
            if config
                .max_importance_weight
                .is_some_and(|max| importance_weight > max)
            {
                overlap_failure_count += 1;
                None
            } else {
                supported_observations += 1;
                importance_stats.observe(importance_weight);
                Some(importance_weight)
            }
        };
        let doubly_robust_value = if let Some(correction) = correction {
            let residual = finite_derived(
                index,
                observation.reward - model_logged_action,
                "reward-model residual",
            )?;
            let correction =
                finite_derived(index, correction * residual, "doubly robust correction")?;
            finite_derived(
                index,
                model_policy_value + correction,
                "doubly robust value",
            )?
        } else {
            model_policy_value
        };
        estimate_mean.observe(doubly_robust_value);
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
        estimate: estimate_mean.value(),
        effective_sample_size: importance_stats.effective_sample_size(),
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
    fn extreme_finite_ips_inputs_keep_report_values_finite() {
        let repeated_max = evaluate_self_normalized_ips(
            &[
                observation(f64::MAX, 1.0, 1.0),
                observation(f64::MAX, 1.0, 1.0),
            ],
            &Default::default(),
        )
        .unwrap();
        assert_eq!(repeated_max.ips, Some(f64::MAX));
        assert_eq!(repeated_max.self_normalized_ips, Some(f64::MAX));
        assert_eq!(repeated_max.effective_sample_size, Some(2.0));

        let large_weight =
            evaluate_self_normalized_ips(&[observation(1e-200, 1e-200, 1.0)], &Default::default())
                .unwrap();
        assert!(large_weight.ips.unwrap().is_finite());
        assert_eq!(large_weight.self_normalized_ips, Some(1e-200));
        assert_eq!(large_weight.effective_sample_size, Some(1.0));
    }

    #[test]
    fn unrepresentable_importance_weight_is_rejected() {
        assert!(matches!(
            evaluate_self_normalized_ips(
                &[observation(1.0, f64::from_bits(1), 1.0)],
                &Default::default()
            ),
            Err(OffPolicyError::NonFiniteDerivedValue {
                field: "importance weight",
                ..
            })
        ));
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
            evaluate_self_normalized_ips(&[observation(0.0, 1.1, 1.0)], &Default::default()),
            Err(OffPolicyError::LoggingPropensityAboveOne { .. })
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
    fn empty_state_or_action_is_rejected_by_the_row_contract() {
        let mut row = observation(1.0, 1.0, 1.0);
        row.state.clear();
        assert!(matches!(
            validate_offpolicy_observations(&[row]),
            Err(OffPolicyError::EmptyState { index: 0 })
        ));

        let mut row = observation(1.0, 1.0, 1.0);
        row.action.clear();
        assert!(matches!(
            validate_offpolicy_observations(&[row]),
            Err(OffPolicyError::EmptyAction { index: 0 })
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
    fn unsupported_extreme_dr_rows_do_not_form_zero_times_infinity() {
        let mut row = observation(f64::MAX, 1.0, 0.0);
        row.reward_model_policy_value = Some(f64::MAX);
        row.reward_model_logged_action = Some(-f64::MAX);
        let report = evaluate_doubly_robust(&[row.clone(), row], &Default::default()).unwrap();

        assert_eq!(report.estimate, Some(f64::MAX));
        assert_eq!(report.supported_observations, 0);
        assert_eq!(report.overlap_failure_count, 2);
        assert_eq!(report.effective_sample_size, None);
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
    fn bootstrap_index_uses_the_full_u64_state_before_narrowing() {
        let mut state = u64::from(u32::MAX);
        let expected_state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let expected_index = (expected_state % 7) as usize;

        assert_eq!(next_index(&mut state, 7), expected_index);
        assert_eq!(state, expected_state);
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
        assert!(matches!(
            bootstrap_self_normalized_ips(
                &[],
                &Default::default(),
                BootstrapConfig {
                    resamples: MAX_BOOTSTRAP_RESAMPLES + 1,
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
