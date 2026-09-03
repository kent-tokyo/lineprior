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
            "lineprior_version": "0.11.1",
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
        "lineprior_version": "0.11.1",
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
                "lineprior_version": "0.11.1",
                "input_sha256": {"off": "0".repeat(64), "on": "1".repeat(64)}
            },
            "paired_rows": 2,
            "off": {"supported_rows": 2},
            "on": {"supported_rows": 2}
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
