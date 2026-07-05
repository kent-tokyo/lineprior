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
