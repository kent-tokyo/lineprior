use lineprior::{
    BuildConfig, IncrementalPriorBuilder, Observation, Outcome, build_prior_book,
    build_prior_book_from_reader,
};

fn observation(
    sequence_id: &str,
    step: u32,
    state: &str,
    action: &str,
    outcome: Outcome,
) -> Observation {
    Observation {
        sequence_id: sequence_id.to_owned(),
        step,
        state: state.to_owned(),
        action: action.to_owned(),
        outcome,
        score: None,
        weight: 1.0,
        tags: Vec::new(),
        observed_at_unix_seconds: None,
        source: None,
    }
}

#[test]
fn incremental_builder_matches_eager_and_jsonl_streaming_construction() {
    let observations = vec![
        observation("game-1", 0, "s0", "a", Outcome::Success),
        observation("game-1", 1, "s1", "b", Outcome::Success),
        observation("game-2", 0, "s0", "a", Outcome::Failure),
        observation("game-2", 1, "s1", "c", Outcome::Success),
    ];
    let config = BuildConfig {
        context_order: 1,
        terminal_credit_weight: 0.25,
        ..BuildConfig::default()
    };

    let eager = build_prior_book(&observations, &config).unwrap();

    let mut incremental = IncrementalPriorBuilder::new(config.clone()).unwrap();
    for observation in observations.clone() {
        incremental.observe(observation).unwrap();
    }
    let incremental = incremental.finish();

    let jsonl = observations
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    let streaming = build_prior_book_from_reader(jsonl.as_bytes(), true, &config).unwrap();

    assert_eq!(eager.entries_sorted(), incremental.book.entries_sorted());
    assert_eq!(
        eager.context_entries_sorted(),
        incremental.book.context_entries_sorted()
    );
    assert_eq!(
        streaming.book.entries_sorted(),
        incremental.book.entries_sorted()
    );
    assert_eq!(
        streaming.book.context_entries_sorted(),
        incremental.book.context_entries_sorted()
    );
    assert_eq!(streaming.stats, incremental.stats);
    assert!(incremental.warnings.is_empty());
}

#[test]
fn incremental_builder_enforces_context_sequence_ordering() {
    let config = BuildConfig {
        context_order: 1,
        ..BuildConfig::default()
    };
    let mut builder = IncrementalPriorBuilder::new(config).unwrap();
    builder
        .observe(observation("game-1", 1, "s1", "b", Outcome::Success))
        .unwrap();

    let error = builder
        .observe(observation("game-1", 1, "s2", "c", Outcome::Success))
        .unwrap_err();
    assert!(matches!(error, lineprior::Error::SequenceNotSorted { .. }));
}
