//! Opt-in similarity fallback over caller-supplied, already-known states.
//!
//! This module deliberately does not compute embeddings, search a vector
//! database, or invent actions. A caller owns state representation and
//! nearest-neighbor retrieval, then supplies the resulting states with a
//! distance and provenance label. The book only contributes actions that are
//! already present for those states.

use crate::error::{Error, Result};
use crate::model::PriorBook;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One caller-provided state considered as a neighbor of a query state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilarState {
    pub state: String,
    /// Non-negative distance in the caller's representation space.
    pub distance: f64,
    /// Opaque explanation such as an index name, embedding version, or
    /// retrieval query id. It is returned unchanged and never interpreted.
    pub provenance: String,
}

/// Controls the conservative weighting of caller-provided neighbors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilarityConfig {
    /// Optional cap after deterministic `(distance, state, provenance)` sort.
    pub max_neighbors: Option<usize>,
    /// Optional upper bound on distance. Neighbors beyond it are ignored.
    pub max_distance: Option<f64>,
    /// Exponential distance scale: `exp(-distance / distance_scale)`.
    pub distance_scale: f64,
}

impl Default for SimilarityConfig {
    fn default() -> Self {
        Self {
            max_neighbors: None,
            max_distance: None,
            distance_scale: 1.0,
        }
    }
}

impl SimilarityConfig {
    fn validate(&self) -> Result<()> {
        if !self.distance_scale.is_finite() || self.distance_scale <= 0.0 {
            return Err(Error::InvalidConfig {
                message: "similarity distance_scale must be finite and > 0".to_string(),
            });
        }
        if let Some(max_distance) = self.max_distance
            && (!max_distance.is_finite() || max_distance < 0.0)
        {
            return Err(Error::InvalidConfig {
                message: "similarity max_distance must be finite and >= 0".to_string(),
            });
        }
        Ok(())
    }
}

/// Evidence showing which caller-supplied neighbor supported an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilarityEvidence {
    pub state: String,
    pub distance: f64,
    pub provenance: String,
    pub contribution_weight: f64,
}

/// An observed action aggregated from one or more similar states.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilarityCandidate {
    pub action: String,
    /// Normalized weighted mean of the source states' existing priors.
    pub prior: f64,
    /// Weighted mean of existing confidence values. This is a heuristic,
    /// not a new statistical confidence interval.
    pub confidence: f64,
    pub evidence: Vec<SimilarityEvidence>,
}

/// Result of an opt-in similarity lookup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilarityQueryResult {
    pub candidates: Vec<SimilarityCandidate>,
    pub neighbors_used: usize,
}

impl PriorBook {
    /// Aggregate candidates from caller-provided similar states.
    ///
    /// The exact-match APIs remain the safe default. This method is opt-in
    /// and only uses actions found in the supplied states; an empty neighbor
    /// list, an unseen neighbor, or a filtered set returns no candidates.
    /// Candidate ordering is deterministic and independent of input order.
    pub fn query_with_similarity(
        &self,
        neighbors: &[SimilarState],
        config: &SimilarityConfig,
        top_k: Option<usize>,
    ) -> Result<SimilarityQueryResult> {
        config.validate()?;
        let mut selected: Vec<&SimilarState> = neighbors
            .iter()
            .filter(|neighbor| {
                neighbor.distance.is_finite()
                    && neighbor.distance >= 0.0
                    && config
                        .max_distance
                        .is_none_or(|max| neighbor.distance <= max)
            })
            .collect();
        selected.sort_by(|left, right| {
            left.distance
                .total_cmp(&right.distance)
                .then_with(|| left.state.cmp(&right.state))
                .then_with(|| left.provenance.cmp(&right.provenance))
        });
        if let Some(max_neighbors) = config.max_neighbors {
            selected.truncate(max_neighbors);
        }

        let mut aggregates: BTreeMap<String, Aggregate> = BTreeMap::new();
        for neighbor in selected.iter().copied() {
            let weight = (-neighbor.distance / config.distance_scale).exp();
            for action in self.query(&neighbor.state, None) {
                let entry = aggregates.entry(action.action.clone()).or_default();
                entry.prior_sum += weight * action.prior;
                entry.confidence_sum += weight * action.confidence;
                entry.weight_sum += weight;
                entry.evidence.push(SimilarityEvidence {
                    state: neighbor.state.clone(),
                    distance: neighbor.distance,
                    provenance: neighbor.provenance.clone(),
                    contribution_weight: weight,
                });
            }
        }

        let mut candidates: Vec<SimilarityCandidate> = aggregates
            .into_iter()
            .map(|(action, aggregate)| SimilarityCandidate {
                action,
                prior: aggregate.prior_sum / aggregate.weight_sum,
                confidence: aggregate.confidence_sum / aggregate.weight_sum,
                evidence: aggregate.evidence,
            })
            .collect();
        let prior_sum: f64 = candidates.iter().map(|candidate| candidate.prior).sum();
        if prior_sum > 0.0 {
            for candidate in &mut candidates {
                candidate.prior /= prior_sum;
            }
        }
        candidates.sort_by(|left, right| {
            right
                .prior
                .total_cmp(&left.prior)
                .then_with(|| left.action.cmp(&right.action))
        });
        if let Some(top_k) = top_k {
            candidates.truncate(top_k);
        }

        Ok(SimilarityQueryResult {
            candidates,
            neighbors_used: selected.len(),
        })
    }
}

#[derive(Default)]
struct Aggregate {
    prior_sum: f64,
    confidence_sum: f64,
    weight_sum: f64,
    evidence: Vec<SimilarityEvidence>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PriorAction;
    use std::collections::HashMap;

    fn action(name: &str, prior: f64, confidence: f64) -> PriorAction {
        PriorAction {
            action: name.to_string(),
            count: 1,
            weighted_count: 1.0,
            success_rate: None,
            mean_score: None,
            prior,
            confidence,
        }
    }

    fn book() -> PriorBook {
        let mut entries = HashMap::new();
        entries.insert(
            "near-a".to_string(),
            vec![action("x", 0.8, 0.9), action("shared", 0.2, 0.4)],
        );
        entries.insert(
            "near-b".to_string(),
            vec![action("y", 0.7, 0.8), action("shared", 0.3, 0.5)],
        );
        PriorBook {
            entries,
            ..Default::default()
        }
    }

    #[test]
    fn only_observed_actions_are_returned_and_evidence_is_preserved() {
        let result = book()
            .query_with_similarity(
                &[SimilarState {
                    state: "near-a".to_string(),
                    distance: 0.0,
                    provenance: "fixture".to_string(),
                }],
                &SimilarityConfig::default(),
                None,
            )
            .unwrap();
        assert_eq!(result.neighbors_used, 1);
        assert_eq!(result.candidates[0].action, "x");
        assert_eq!(result.candidates[0].evidence[0].provenance, "fixture");
        assert!(
            result
                .candidates
                .iter()
                .all(|candidate| candidate.action != "invented")
        );
    }

    #[test]
    fn distance_weighting_and_normalization_are_deterministic() {
        let neighbors = vec![
            SimilarState {
                state: "near-b".to_string(),
                distance: 1.0,
                provenance: "b".to_string(),
            },
            SimilarState {
                state: "near-a".to_string(),
                distance: 0.0,
                provenance: "a".to_string(),
            },
        ];
        let first = book()
            .query_with_similarity(&neighbors, &SimilarityConfig::default(), None)
            .unwrap();
        let mut reversed = neighbors;
        reversed.reverse();
        let second = book()
            .query_with_similarity(&reversed, &SimilarityConfig::default(), None)
            .unwrap();
        assert_eq!(first, second);
        assert!((first.candidates.iter().map(|c| c.prior).sum::<f64>() - 1.0).abs() < 1e-12);
        assert_eq!(first.candidates[0].action, "x");
    }

    #[test]
    fn filters_sort_and_top_k_are_applied_before_aggregation() {
        let config = SimilarityConfig {
            max_neighbors: Some(1),
            max_distance: Some(0.5),
            distance_scale: 2.0,
        };
        let result = book()
            .query_with_similarity(
                &[
                    SimilarState {
                        state: "near-b".to_string(),
                        distance: 0.1,
                        provenance: "b".to_string(),
                    },
                    SimilarState {
                        state: "near-a".to_string(),
                        distance: 0.2,
                        provenance: "a".to_string(),
                    },
                    SimilarState {
                        state: "near-a".to_string(),
                        distance: 4.0,
                        provenance: "far".to_string(),
                    },
                ],
                &config,
                Some(1),
            )
            .unwrap();
        assert_eq!(result.neighbors_used, 1);
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].action, "y");
    }

    #[test]
    fn invalid_distance_data_is_ignored_but_invalid_config_is_rejected() {
        let result = book()
            .query_with_similarity(
                &[SimilarState {
                    state: "near-a".to_string(),
                    distance: f64::NAN,
                    provenance: "bad".to_string(),
                }],
                &SimilarityConfig::default(),
                None,
            )
            .unwrap();
        assert!(result.candidates.is_empty());
        let error = book().query_with_similarity(
            &[],
            &SimilarityConfig {
                distance_scale: 0.0,
                ..Default::default()
            },
            None,
        );
        assert!(matches!(error, Err(Error::InvalidConfig { .. })));
    }
}
