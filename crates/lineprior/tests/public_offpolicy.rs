use lineprior::{
    BootstrapConfig, OffPolicyConfig, OffPolicyObservation, bootstrap_self_normalized_ips,
    evaluate_doubly_robust, evaluate_self_normalized_ips,
};
use std::collections::BTreeMap;

fn row(reward: f64, logging_propensity: f64, evaluation_probability: f64) -> OffPolicyObservation {
    OffPolicyObservation {
        state: "state".to_string(),
        action: "action".to_string(),
        reward,
        logging_propensity,
        evaluation_probability,
        reward_model_policy_value: None,
        reward_model_logged_action: None,
        metadata: BTreeMap::new(),
    }
}

#[test]
fn public_offpolicy_api_round_trips_ips_and_bootstrap() {
    let observations = vec![row(1.0, 0.5, 0.5), row(0.0, 0.5, 0.5)];
    let report = evaluate_self_normalized_ips(&observations, &OffPolicyConfig::default()).unwrap();
    assert_eq!(report.supported_observations, 2);
    assert_eq!(report.ips, Some(0.5));

    let bootstrap = bootstrap_self_normalized_ips(
        &observations,
        &OffPolicyConfig::default(),
        BootstrapConfig {
            resamples: 32,
            seed: 7,
            confidence_level: 0.9,
        },
    )
    .unwrap();
    let repeated_bootstrap = bootstrap_self_normalized_ips(
        &observations,
        &OffPolicyConfig::default(),
        BootstrapConfig {
            resamples: 32,
            seed: 7,
            confidence_level: 0.9,
        },
    )
    .unwrap();
    assert_eq!(bootstrap.seed, 7);
    assert_eq!(bootstrap, repeated_bootstrap);
    assert_eq!(
        bootstrap.self_normalized_ips.unwrap().successful_resamples,
        32
    );
}

#[test]
fn public_doubly_robust_api_requires_explicit_model_values() {
    let mut observation = row(1.0, 1.0, 1.0);
    observation.reward_model_policy_value = Some(0.25);
    observation.reward_model_logged_action = Some(0.25);
    let report = evaluate_doubly_robust(&[observation], &OffPolicyConfig::default()).unwrap();
    assert_eq!(report.estimate, Some(1.0));
}

#[test]
fn checked_in_ope_fixture_has_replayable_estimates() {
    let observations: Vec<OffPolicyObservation> = include_str!("../../../examples/offpolicy.jsonl")
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()
        .unwrap();

    let ips = evaluate_self_normalized_ips(&observations, &OffPolicyConfig::default()).unwrap();
    assert_eq!(ips.supported_observations, 2);
    assert_eq!(ips.self_normalized_ips, Some(0.5));
    assert_eq!(observations[0].metadata.get("split").unwrap(), "train");

    let doubly_robust = evaluate_doubly_robust(&observations, &OffPolicyConfig::default()).unwrap();
    assert!((doubly_robust.estimate.unwrap() - 0.85).abs() < 1e-12);
}
