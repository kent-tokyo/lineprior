use lineprior::{PriorAction, PriorBook, SimilarState, SimilarityConfig};
use std::collections::HashMap;

#[test]
fn public_similarity_api_only_returns_actions_from_supplied_states() {
    let mut entries = HashMap::new();
    entries.insert(
        "neighbor".to_string(),
        vec![PriorAction {
            action: "known-action".to_string(),
            count: 3,
            weighted_count: 3.0,
            success_rate: Some(1.0),
            mean_score: None,
            prior: 1.0,
            confidence: 0.75,
        }],
    );
    let book = PriorBook {
        entries,
        ..Default::default()
    };
    let result = book
        .query_with_similarity(
            &[SimilarState {
                state: "neighbor".to_string(),
                distance: 0.2,
                provenance: "test-index".to_string(),
            }],
            &SimilarityConfig::default(),
            None,
        )
        .unwrap();
    assert_eq!(result.neighbors_used, 1);
    assert_eq!(result.candidates[0].action, "known-action");
    assert_eq!(result.candidates[0].evidence[0].provenance, "test-index");
}
