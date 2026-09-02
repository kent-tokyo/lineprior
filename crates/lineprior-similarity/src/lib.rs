//! A small deterministic feature-vector adapter for `lineprior`.
//!
//! This crate owns only nearest-neighbor retrieval. It does not embed domain
//! states, store a vector index, or choose actions. The returned
//! [`lineprior::SimilarState`] values can be passed to
//! [`lineprior::PriorBook::query_with_similarity`].

use lineprior::SimilarState;
use std::fmt;

/// A state and its caller-provided numeric representation.
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureVectorState {
    pub state: String,
    pub features: Vec<f64>,
    pub provenance: String,
}

/// Limits for the deterministic nearest-neighbor scan.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct NearestNeighborConfig {
    pub max_neighbors: Option<usize>,
    pub max_distance: Option<f64>,
}

/// Errors from malformed feature-vector input.
#[derive(Debug, Clone, PartialEq)]
pub enum FeatureVectorError {
    EmptyQuery,
    EmptyCandidate {
        state: String,
    },
    DimensionMismatch {
        state: String,
        expected: usize,
        found: usize,
    },
    NonFiniteValue {
        state: String,
        index: usize,
    },
    InvalidMaxDistance,
}

impl fmt::Display for FeatureVectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyQuery => write!(f, "query feature vector must not be empty"),
            Self::EmptyCandidate { state } => {
                write!(f, "candidate state `{state}` has an empty feature vector")
            }
            Self::DimensionMismatch {
                state,
                expected,
                found,
            } => write!(
                f,
                "candidate state `{state}` has {found} features, expected {expected}"
            ),
            Self::NonFiniteValue { state, index } => {
                write!(
                    f,
                    "state `{state}` has a non-finite feature at index {index}"
                )
            }
            Self::InvalidMaxDistance => write!(f, "max_distance must be finite and >= 0"),
        }
    }
}

impl std::error::Error for FeatureVectorError {}

/// Finds the nearest caller-supplied states using Euclidean distance.
///
/// Results are sorted by ascending distance, then state, then provenance, so
/// input ordering cannot change which neighbors are selected. The operation
/// is intentionally a linear scan: callers with a large corpus can replace
/// this adapter with an indexed provider without changing the lineprior API.
pub fn nearest_neighbors(
    query: &[f64],
    candidates: &[FeatureVectorState],
    config: NearestNeighborConfig,
) -> Result<Vec<SimilarState>, FeatureVectorError> {
    validate_vector("<query>", query, None)?;
    if let Some(max_distance) = config.max_distance
        && (!max_distance.is_finite() || max_distance < 0.0)
    {
        return Err(FeatureVectorError::InvalidMaxDistance);
    }

    let mut ranked = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        validate_vector(&candidate.state, &candidate.features, Some(query.len()))?;
        let distance = query
            .iter()
            .zip(&candidate.features)
            .map(|(left, right)| (left - right).powi(2))
            .sum::<f64>()
            .sqrt();
        if config.max_distance.is_none_or(|max| distance <= max) {
            ranked.push((distance, candidate));
        }
    }
    ranked.sort_by(|(left_distance, left), (right_distance, right)| {
        left_distance
            .total_cmp(right_distance)
            .then_with(|| left.state.cmp(&right.state))
            .then_with(|| left.provenance.cmp(&right.provenance))
    });
    if let Some(max_neighbors) = config.max_neighbors {
        ranked.truncate(max_neighbors);
    }
    Ok(ranked
        .into_iter()
        .map(|(distance, candidate)| SimilarState {
            state: candidate.state.clone(),
            distance,
            provenance: candidate.provenance.clone(),
        })
        .collect())
}

fn validate_vector(
    state: &str,
    features: &[f64],
    expected_dimension: Option<usize>,
) -> Result<(), FeatureVectorError> {
    if features.is_empty() {
        return Err(if expected_dimension.is_none() {
            FeatureVectorError::EmptyQuery
        } else {
            FeatureVectorError::EmptyCandidate {
                state: state.to_string(),
            }
        });
    }
    if let Some(expected) = expected_dimension
        && features.len() != expected
    {
        return Err(FeatureVectorError::DimensionMismatch {
            state: state.to_string(),
            expected,
            found: features.len(),
        });
    }
    for (index, value) in features.iter().enumerate() {
        if !value.is_finite() {
            return Err(FeatureVectorError::NonFiniteValue {
                state: state.to_string(),
                index,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lineprior::{PriorAction, PriorBook};

    fn candidate(state: &str, features: &[f64], provenance: &str) -> FeatureVectorState {
        FeatureVectorState {
            state: state.to_string(),
            features: features.to_vec(),
            provenance: provenance.to_string(),
        }
    }

    #[test]
    fn returns_euclidean_neighbors_in_deterministic_order() {
        let candidates = vec![
            candidate("b", &[2.0, 0.0], "index"),
            candidate("a", &[0.0, 1.0], "index"),
            candidate("near", &[0.0, 0.2], "index"),
        ];
        let result = nearest_neighbors(&[0.0, 0.0], &candidates, Default::default()).unwrap();
        assert_eq!(
            result
                .iter()
                .map(|neighbor| neighbor.state.as_str())
                .collect::<Vec<_>>(),
            vec!["near", "a", "b"]
        );
        assert!((result[0].distance - 0.2).abs() < 1e-12);
    }

    #[test]
    fn max_distance_and_neighbor_count_are_applied() {
        let candidates = vec![
            candidate("near", &[0.1], "index"),
            candidate("middle", &[0.2], "index"),
            candidate("far", &[0.3], "index"),
        ];
        let result = nearest_neighbors(
            &[0.0],
            &candidates,
            NearestNeighborConfig {
                max_neighbors: Some(1),
                max_distance: Some(0.25),
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].state, "near");
    }

    #[test]
    fn input_order_does_not_change_ties() {
        let first = vec![candidate("b", &[1.0], "z"), candidate("a", &[1.0], "z")];
        let second = vec![candidate("a", &[1.0], "z"), candidate("b", &[1.0], "z")];
        assert_eq!(
            nearest_neighbors(&[0.0], &first, Default::default()).unwrap(),
            nearest_neighbors(&[0.0], &second, Default::default()).unwrap()
        );
    }

    #[test]
    fn malformed_vectors_are_rejected_without_panicking() {
        assert_eq!(
            nearest_neighbors(&[], &[], Default::default()),
            Err(FeatureVectorError::EmptyQuery)
        );
        assert!(matches!(
            nearest_neighbors(
                &[0.0],
                &[candidate("bad", &[0.0, 1.0], "index")],
                Default::default()
            ),
            Err(FeatureVectorError::DimensionMismatch { .. })
        ));
        assert!(matches!(
            nearest_neighbors(&[f64::NAN], &[], Default::default()),
            Err(FeatureVectorError::NonFiniteValue { .. })
        ));
    }

    #[test]
    fn adapter_output_composes_with_lineprior_without_inventing_actions() {
        let mut book = PriorBook::default();
        book.entries.insert(
            "known".to_string(),
            vec![PriorAction {
                action: "observed".to_string(),
                count: 2,
                weighted_count: 2.0,
                success_rate: None,
                mean_score: None,
                prior: 1.0,
                confidence: 0.5,
            }],
        );
        let neighbors = nearest_neighbors(
            &[0.0],
            &[candidate("known", &[0.1], "fixture")],
            Default::default(),
        )
        .unwrap();
        let result = book
            .query_with_similarity(&neighbors, &Default::default(), None)
            .unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].action, "observed");
    }
}
