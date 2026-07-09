use assert_cmd::Command;
use std::path::{Path, PathBuf};

// Fixtures live at the workspace root so lib and CLI tests can share them.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("lineprior_cli_test_{name}"))
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
    assert!(contents.contains("\"state\":\"state_a\""));
    assert!(contents.contains("\"prior\""));
    assert!(contents.contains("\"confidence\""));

    let _ = std::fs::remove_file(&out);
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
