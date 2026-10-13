use jsonschema::validator_for;
use lineprior::{GateObservation, validate_gate_observations};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

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
        "producer_version": "0.12.4",
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

fn candidate_ordering_artifact() -> Value {
    serde_json::from_str(
        &fs::read_to_string(fixture("candidate_ordering_not_ready.json"))
            .expect("candidate-ordering fixture should be readable"),
    )
    .expect("candidate-ordering fixture should be valid JSON")
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
fn candidate_ordering_schema_accepts_a_not_ready_artifact() {
    let validator = validator_for(&schema("candidate-ordering-evaluation-v1.schema.json")).unwrap();
    assert!(validator.is_valid(&candidate_ordering_artifact()));
}

#[test]
fn candidate_ordering_schema_accepts_a_completed_paired_artifact() {
    let validator = validator_for(&schema("candidate-ordering-evaluation-v1.schema.json")).unwrap();
    let mut artifact = candidate_ordering_artifact();
    artifact["measurement"]["input_sha256"]["off_results"] = json!("3".repeat(64));
    artifact["measurement"]["input_sha256"]["on_results"] = json!("4".repeat(64));
    let usage = json!({
        "eligible_decisions": 10,
        "prior_selections": 2,
        "prior_abstentions": 8,
        "unavailable_fallbacks": 0,
        "selection_rate": 0.2,
        "fallback_reasons": {"unseen_state": 8},
        "passed_coverage_gate": true
    });
    artifact["preflight"] = usage.clone();
    artifact["arms"] = json!({
        "off": {
            "cases": 10,
            "quality_estimate": 1.0,
            "quality_interval": {"lower": 0.9, "upper": 1.1},
            "cost_estimate": 100.0,
            "cost_interval": {"lower": 90.0, "upper": 110.0}
        },
        "on": {
            "cases": 10,
            "quality_estimate": 1.0,
            "quality_interval": {"lower": 0.9, "upper": 1.1},
            "cost_estimate": 90.0,
            "cost_interval": {"lower": 80.0, "upper": 100.0},
            "usage": usage
        }
    });
    artifact["decision"] = json!({
        "status": "PASS",
        "quality_noninferiority_met": true,
        "cost_improvement_met": true,
        "rationale": "both preregistered gates passed"
    });

    assert!(validator.is_valid(&artifact));
}

#[test]
fn candidate_ordering_schema_rejects_protocol_drift_and_missing_preregistration() {
    let validator = validator_for(&schema("candidate-ordering-evaluation-v1.schema.json")).unwrap();
    let mut wrong_protocol = candidate_ordering_artifact();
    wrong_protocol["protocol"] = json!("candidate-ordering-evaluation-v0");
    assert!(!validator.is_valid(&wrong_protocol));

    let mut missing_preregistration = candidate_ordering_artifact();
    missing_preregistration
        .as_object_mut()
        .unwrap()
        .remove("preregistration");
    assert!(!validator.is_valid(&missing_preregistration));
}

#[test]
fn candidate_ordering_schema_leaves_count_semantics_to_the_semantic_validator() {
    let validator = validator_for(&schema("candidate-ordering-evaluation-v1.schema.json")).unwrap();
    let mut inconsistent = candidate_ordering_artifact();
    inconsistent["preflight"]["selection_rate"] = json!(0.5);
    assert!(validator.is_valid(&inconsistent));
}

#[test]
fn raw_observation_schemas_accept_representative_rows() {
    let offpolicy = validator_for(&schema("offpolicy-observation-v1.schema.json")).unwrap();
    assert!(offpolicy.is_valid(&offpolicy_observation()));

    let gate = validator_for(&schema("gate-observation-v1.schema.json")).unwrap();
    assert!(gate.is_valid(&gate_observation()));
}

#[test]
fn sekirei_v0_3_68_gate_fixture_matches_schema_and_semantics() {
    // This mirrors the fixture shipped at Sekirei v0.3.68. Keeping the
    // downstream shape here catches contract drift before real gate history
    // reaches model fitting.
    let bytes = fs::read(fixture("sekirei_v0_3_68_gate_observation.jsonl"))
        .expect("Sekirei fixture should be readable");
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "64cbdd9f9c7d1ba76e7aab8a1155756c6030d5d232cd8e931e42f1d5f606075c"
    );
    let input = String::from_utf8(bytes).expect("Sekirei fixture should be UTF-8");
    let validator = validator_for(&schema("gate-observation-v1.schema.json")).unwrap();
    let observations = input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value =
                serde_json::from_str(line).expect("fixture row should be valid JSON");
            assert!(validator.is_valid(&value));
            serde_json::from_value::<GateObservation>(value)
                .expect("fixture row should deserialize as GateObservation")
        })
        .collect::<Vec<_>>();

    assert_eq!(observations.len(), 1);
    validate_gate_observations(&observations)
        .expect("Sekirei fixture should pass collection-time semantics");
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
