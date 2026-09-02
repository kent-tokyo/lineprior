//! Regression guard for issue #1: `GateStatus`/`PredictionStatus` must be
//! nameable and matchable from the crate root, not just reachable as a
//! struct field's type. An in-`src/` unit test wouldn't catch a missing
//! `pub use` (it can reach `gate::GateStatus` directly regardless), so this
//! lives as an integration test that only ever spells `lineprior::`.
//!
//! Also covers `ContextQueryResult`, found via the same failure mode while
//! fixing broken rustdoc intra-doc links: `PriorBook::query_with_context`
//! returns it, but it wasn't re-exported either.

use lineprior::{
    ContextQueryResult, GateAcquisitionConfig, GateAcquisitionQuery, GateModel, GateModelConfig,
    GateObservation, GateQuery, GateStatus, GateVerdictConfig, MonotonicDirection,
    PredictionStatus, PriorBook,
};
use std::collections::BTreeMap;

#[test]
fn gate_status_is_nameable_and_matchable_from_the_crate_root() {
    let status = GateStatus::Pass;
    let label = match status {
        GateStatus::Pass => "pass",
        GateStatus::Fail => "fail",
        GateStatus::Inconclusive => "inconclusive",
    };
    assert_eq!(label, "pass");
}

#[test]
fn verdict_and_acquisition_apis_are_public_and_deterministic() {
    let observations = (0..6)
        .map(|index| {
            let mut features = BTreeMap::new();
            features.insert("signal".to_string(), index as f64);
            GateObservation {
                candidate_id: format!("candidate-{index}"),
                group_id: if index < 3 { "group-a" } else { "group-b" }.to_string(),
                features,
                gate_elo_delta: index as f64 * 2.0 - 3.0,
                gate_games_played: 100.0,
                actual_elo_stddev: None,
                elo_ci_low: None,
                elo_ci_high: None,
                completed_pairs: None,
                gate_status: None,
                provenance: BTreeMap::new(),
            }
        })
        .collect::<Vec<_>>();
    let model = GateModel::fit(&observations, &GateModelConfig::default())
        .unwrap()
        .model;
    let mut features = BTreeMap::new();
    features.insert("signal".to_string(), 4.0);
    let query = GateQuery { features };

    let verdict = model
        .predict_verdict(&query, &GateVerdictConfig::default())
        .unwrap();
    let probability_sum =
        verdict.pass_probability + verdict.fail_probability + verdict.inconclusive_probability;
    assert!((probability_sum - 1.0).abs() < 1e-9);

    let acquisition = model
        .acquire(
            &GateAcquisitionQuery {
                query,
                expected_gate_cost: 10.0,
            },
            &GateAcquisitionConfig::default(),
        )
        .unwrap();
    assert!(acquisition.expected_improvement >= 0.0);
    assert!(acquisition.acquisition_score >= 0.0);
}

#[test]
fn monotonic_constraint_is_public_and_enforces_prediction_direction() {
    let observations = (0..6)
        .map(|index| {
            let mut features = BTreeMap::new();
            features.insert("signal".to_string(), index as f64);
            GateObservation {
                candidate_id: format!("candidate-{index}"),
                group_id: if index < 3 { "group-a" } else { "group-b" }.to_string(),
                features,
                gate_elo_delta: index as f64 * 2.0 - 3.0,
                gate_games_played: 100.0,
                actual_elo_stddev: None,
                elo_ci_low: None,
                elo_ci_high: None,
                completed_pairs: None,
                gate_status: None,
                provenance: BTreeMap::new(),
            }
        })
        .collect::<Vec<_>>();
    let mut config = GateModelConfig::default();
    config
        .monotonic_constraints
        .insert("signal".to_string(), MonotonicDirection::Decreasing);
    let model = GateModel::fit(&observations, &config).unwrap().model;
    assert_eq!(
        model.monotonic_constraints().get("signal"),
        Some(&MonotonicDirection::Decreasing)
    );

    let mut low_features = BTreeMap::new();
    low_features.insert("signal".to_string(), 0.0);
    let mut high_features = BTreeMap::new();
    high_features.insert("signal".to_string(), 5.0);
    let low = model.predict(&GateQuery {
        features: low_features,
    });
    let high = model.predict(&GateQuery {
        features: high_features,
    });
    assert!(high.expected_elo <= low.expected_elo + 1e-9);
}

#[test]
fn prediction_status_is_nameable_and_matchable_from_the_crate_root() {
    let status = PredictionStatus::Supported;
    let label = match status {
        PredictionStatus::Supported => "supported",
        PredictionStatus::Extrapolation => "extrapolation",
        PredictionStatus::Unsupported => "unsupported",
    };
    assert_eq!(label, "supported");
}

#[test]
fn context_query_result_is_nameable_from_the_crate_root() {
    let book = PriorBook::default();
    let result: ContextQueryResult = book.query_with_context("state", &[], None);
    assert_eq!(result.matched_order, 0);
    assert!(result.candidates.is_empty());
}
