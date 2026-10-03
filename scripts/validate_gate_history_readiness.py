#!/usr/bin/env python3
"""Validate a GateModel real-history readiness artifact."""

import argparse
import hashlib
import json
import pathlib
import re

from version_contract import workspace_version


ROOT = pathlib.Path(__file__).resolve().parent.parent
SHA256 = re.compile(r"[0-9a-f]{64}")
GIT_COMMIT = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def manifest_sha256(records):
    manifest = "".join(f"{row['path']}\0{row['sha256']}\n" for row in records)
    return hashlib.sha256(manifest.encode()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("report")
    parser.add_argument("--expected-lineprior-version", default=workspace_version())
    args = parser.parse_args()
    report = json.loads(pathlib.Path(args.report).read_text())
    require(report.get("protocol") == "gate-history-readiness-audit-v1", "unexpected protocol")
    require(report.get("status") in ("ready", "not_estimable"), "unexpected status")
    measurement = report["measurement"]
    require(measurement.get("lineprior_version") == args.expected_lineprior_version, "version mismatch")
    require(bool(GIT_COMMIT.fullmatch(measurement["dataset_source_commit"])), "invalid source commit")

    runner = (ROOT / measurement["runner_path"]).resolve()
    require(ROOT in runner.parents and runner.is_file(), "runner path is missing or outside repository")
    require(hashlib.sha256(runner.read_bytes()).hexdigest() == measurement["runner_sha256"], "runner hash mismatch")

    audit = report["audit"]
    records = audit["records"]
    summary = audit["summary"]
    require(summary["aggregate_outcome_files"] == len(records), "aggregate count mismatch")
    require(measurement["dataset_files_content_pinned"] == len(records), "content-pinned count mismatch")
    require(
        0 <= measurement["dataset_files_git_tracked"] <= len(records),
        "git-tracked count is out of range",
    )
    require(summary["parse_errors"] == len(audit["parse_errors"]), "parse-error count mismatch")
    require(records == sorted(records, key=lambda row: row["path"]), "records are not sorted")
    require(len({row["path"] for row in records}) == len(records), "duplicate record path")
    for row in records:
        require(not pathlib.PurePosixPath(row["path"]).is_absolute(), "absolute input path is not portable")
        require(bool(SHA256.fullmatch(row["sha256"])), f"invalid input hash for {row['path']}")
        require(row["games"] >= measurement["min_games"], f"games below threshold for {row['path']}")
        require(
            row["eligible_gate_observation"]
            == (
                row["candidate_identity_present"]
                and row["group_id_present"]
                and row["numeric_feature_map_present"]
            ),
            f"eligibility mismatch for {row['path']}",
        )
    require(
        manifest_sha256(records) == measurement["aggregate_content_manifest_sha256"],
        "aggregate content manifest hash mismatch",
    )
    require(
        sum(row["eligible_gate_observation"] for row in records)
        == summary["eligible_gate_observation_rows"],
        "eligible-row count mismatch",
    )

    if report["status"] == "not_estimable":
        require(report["decision"] == "do_not_benchmark_or_expand_gate_model", "unsafe decision")
        require(not summary["gate_model_fit_contract_ready"], "non-estimable report is fit-ready")
        require(
            all(value == "not_computable" for value in report["requested_comparisons"].values()),
            "non-estimable report contains a computed comparison",
        )
    require(report["required_next_history_fields"], "next-history contract is missing")
    print("gate-history readiness artifact contract: ok")


if __name__ == "__main__":
    try:
        main()
    except (KeyError, OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"gate-history readiness artifact error: {error}")
