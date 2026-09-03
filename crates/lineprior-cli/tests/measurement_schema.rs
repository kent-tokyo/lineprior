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
