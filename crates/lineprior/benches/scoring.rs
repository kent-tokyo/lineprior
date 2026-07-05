use criterion::{Criterion, criterion_group, criterion_main};
use lineprior::{BuildConfig, Observation, Outcome, build_prior_book};

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

criterion_group!(
    benches,
    bench_build_small,
    bench_build_medium,
    bench_build_large
);
criterion_main!(benches);
