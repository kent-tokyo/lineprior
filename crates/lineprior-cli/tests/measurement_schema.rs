use jsonschema::validator_for;
use serde_json::{Value, json};
use std::fs;

fn schema(name: &str) -> Value {
    let path = format!("../../docs/measurements/{name}");
    serde_json::from_str(&fs::read_to_string(path).expect("schema should be readable"))
        .expect("schema should be valid JSON")
}

fn similarity_artifact() -> Value {
    let arm = json!({
        "coverage": 0.5,
        "abstention_rate": 0.5,
        "top1_hit_rate": 0.5,
        "mrr": 0.5,
        "calibration_brier": 0.25
    });
    json!({
        "protocol": "similarity-real-data-v1",
        "num_queries": 2,
        "measurement": {
            "dataset_id": "fixture-v1",
            "split": "heldout",
            "feature_version": "fixture-v1",
            "lineprior_version": "0.12.0",
            "prior_config_fingerprint": 123,
            "input_sha256": {"prior": "0".repeat(64), "queries": "1".repeat(64)}
        },
        "arms": {"exact": arm, "similarity": arm, "no_prior": arm}
    })
}

fn offpolicy_artifact() -> Value {
    let arm = json!({
        "ips": {
            "ips": 0.42,
            "self_normalized_ips": 0.4,
            "support_fraction": 1.0,
            "effective_sample_size": 2.0
        },
        "doubly_robust": {"estimate": 0.39},
        "bootstrap": {
            "seed": 42,
            "resamples": 16,
            "confidence_level": 0.95,
            "lower": 0.1,
            "upper": 0.7
        }
    });
    let measurement = json!({
        "dataset_id": "fixture-v1",
        "split": "heldout",
        "lineprior_version": "0.12.0",
        "policy_version": "policy-v1",
        "input_sha256": {"off": "0".repeat(64), "on": "1".repeat(64)}
    });
    json!({
        "protocol": "offpolicy-integrated-arms-v1",
        "measurement": measurement,
        "arms": {"off": arm, "on": arm},
        "paired": {
            "protocol": "offpolicy-paired-arms-v1",
            "measurement": {
                "dataset_id": "fixture-v1",
                "split": "heldout",
                "lineprior_version": "0.12.0",
                "input_sha256": {"off": "0".repeat(64), "on": "1".repeat(64)}
            },
            "paired_rows": 2,
            "off": {"supported_rows": 2},
            "on": {"supported_rows": 2}
        }
    })
}

fn offpolicy_observation() -> Value {
    json!({
        "state": "state-a",
        "action": "action-a",
        "reward": 1.0,
        "logging_propensity": 0.5,
        "evaluation_probability": 0.25,
        "metadata": {"decision_id": "d-1"}
    })
}

fn gate_observation() -> Value {
    json!({
        "candidate_id": "candidate-a",
        "group_id": "group-a",
        "features": {"validation_loss": 0.25},
        "gate_elo_delta": 12.0,
        "gate_games_played": 100.0,
        "gate_status": "Pass",
        "provenance": {"manifest_sha256": "0".repeat(64)}
    })
}

fn prior_artifact_manifest() -> Value {
    json!({
        "schema": "lineprior.prior-artifact-manifest.v1",
        "producer_version": "0.12.2",
        "input_sha256": "0".repeat(64),
        "prior_book_sha256": "1".repeat(64),
        "build_config": {},
        "build_config_fingerprint": 123,
        "lineage": {
            "dataset_id": "fixture-v1",
            "split": "train",
            "adapter_id": "example",
            "adapter_version": "v1",
            "metadata": {"downstream_commit": "abc"}
        },
        "summary": {
            "warning_count": 0,
            "order_zero_states_kept": 1,
            "context_entries_kept": 0,
            "build_stats": {
                "observations_kept": 1,
                "observations_dropped_by_step_or_tag_filter": 0,
                "observations_dropped_by_missing_timestamp": 0,
                "candidates_before_filtering": 1,
                "candidates_dropped_by_min_count": 0,
                "candidates_dropped_by_min_weighted_count": 0,
                "candidates_dropped_by_min_confidence": 0,
                "candidates_dropped_by_max_actions_per_state": 0,
                "candidates_kept": 1
            }
        }
    })
}

#[test]
fn similarity_schema_accepts_a_representative_artifact() {
    let validator = validator_for(&schema("similarity-real-data-v1.schema.json")).unwrap();
    assert!(validator.is_valid(&similarity_artifact()));
}

#[test]
fn similarity_schema_rejects_protocol_drift_and_missing_required_fields() {
    let validator = validator_for(&schema("similarity-real-data-v1.schema.json")).unwrap();
    let mut wrong_protocol = similarity_artifact();
    wrong_protocol["protocol"] = json!("similarity-real-data-v0");
    assert!(!validator.is_valid(&wrong_protocol));

    let mut missing_measurement = similarity_artifact();
    missing_measurement
        .as_object_mut()
        .unwrap()
        .remove("measurement");
    assert!(!validator.is_valid(&missing_measurement));
}

#[test]
fn offpolicy_schema_accepts_a_representative_integrated_artifact() {
    let validator = validator_for(&schema("offpolicy-integrated-arms-v1.schema.json")).unwrap();
    assert!(validator.is_valid(&offpolicy_artifact()));
}

#[test]
fn offpolicy_schema_rejects_protocol_drift_and_missing_paired_report() {
    let validator = validator_for(&schema("offpolicy-integrated-arms-v1.schema.json")).unwrap();
    let mut wrong_protocol = offpolicy_artifact();
    wrong_protocol["paired"]["protocol"] = json!("offpolicy-paired-arms-v0");
    assert!(!validator.is_valid(&wrong_protocol));

    let mut missing_paired = offpolicy_artifact();
    missing_paired.as_object_mut().unwrap().remove("paired");
    assert!(!validator.is_valid(&missing_paired));
}

#[test]
fn offpolicy_schema_leaves_cross_artifact_semantics_to_the_semantic_validator() {
    let validator = validator_for(&schema("offpolicy-integrated-arms-v1.schema.json")).unwrap();

    // JSON Schema can validate each nested object, but it does not express the
    // equality between the integrated and paired lineage records.
    let mut mismatched_lineage = offpolicy_artifact();
    mismatched_lineage["paired"]["measurement"]["dataset_id"] = json!("other-fixture");
    assert!(validator.is_valid(&mismatched_lineage));

    // The semantic validator owns metric ranges that are intentionally not
    // duplicated in this structural schema.
    let mut invalid_support_fraction = offpolicy_artifact();
    invalid_support_fraction["arms"]["off"]["ips"]["support_fraction"] = json!(1.5);
    assert!(validator.is_valid(&invalid_support_fraction));
}

#[test]
fn raw_observation_schemas_accept_representative_rows() {
    let offpolicy = validator_for(&schema("offpolicy-observation-v1.schema.json")).unwrap();
    assert!(offpolicy.is_valid(&offpolicy_observation()));

    let gate = validator_for(&schema("gate-observation-v1.schema.json")).unwrap();
    assert!(gate.is_valid(&gate_observation()));
}

#[test]
fn raw_observation_schemas_reject_missing_identity_and_bad_probability() {
    let offpolicy = validator_for(&schema("offpolicy-observation-v1.schema.json")).unwrap();
    let mut bad_probability = offpolicy_observation();
    bad_probability["logging_propensity"] = json!(1.1);
    assert!(!offpolicy.is_valid(&bad_probability));

    let gate = validator_for(&schema("gate-observation-v1.schema.json")).unwrap();
    let mut missing_group = gate_observation();
    missing_group.as_object_mut().unwrap().remove("group_id");
    assert!(!gate.is_valid(&missing_group));
}

#[test]
fn prior_artifact_manifest_schema_validates_hashes_and_counts() {
    let validator = validator_for(&schema("prior-artifact-manifest-v1.schema.json")).unwrap();
    assert!(validator.is_valid(&prior_artifact_manifest()));

    let mut bad_hash = prior_artifact_manifest();
    bad_hash["input_sha256"] = json!("not-a-digest");
    assert!(!validator.is_valid(&bad_hash));
}
