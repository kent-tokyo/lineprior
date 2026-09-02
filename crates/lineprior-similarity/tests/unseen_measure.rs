use lineprior::{BuildConfig, SimilarityConfig, build_prior_book, parse_jsonl};
use lineprior_similarity::{FeatureVectorState, NearestNeighborConfig, nearest_neighbors};

const TRAIN: &str = include_str!("fixtures/unseen_states.jsonl");

#[test]
fn unseen_state_measurement_keeps_exact_and_no_prior_as_abstentions() {
    let parsed = parse_jsonl(TRAIN.as_bytes(), true).unwrap();
    let book = build_prior_book(&parsed.observations, &BuildConfig::default()).unwrap();
    let query_state = "screen-cart-unseen";
    let expected_action = "click:add-to-cart";

    let exact = book.query(query_state, Some(1));
    assert!(exact.is_empty());
    let exact_coverage = !exact.is_empty();

    let no_prior_top1_hit = exact
        .first()
        .is_some_and(|item| item.action == expected_action);
    assert!(!no_prior_top1_hit);

    let neighbors = nearest_neighbors(
        &[0.05, 0.05],
        &[
            FeatureVectorState {
                state: "screen-cart".to_string(),
                features: vec![0.0, 0.0],
                provenance: "fixture-v1".to_string(),
            },
            FeatureVectorState {
                state: "screen-checkout".to_string(),
                features: vec![1.0, 1.0],
                provenance: "fixture-v1".to_string(),
            },
        ],
        NearestNeighborConfig {
            max_neighbors: Some(1),
            max_distance: Some(0.5),
        },
    )
    .unwrap();
    let similarity = book
        .query_with_similarity(&neighbors, &SimilarityConfig::default(), Some(1))
        .unwrap();

    let similarity_top1_hit = similarity
        .candidates
        .first()
        .is_some_and(|item| item.action == expected_action);
    assert!(similarity_top1_hit);
    assert_eq!(similarity.neighbors_used, 1);

    // This is a deterministic boundary fixture, not a real-data quality claim:
    // it proves the measurement harness distinguishes abstention from opt-in
    // similarity recovery on an unseen state.
    let coverage = |has_candidate: bool| usize::from(has_candidate);
    assert_eq!(coverage(exact_coverage), 0);
    assert_eq!(coverage(!similarity.candidates.is_empty()), 1);
    assert_eq!(coverage(exact_coverage), 0);
}
