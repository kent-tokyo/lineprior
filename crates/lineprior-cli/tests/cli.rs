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
