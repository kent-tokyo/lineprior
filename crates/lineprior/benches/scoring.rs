use criterion::{Criterion, criterion_group, criterion_main};
use lineprior::{
    BuildConfig, MacroActionConfig, Observation, Outcome, PriorAction, PriorBook,
    build_macro_actions, build_prior_book, build_prior_book_from_reader,
};
use std::collections::HashMap;
use std::hint::black_box;

/// `n_states` states, each with `actions_per_state` actions, each observed
/// `obs_per_action` times. Outcomes/scores vary slightly per observation so
/// smoothing and normalization do real work instead of hitting all-identical
/// fast paths.
fn synthetic_observations(
    n_states: usize,
    actions_per_state: usize,
    obs_per_action: usize,
) -> Vec<Observation> {
    let mut observations = Vec::with_capacity(n_states * actions_per_state * obs_per_action);
    let mut sequence = 0usize;
    for s in 0..n_states {
        let state = format!("state_{s:05}");
        for a in 0..actions_per_state {
            let action = format!("action_{a:03}");
            for i in 0..obs_per_action {
                sequence += 1;
                observations.push(Observation {
                    sequence_id: format!("seq_{sequence}"),
                    step: 0,
                    state: state.clone(),
                    action: action.clone(),
                    outcome: if i % 3 == 0 {
                        Outcome::Failure
                    } else {
                        Outcome::Success
                    },
                    score: Some(0.5 + (i % 10) as f64 * 0.01),
                    weight: 1.0,
                    tags: Vec::new(),
                    observed_at_unix_seconds: None,
                    source: None,
                });
            }
        }
    }
    observations
}

fn bench_build_small(c: &mut Criterion) {
    let observations = synthetic_observations(50, 5, 4); // 1,000 observations
    c.bench_function("build_small_1000obs", |b| {
        b.iter(|| build_prior_book(&observations, &BuildConfig::default()).unwrap())
    });
}

fn bench_build_medium(c: &mut Criterion) {
    let observations = synthetic_observations(200, 10, 5); // 10,000 observations
    c.bench_function("build_medium_10000obs", |b| {
        b.iter(|| build_prior_book(&observations, &BuildConfig::default()).unwrap())
    });
}

fn bench_build_large(c: &mut Criterion) {
    let observations = synthetic_observations(500, 20, 5); // 50,000 observations
    c.bench_function("build_large_50000obs", |b| {
        b.iter(|| build_prior_book(&observations, &BuildConfig::default()).unwrap())
    });
}

/// Same shape as `synthetic_observations`, rendered as JSONL text, so the
/// streaming entry point can be benchmarked against its own realistic
/// input rather than an already-parsed `Vec<Observation>`.
fn synthetic_jsonl(n_states: usize, actions_per_state: usize, obs_per_action: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut sequence = 0usize;
    for s in 0..n_states {
        for a in 0..actions_per_state {
            for i in 0..obs_per_action {
                sequence += 1;
                let outcome = if i % 3 == 0 { "failure" } else { "success" };
                let score = 0.5 + (i % 10) as f64 * 0.01;
                buf.extend_from_slice(
                    format!(
                        "{{\"sequence_id\":\"seq_{sequence}\",\"step\":0,\"state\":\"state_{s:05}\",\"action\":\"action_{a:03}\",\"outcome\":\"{outcome}\",\"score\":{score:.2},\"weight\":1.0}}\n"
                    )
                    .as_bytes(),
                );
            }
        }
    }
    buf
}

fn bench_build_from_reader_large(c: &mut Criterion) {
    let jsonl = synthetic_jsonl(500, 20, 5); // 50,000 observations
    c.bench_function("build_from_reader_large_50000obs", |b| {
        b.iter(|| {
            build_prior_book_from_reader(jsonl.as_slice(), false, &BuildConfig::default()).unwrap()
        })
    });
}

fn context_book(n_states: usize, context_order: usize) -> PriorBook {
    let mut book = PriorBook {
        entries: HashMap::new(),
        context_entries: HashMap::new(),
    };
    for state_index in 0..n_states {
        let state = format!("state_{state_index:05}");
        book.entries.insert(
            state.clone(),
            vec![PriorAction {
                action: "base".into(),
                count: 1,
                weighted_count: 1.0,
                success_rate: Some(0.5),
                mean_score: None,
                prior: 1.0,
                confidence: 0.1,
            }],
        );
        for order in 1..=context_order {
            let context = (0..order)
                .map(|index| format!("action_{index:03}"))
                .collect::<Vec<_>>();
            book.context_entries
                .insert((context, state.clone()), book.entries[&state].clone());
        }
    }
    book
}

fn bench_trie_materialize(c: &mut Criterion) {
    let book = context_book(500, 4);
    c.bench_function("trie_materialize_500_states_order4", |b| {
        b.iter(|| black_box(book.to_trie()))
    });
}

fn bench_flat_context_query(c: &mut Criterion) {
    let book = context_book(500, 4);
    let recent = (0..4)
        .map(|index| format!("action_{index:03}"))
        .collect::<Vec<_>>();
    c.bench_function("flat_context_query_order4", |b| {
        b.iter(|| black_box(book.query_with_context("state_00250", &recent, None)))
    });
}

fn bench_trie_context_query(c: &mut Criterion) {
    let trie = context_book(500, 4).to_trie();
    let recent = (0..4)
        .map(|index| format!("action_{index:03}"))
        .collect::<Vec<_>>();
    c.bench_function("trie_context_query_order4", |b| {
        b.iter(|| black_box(trie.query("state_00250", &recent, None)))
    });
}

fn bench_macro_action_extraction(c: &mut Criterion) {
    let mut observations = synthetic_observations(100, 5, 10);
    for (step, observation) in observations.iter_mut().enumerate() {
        observation.sequence_id = "macro-sequence".into();
        observation.step = step as u32;
    }
    c.bench_function("macro_actions_5000obs_window2to4", |b| {
        b.iter(|| {
            black_box(build_macro_actions(
                &observations,
                MacroActionConfig {
                    min_length: 2,
                    max_length: 4,
                    min_count: 2,
                },
            ))
        })
    });
}

criterion_group!(
    benches,
    bench_build_small,
    bench_build_medium,
    bench_build_large,
    bench_build_from_reader_large,
    bench_trie_materialize,
    bench_flat_context_query,
    bench_trie_context_query,
    bench_macro_action_extraction
);
criterion_main!(benches);
