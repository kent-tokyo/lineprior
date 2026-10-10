use assert_cmd::Command;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

// Fixtures live at the workspace root so lib and CLI tests can share them.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

#[test]
fn offpolicy_command_writes_ips_dr_and_bootstrap_report() {
    let input = temp_path("offpolicy_input.jsonl");
    let output = temp_path("offpolicy_report.json");
    std::fs::write(
        &input,
        concat!(
            "{\"state\":\"s\",\"action\":\"a\",\"reward\":1.0,\"logging_propensity\":0.5,\"evaluation_probability\":0.5,\"reward_model_policy_value\":0.4,\"reward_model_logged_action\":0.4}\n",
            "{\"state\":\"s\",\"action\":\"b\",\"reward\":0.0,\"logging_propensity\":0.5,\"evaluation_probability\":0.5,\"reward_model_policy_value\":0.4,\"reward_model_logged_action\":0.4}\n",
        ),
    )
    .unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "offpolicy",
            input.to_str().unwrap(),
            "--out",
            output.to_str().unwrap(),
            "--policy-name",
            "candidate",
            "--doubly-robust",
            "--bootstrap-resamples",
            "16",
            "--bootstrap-seed",
            "9",
        ])
        .assert()
        .success();

    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(report["ips"]["policy_name"], "candidate");
    assert!(report["doubly_robust"]["estimate"].is_number());
    assert_eq!(report["bootstrap"]["seed"], 9);

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn offpolicy_command_rejects_malformed_json_with_input_exit_code() {
    let input = temp_path("offpolicy_invalid.jsonl");
    let output = temp_path("offpolicy_invalid_report.json");
    std::fs::write(&input, "not-json\n").unwrap();
    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "offpolicy",
            input.to_str().unwrap(),
            "--out",
            output.to_str().unwrap(),
        ])
        .assert()
        .code(3);
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn offpolicy_command_rejects_excessive_bootstrap_resamples_before_output() {
    let input = temp_path("offpolicy_excessive_bootstrap.jsonl");
    let output = temp_path("offpolicy_excessive_bootstrap_report.json");
    std::fs::write(
        &input,
        "{\"state\":\"s\",\"action\":\"a\",\"reward\":1.0,\"logging_propensity\":1.0,\"evaluation_probability\":1.0}\n",
    )
    .unwrap();
    let excessive = (lineprior::MAX_BOOTSTRAP_RESAMPLES + 1).to_string();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "offpolicy",
            input.to_str().unwrap(),
            "--out",
            output.to_str().unwrap(),
            "--bootstrap-resamples",
            &excessive,
        ])
        .assert()
        .code(3);
    assert!(!output.exists());

    let _ = std::fs::remove_file(&input);
}

#[test]
fn gate_command_writes_verdict_and_acquisition_report() {
    let input = temp_path("gate_input.jsonl");
    let output = temp_path("gate_report.json");
    let rows = (0..6)
        .map(|index| {
            format!(
                "{{\"candidate_id\":\"c-{index}\",\"group_id\":\"g-{}\",\"features\":{{\"signal\":{index}}},\"gate_elo_delta\":{},\"gate_games_played\":100}}",
                if index < 3 { "a" } else { "b" },
                index * 2 - 3
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&input, rows + "\n").unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "gate",
            input.to_str().unwrap(),
            "--feature",
            "signal=4",
            "--monotonic",
            "signal=increasing",
            "--expected-gate-cost",
            "10",
            "--out",
            output.to_str().unwrap(),
        ])
        .assert()
        .success();

    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&output).unwrap()).unwrap();
    assert!(report["prediction"]["expected_elo"].is_number());
    assert!(report["verdict"]["pass_probability"].is_number());
    assert!(report["acquisition"]["acquisition_score"].is_number());

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
}

#[test]
fn validate_command_checks_raw_offpolicy_semantics() {
    let valid = temp_path("validate_offpolicy_valid.jsonl");
    let invalid = temp_path("validate_offpolicy_invalid.jsonl");
    std::fs::write(
        &valid,
        "{\"state\":\"s\",\"action\":\"a\",\"reward\":1.0,\"logging_propensity\":0.5,\"evaluation_probability\":0.5}\n",
    )
    .unwrap();
    std::fs::write(
        &invalid,
        "{\"state\":\"s\",\"action\":\"a\",\"reward\":1.0,\"logging_propensity\":1.1,\"evaluation_probability\":0.5}\n",
    )
    .unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args(["validate", valid.to_str().unwrap(), "--kind", "offpolicy"])
        .assert()
        .success();
    Command::cargo_bin("lineprior")
        .unwrap()
        .args(["validate", invalid.to_str().unwrap(), "--kind", "offpolicy"])
        .assert()
        .code(3);

    let _ = std::fs::remove_file(&valid);
    let _ = std::fs::remove_file(&invalid);
}

#[test]
fn validate_command_checks_gate_rows_without_requiring_a_fit_sized_history() {
    let input = temp_path("validate_gate.jsonl");
    std::fs::write(
        &input,
        "{\"candidate_id\":\"c-1\",\"group_id\":\"g-1\",\"features\":{\"signal\":1.0},\"gate_elo_delta\":2.0,\"gate_games_played\":20.0}\n",
    )
    .unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args(["validate", input.to_str().unwrap(), "--kind", "gate"])
        .assert()
        .success();

    let _ = std::fs::remove_file(&input);
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lineprior_cli_test_{name}"))
}

fn sha256(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

#[test]
fn pack_command_rejects_malformed_input_without_creating_output() {
    let input = temp_path("pack_malformed_input.jsonl");
    let out = temp_path("pack_malformed_output.lpb");
    let _ = std::fs::remove_file(&out);
    std::fs::write(&input, "not-json\n").unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "pack",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(3);

    assert!(!out.exists());
    std::fs::remove_file(&input).unwrap();
}

#[test]
fn unpack_command_rejects_truncated_input_without_creating_output() {
    let input = temp_path("unpack_truncated_input.lpb");
    let out = temp_path("unpack_truncated_output.jsonl");
    let _ = std::fs::remove_file(&out);
    std::fs::write(&input, b"LPB\0").unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "unpack",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(3);

    assert!(!out.exists());
    std::fs::remove_file(&input).unwrap();
}

#[test]
fn pack_command_rejects_identical_input_and_output() {
    let input = temp_path("pack_same_input_output.jsonl");
    let contents = b"not parsed because paths collide\n";
    std::fs::write(&input, contents).unwrap();

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "pack",
            input.to_str().unwrap(),
            "--out",
            input.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("input and --out"));
    assert_eq!(std::fs::read(&input).unwrap(), contents);
    std::fs::remove_file(&input).unwrap();
}

#[cfg(any(unix, windows))]
#[test]
fn unpack_command_rejects_hard_linked_input_and_output() {
    let input = temp_path("unpack_hard_link_input.lpb");
    let out = temp_path("unpack_hard_link_output.jsonl");
    let contents = b"not parsed because paths collide";
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&out);
    std::fs::write(&input, contents).unwrap();
    std::fs::hard_link(&input, &out).unwrap();

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "unpack",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("input and --out"));
    assert_eq!(std::fs::read(&input).unwrap(), contents);
    std::fs::remove_file(&out).unwrap();
    std::fs::remove_file(&input).unwrap();
}

#[test]
fn build_command_writes_a_prior_book() {
    let out = temp_path("build_writes.jsonl");

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            fixture("mixed_outcomes.jsonl").to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--min-count",
            "1",
        ])
        .assert()
        .success();

    let contents = std::fs::read_to_string(&out).unwrap();
    let header: serde_json::Value = serde_json::from_str(contents.lines().next().unwrap()).unwrap();
    assert_eq!(header["prior_book_schema_version"], 1);
    assert_eq!(header["producer_version"], env!("CARGO_PKG_VERSION"));
    assert!(header["build_config"].is_object());
    assert!(header["build_config_fingerprint"].is_u64());
    assert!(contents.contains("\"state\":\"state_a\""));
    assert!(contents.contains("\"prior\""));
    assert!(contents.contains("\"confidence\""));

    let _ = std::fs::remove_file(&out);
}

#[test]
fn build_command_writes_a_content_linked_artifact_manifest() {
    let input = fixture("mixed_outcomes.jsonl");
    let out = temp_path("build_manifest_book.jsonl");
    let manifest = temp_path("build_manifest.json");

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--min-count",
            "1",
            "--artifact-manifest",
            manifest.to_str().unwrap(),
            "--dataset-id",
            "fixture-v1",
            "--split",
            "train",
            "--adapter-id",
            "fixture-adapter",
            "--adapter-version",
            "v1",
            "--lineage",
            "downstream_commit=abc123",
        ])
        .assert()
        .success();

    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    assert_eq!(value["schema"], "lineprior.prior-artifact-manifest.v1");
    assert_eq!(value["producer_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(value["input_sha256"], sha256(&input));
    assert_eq!(value["prior_book_sha256"], sha256(&out));
    assert_eq!(value["lineage"]["dataset_id"], "fixture-v1");
    assert_eq!(value["lineage"]["metadata"]["downstream_commit"], "abc123");
    assert!(value["summary"]["build_stats"]["observations_kept"].is_u64());

    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&manifest);
}

#[test]
fn verify_artifact_checks_round_trip_bytes_and_book_metadata() {
    let input = temp_path("verify_artifact_input.jsonl");
    let out = temp_path("verify_artifact_book.jsonl");
    let manifest = temp_path("verify_artifact_manifest.json");
    let input_bytes = std::fs::read(fixture("mixed_outcomes.jsonl")).unwrap();
    std::fs::write(&input, &input_bytes).unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--artifact-manifest",
            manifest.to_str().unwrap(),
        ])
        .assert()
        .success();

    let verify = || {
        Command::cargo_bin("lineprior")
            .unwrap()
            .args([
                "verify-artifact",
                "--manifest",
                manifest.to_str().unwrap(),
                "--input",
                input.to_str().unwrap(),
                "--book",
                out.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let output = verify();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("schema-v1 book metadata"));

    std::fs::write(&input, [input_bytes.as_slice(), b"\n"].concat()).unwrap();
    let output = verify();
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("input_sha256 mismatch"));
    std::fs::write(&input, &input_bytes).unwrap();

    let book_bytes = std::fs::read(&out).unwrap();
    std::fs::write(&out, [book_bytes.as_slice(), b"\n"].concat()).unwrap();
    let output = verify();
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("prior_book_sha256 mismatch"));
    std::fs::write(&out, &book_bytes).unwrap();

    let mut manifest_value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    let original_manifest_value = manifest_value.clone();

    manifest_value["schema"] = serde_json::json!("lineprior.prior-artifact-manifest.v2");
    std::fs::write(
        &manifest,
        serde_json::to_vec_pretty(&manifest_value).unwrap(),
    )
    .unwrap();
    let output = verify();
    assert_eq!(output.status.code(), Some(3));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unsupported artifact manifest schema")
    );

    manifest_value = original_manifest_value.clone();
    manifest_value["input_sha256"] = serde_json::json!("A".repeat(64));
    std::fs::write(
        &manifest,
        serde_json::to_vec_pretty(&manifest_value).unwrap(),
    )
    .unwrap();
    let output = verify();
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("must be a lowercase SHA-256 digest"));

    manifest_value = original_manifest_value;
    manifest_value["producer_version"] = serde_json::json!("tampered");
    std::fs::write(
        &manifest,
        serde_json::to_vec_pretty(&manifest_value).unwrap(),
    )
    .unwrap();
    let output = verify();
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("producer_version mismatch"));

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&manifest);
}

#[test]
fn build_command_rejects_artifact_manifest_path_collisions() {
    let out = temp_path("build_manifest_collision.jsonl");
    let _ = std::fs::remove_file(&out);
    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            fixture("mixed_outcomes.jsonl").to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--artifact-manifest",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(3);
    assert!(!out.exists());
}

#[test]
fn build_command_rejects_nonexistent_artifact_manifest_path_aliases() {
    let out = temp_path("build_manifest_alias_collision.jsonl");
    let alias_dir = temp_path("build_manifest_alias_parent");
    let manifest_alias = alias_dir.join("..").join(out.file_name().unwrap());
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_dir(&alias_dir);
    std::fs::create_dir(&alias_dir).unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            fixture("mixed_outcomes.jsonl").to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--artifact-manifest",
            manifest_alias.to_str().unwrap(),
        ])
        .assert()
        .code(3);

    assert!(!out.exists());
    std::fs::remove_dir(&alias_dir).unwrap();
}

#[cfg(any(unix, windows))]
#[test]
fn build_command_rejects_hard_linked_input_and_output() {
    let input = temp_path("build_hard_link_input.jsonl");
    let out = temp_path("build_hard_link_output.jsonl");
    let contents = std::fs::read(fixture("mixed_outcomes.jsonl")).unwrap();
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&out);
    std::fs::write(&input, &contents).unwrap();
    std::fs::hard_link(&input, &out).unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(3);

    assert_eq!(std::fs::read(&input).unwrap(), contents);
    std::fs::remove_file(&out).unwrap();
    std::fs::remove_file(&input).unwrap();
}

#[test]
fn build_command_reports_no_usable_data_as_exit_code_two() {
    let out = temp_path("build_empty.jsonl");

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            fixture("mixed_outcomes.jsonl").to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--max-step",
            "0",
            "--tags",
            "no-such-tag",
        ])
        .assert()
        .code(2);
}

#[test]
fn build_command_applies_time_decay() {
    let input = temp_path("decay_input.jsonl");
    let out_no_decay = temp_path("decay_out_no_decay.jsonl");
    let out_decay = temp_path("decay_out_decay.jsonl");
    std::fs::write(
        &input,
        "{\"sequence_id\":\"s1\",\"step\":0,\"state\":\"s\",\"action\":\"a\",\
         \"outcome\":\"success\",\"observed_at_unix_seconds\":0}\n",
    )
    .unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out_no_decay.to_str().unwrap(),
        ])
        .assert()
        .success();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out_decay.to_str().unwrap(),
            "--time-decay-half-life-days",
            "10",
            "--time-decay-reference-unix-seconds",
            "864000", // 10 days after the epoch -- exactly one half-life
        ])
        .assert()
        .success();

    assert!(
        std::fs::read_to_string(&out_no_decay)
            .unwrap()
            .contains("\"weighted_count\":1.0")
    );
    assert!(
        std::fs::read_to_string(&out_decay)
            .unwrap()
            .contains("\"weighted_count\":0.5")
    );

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&out_no_decay);
    let _ = std::fs::remove_file(&out_decay);
}

#[test]
fn build_command_applies_source_weights() {
    let input = temp_path("source_weights_input.jsonl");
    let out = temp_path("source_weights_out.jsonl");
    std::fs::write(
        &input,
        "{\"sequence_id\":\"s1\",\"step\":0,\"state\":\"s\",\"action\":\"a\",\
         \"outcome\":\"success\",\"source\":\"human\"}\n",
    )
    .unwrap();

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--source-weights",
            "human=0.5",
        ])
        .assert()
        .success();

    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("\"weighted_count\":0.5")
    );

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn build_command_rejects_half_life_without_reference() {
    let input = temp_path("invalid_decay_input.jsonl");
    let out = temp_path("invalid_decay_out.jsonl");
    std::fs::write(
        &input,
        "{\"sequence_id\":\"s1\",\"step\":0,\"state\":\"s\",\"action\":\"a\",\"outcome\":\"success\"}\n",
    )
    .unwrap();

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--time-decay-half-life-days",
            "10",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("time_decay_reference_unix_seconds"));

    let _ = std::fs::remove_file(&input);
}

#[test]
fn query_command_finds_known_state_and_returns_nothing_for_unseen() {
    let out = temp_path("query_book.jsonl");

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            fixture("simple_success.jsonl").to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let known = Command::cargo_bin("lineprior")
        .unwrap()
        .args(["query", out.to_str().unwrap(), "--state", "state_a"])
        .output()
        .unwrap();
    assert!(known.status.success());
    let stdout = String::from_utf8(known.stdout).unwrap();
    assert!(stdout.contains("action_x"));

    let unseen = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "query",
            out.to_str().unwrap(),
            "--state",
            "nonexistent_state",
        ])
        .output()
        .unwrap();
    assert!(unseen.status.success());
    assert!(String::from_utf8(unseen.stdout).unwrap().is_empty());

    let _ = std::fs::remove_file(&out);
}

/// Enough distinct sequences that an 80/20 hash split reliably yields a
/// non-empty test set, unlike the small hand-written fixtures above.
fn write_eval_fixture(path: &std::path::Path) {
    let mut jsonl = String::new();
    for i in 0..60 {
        jsonl.push_str(&format!(
            "{{\"sequence_id\":\"seq-{i}\",\"step\":0,\"state\":\"s\",\"action\":\"a\",\"outcome\":\"success\"}}\n"
        ));
    }
    std::fs::write(path, jsonl).unwrap();
}

#[test]
fn eval_command_prints_a_valid_json_report_to_stdout() {
    let input = temp_path("eval_input.jsonl");
    write_eval_fixture(&input);

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args(["eval", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(report["num_train_observations"].as_u64().unwrap() > 0);
    assert!(report["num_test_observations"].as_u64().unwrap() > 0);
    assert_eq!(report["top1_hit_rate"], 1.0);

    let _ = std::fs::remove_file(&input);
}

#[test]
fn eval_command_writes_out_file_matching_stdout() {
    let input = temp_path("eval_input_out.jsonl");
    let out = temp_path("eval_report.json");
    write_eval_fixture(&input);

    let stdout_run = Command::cargo_bin("lineprior")
        .unwrap()
        .args(["eval", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(stdout_run.status.success());

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "eval",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    let from_stdout = String::from_utf8(stdout_run.stdout).unwrap();
    let from_file = std::fs::read_to_string(&out).unwrap();
    assert_eq!(
        from_stdout.trim(),
        from_file.trim(),
        "--out content should match stdout content for the same input"
    );

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn eval_command_reports_calibration_and_threshold_sweep_when_requested() {
    let input = temp_path("eval_calibration_input.jsonl");
    write_eval_fixture(&input);

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "eval",
            input.to_str().unwrap(),
            "--confidence-mode",
            "wilson-lower-bound",
            "--calibration-bins",
            "5",
            "--thresholds",
            "0.1,0.5",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        report["confidence_calibration"].as_array().unwrap().len(),
        5
    );
    assert_eq!(report["threshold_sweep"].as_array().unwrap().len(), 2);

    let _ = std::fs::remove_file(&input);
}

#[test]
fn tune_command_runs_a_grid_and_reports_consistent_counts() {
    let input = temp_path("tune_grid_input.jsonl");
    write_eval_fixture(&input);

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "tune",
            input.to_str().unwrap(),
            "--param",
            "min-confidence=0.0,0.3,0.7",
            "--param",
            "smoothing-alpha=1.0,5.0",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let evaluated = report["evaluated_config_count"].as_u64().unwrap();
    let skipped = report["skipped_config_count"].as_u64().unwrap();
    assert_eq!(evaluated + skipped, 6); // 3 min-confidence values * 2 smoothing-alpha values
    assert_eq!(
        report["all_results"].as_array().unwrap().len(),
        evaluated as usize
    );
    assert!(report["best"].is_object());

    let _ = std::fs::remove_file(&input);
}

#[test]
fn tune_command_success_weighted_objective_runs_and_reports_the_metric() {
    let input = temp_path("tune_success_weighted_input.jsonl");
    write_eval_fixture(&input);

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "tune",
            input.to_str().unwrap(),
            "--param",
            "min-count=1,2",
            "--objective",
            "success-weighted-mrr",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["objective"], "success_weighted_mrr");
    let first = &report["all_results"].as_array().unwrap()[0];
    assert!(first["metrics"]["success_weighted_mean_reciprocal_rank"].is_number());
    assert!(first["metrics"]["success_weighted_top1_hit_rate"].is_number());

    let _ = std::fs::remove_file(&input);
}

#[test]
fn tune_command_reports_null_best_when_no_candidate_meets_constraints() {
    let input = temp_path("tune_unsatisfiable_input.jsonl");
    write_eval_fixture(&input);

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "tune",
            input.to_str().unwrap(),
            "--param",
            "min-confidence=0.0,0.5",
            "--min-covered-fraction",
            "1.5", // impossible to satisfy -- coverage can never exceed 1.0
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(report["best"].is_null());
    assert!(!report["all_results"].as_array().unwrap().is_empty());
    assert!(report["warnings"].as_array().unwrap().iter().any(|w| {
        w.as_str()
            .unwrap()
            .contains("no candidate configuration satisfied")
    }));

    let _ = std::fs::remove_file(&input);
}

#[test]
fn tune_save_best_config_can_be_loaded_by_build_config_flag() {
    let input = temp_path("tune_save_best_input.jsonl");
    let best_config = temp_path("tune_best_config.json");
    let out = temp_path("tune_save_best_build_out.jsonl");
    write_eval_fixture(&input);

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "tune",
            input.to_str().unwrap(),
            "--param",
            "min-confidence=0.0,0.3",
            "--save-best-config",
            best_config.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(best_config.exists());

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            fixture("simple_success.jsonl").to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--config",
            best_config.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(out.exists());

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&best_config);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn build_command_rejects_config_combined_with_individual_flag() {
    let out = temp_path("config_conflict_out.jsonl");
    // An empty JSON object deserializes to BuildConfig::default() via
    // #[serde(default)] -- a minimal stand-in for a `tune`-saved config.
    let config_path = temp_path("config_conflict_config.json");
    std::fs::write(&config_path, "{}").unwrap();

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            fixture("simple_success.jsonl").to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--config",
            config_path.to_str().unwrap(),
            "--min-count",
            "5",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("--config cannot be combined"));

    let _ = std::fs::remove_file(&config_path);
}

/// Two-step sequences: state "s0" always precedes state "s", and step 0's
/// action ("x" or "y") determines step 1's action ("A" or "B") -- order-0
/// alone can't tell A/B apart at state "s", but order-1 context can.
fn write_context_fixture(path: &std::path::Path, num_sequences: usize) {
    let mut jsonl = String::new();
    for i in 0..num_sequences {
        let (first, second) = if i % 2 == 0 { ("x", "A") } else { ("y", "B") };
        jsonl.push_str(&format!(
            "{{\"sequence_id\":\"seq-{i}\",\"step\":0,\"state\":\"s0\",\"action\":\"{first}\",\"outcome\":\"success\"}}\n"
        ));
        jsonl.push_str(&format!(
            "{{\"sequence_id\":\"seq-{i}\",\"step\":1,\"state\":\"s\",\"action\":\"{second}\",\"outcome\":\"success\"}}\n"
        ));
    }
    std::fs::write(path, jsonl).unwrap();
}

#[test]
fn build_command_with_context_order_produces_context_bearing_lines() {
    let input = temp_path("context_build_input.jsonl");
    let out = temp_path("context_build_output.jsonl");
    write_context_fixture(&input, 4);

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--context-order",
            "1",
        ])
        .assert()
        .success();

    let contents = std::fs::read_to_string(&out).unwrap();
    assert!(
        contents.contains("\"context\":[\"x\"]") || contents.contains("\"context\":[\"y\"]"),
        "expected a context-bearing line in: {contents}"
    );

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn query_command_with_recent_actions_returns_matched_order() {
    let input = temp_path("context_query_input.jsonl");
    let out = temp_path("context_query_book.jsonl");
    write_context_fixture(&input, 4);

    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--context-order",
            "1",
        ])
        .assert()
        .success();

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "query",
            out.to_str().unwrap(),
            "--state",
            "s",
            "--recent-actions",
            "x",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let result: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(result["matched_order"], 1);
    assert_eq!(result["candidates"][0]["action"], "A");

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn eval_command_with_context_order_reports_context_aware_fields() {
    let input = temp_path("context_eval_input.jsonl");
    write_context_fixture(&input, 60);

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args(["eval", input.to_str().unwrap(), "--context-order", "1"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(report["context_top1_hit_rate"].is_number());
    assert!(report["context_mean_reciprocal_rank"].is_number());
    assert!(report["context_calibration_brier"].is_number());
    assert!(
        !report["hit_rate_by_matched_order"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(report["hit_rate_by_matched_order"][0]["calibration_brier"].is_number());

    let _ = std::fs::remove_file(&input);
}

#[test]
fn build_command_rejects_unsorted_input_when_context_order_is_set() {
    let input = temp_path("context_unsorted_input.jsonl");
    // Same sequence_id, step goes 1 then 0 -- not sorted.
    std::fs::write(
        &input,
        concat!(
            "{\"sequence_id\":\"g1\",\"step\":1,\"state\":\"s\",\"action\":\"a\",\"outcome\":\"success\"}\n",
            "{\"sequence_id\":\"g1\",\"step\":0,\"state\":\"s\",\"action\":\"b\",\"outcome\":\"success\"}\n",
        ),
    )
    .unwrap();
    let out = temp_path("context_unsorted_output.jsonl");

    let output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--context-order",
            "1",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("sorted"), "stderr: {stderr}");

    let _ = std::fs::remove_file(&input);
}

#[test]
fn build_command_score_weight_flags_change_ranking() {
    let input = temp_path("score_weight_input.jsonl");
    let out = temp_path("score_weight_out.jsonl");
    // "a": 1 observation, high score -- small count term, large score term.
    // "b": 10 observations, zero score -- large count term, zero score term.
    let mut lines = String::from(
        "{\"sequence_id\":\"s0\",\"step\":0,\"state\":\"s\",\"action\":\"a\",\
         \"outcome\":\"success\",\"score\":1.0}\n",
    );
    for i in 0..10 {
        lines.push_str(&format!(
            "{{\"sequence_id\":\"s{}\",\"step\":0,\"state\":\"s\",\"action\":\"b\",\
             \"outcome\":\"success\",\"score\":0.0}}\n",
            i + 1
        ));
    }
    std::fs::write(&input, lines).unwrap();

    // Default weights (count=success=score=1): "b"'s large count term wins, ranks first.
    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--smoothing-alpha",
            "0",
        ])
        .assert()
        .success();
    let default_ranked = std::fs::read_to_string(&out).unwrap();
    assert!(
        default_ranked.find("\"action\":\"b\"") < default_ranked.find("\"action\":\"a\""),
        "expected b (higher count) to rank first under default weights: {default_ranked}"
    );

    // Zeroing count/success weight and keeping only score_weight flips the ranking to "a".
    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            input.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--smoothing-alpha",
            "0",
            "--count-weight",
            "0",
            "--success-weight",
            "0",
            "--score-weight",
            "1",
        ])
        .assert()
        .success();
    let score_only_ranked = std::fs::read_to_string(&out).unwrap();
    assert!(
        score_only_ranked.find("\"action\":\"a\"") < score_only_ranked.find("\"action\":\"b\""),
        "expected a (higher score) to rank first under score_weight-only: {score_only_ranked}"
    );

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn tune_sweeps_the_three_scoring_weights_and_the_saved_config_round_trips_through_build_and_eval() {
    let input = temp_path("tune_weight_sweep_input.jsonl");
    let best_config = temp_path("tune_weight_sweep_best_config.json");
    let build_out = temp_path("tune_weight_sweep_build_out.jsonl");
    write_eval_fixture(&input);

    let tune_output = Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "tune",
            input.to_str().unwrap(),
            "--param",
            "count-weight=0.0,1.0",
            "--param",
            "success-weight=0.5,1.0",
            "--param",
            "score-weight=1.0",
            "--save-best-config",
            best_config.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(tune_output.status.success());
    let report: serde_json::Value =
        serde_json::from_slice(&tune_output.stdout).expect("valid JSON report");
    // 2 (count-weight) x 2 (success-weight) x 1 (score-weight) = 4 candidates.
    assert_eq!(report["evaluated_config_count"].as_u64(), Some(4));
    assert!(best_config.exists());

    let saved_config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&best_config).unwrap()).unwrap();
    assert_eq!(saved_config["score_weight"], 1.0);
    assert!(
        saved_config["count_weight"] == 0.0 || saved_config["count_weight"] == 1.0,
        "count_weight should be one of the swept values, got {saved_config}"
    );

    // Round-trip through build.
    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "build",
            fixture("simple_success.jsonl").to_str().unwrap(),
            "--out",
            build_out.to_str().unwrap(),
            "--config",
            best_config.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(build_out.exists());

    // Round-trip through eval.
    Command::cargo_bin("lineprior")
        .unwrap()
        .args([
            "eval",
            input.to_str().unwrap(),
            "--config",
            best_config.to_str().unwrap(),
        ])
        .assert()
        .success();

    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&best_config);
    let _ = std::fs::remove_file(&build_out);
}
