#!/usr/bin/env python3
"""Audit real decision logs before any IPS/DR estimate is attempted."""

import argparse
import hashlib
import json
import pathlib
import subprocess

from version_contract import workspace_version


ROOT = pathlib.Path(__file__).resolve().parent.parent


def sha256_file(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def is_decision(row):
    return row.get("type") == "search" or (
        any(key in row for key in ("state", "sfen"))
        and any(key in row for key in ("action", "bestmove", "bestmove_csa"))
    )


def audit_rows(rows):
    decisions = [row for row in rows if is_decision(row)]
    counts = {
        "decision_rows": len(decisions),
        "state": sum(any(key in row for key in ("state", "sfen")) for row in decisions),
        "action": sum(any(key in row for key in ("action", "bestmove", "bestmove_csa")) for row in decisions),
        "reward": sum("reward" in row for row in decisions),
        "logging_propensity": sum("logging_propensity" in row for row in decisions),
        "evaluation_probability": sum("evaluation_probability" in row for row in decisions),
        "reward_model_policy_value": sum("reward_model_policy_value" in row for row in decisions),
        "reward_model_logged_action": sum("reward_model_logged_action" in row for row in decisions),
    }
    invalid_propensities = sum(
        not isinstance(row.get("logging_propensity"), (int, float))
        or isinstance(row.get("logging_propensity"), bool)
        or not 0.0 < row["logging_propensity"] <= 1.0
        or not isinstance(row.get("evaluation_probability"), (int, float))
        or isinstance(row.get("evaluation_probability"), bool)
        or not 0.0 <= row["evaluation_probability"] <= 1.0
        for row in decisions
        if "logging_propensity" in row and "evaluation_probability" in row
    )
    required_ips = ("state", "action", "reward", "logging_propensity", "evaluation_probability")
    ips_ready = bool(decisions) and all(counts[key] == len(decisions) for key in required_ips) and invalid_propensities == 0
    dr_ready = ips_ready and all(
        counts[key] == len(decisions)
        for key in ("reward_model_policy_value", "reward_model_logged_action")
    )
    return {
        "rows": len(rows),
        "fields_present": counts,
        "invalid_propensity_pairs": invalid_propensities,
        "ips_status": "ready" if ips_ready else "not_estimable",
        "dr_status": "ready" if dr_ready else "not_estimable",
        "support_status": "not_computable" if not ips_ready else "requires_estimator_run",
        "overlap_status": "not_computable" if not ips_ready else "requires_estimator_run",
        "effective_sample_size": None,
        "causal_claim_permitted": False,
    }


def git_lineage():
    try:
        commit = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, stderr=subprocess.DEVNULL
        ).strip()
        dirty = bool(
            subprocess.check_output(
                ["git", "status", "--porcelain"], cwd=ROOT, text=True, stderr=subprocess.DEVNULL
            ).strip()
        )
        return commit, dirty
    except (OSError, subprocess.CalledProcessError):
        return "unavailable", None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("inputs", nargs="+")
    parser.add_argument("--out", required=True)
    parser.add_argument("--dataset-id", required=True)
    parser.add_argument("--dataset-source-commit", required=True)
    args = parser.parse_args()

    rows = []
    hashes = {}
    for name in args.inputs:
        path = pathlib.Path(name)
        hashes[path.name] = sha256_file(path)
        for line_number, line in enumerate(path.read_text().splitlines(), 1):
            if not line.strip():
                continue
            row = json.loads(line)
            if not isinstance(row, dict):
                raise ValueError(f"{path} line {line_number}: expected object")
            rows.append(row)
    audit = audit_rows(rows)
    commit, dirty = git_lineage()
    report = {
        "protocol": "offpolicy-readiness-audit-v1",
        "status": "not_estimable" if audit["ips_status"] != "ready" else "ready",
        "decision": "do_not_run_ips_dr_or_make_causal_claim",
        "measurement": {
            "dataset_id": args.dataset_id,
            "dataset_source_commit": args.dataset_source_commit,
            "lineprior_version": workspace_version(),
            "input_sha256": hashes,
            "runner_commit": commit,
            "runner_tree_dirty": dirty,
            "runner_path": "scripts/audit_offpolicy_readiness.py",
            "runner_sha256": sha256_file(__file__),
        },
        "audit": audit,
        "required_next_log_fields": [
            "state",
            "action",
            "terminal_or_declared_horizon_reward",
            "logging_propensity_recorded_at_decision_time",
            "evaluation_probability",
            "paired_arm_id",
            "split_group_id",
        ],
        "notes": [
            "search score, PV rank, and post-hoc softmax are not logging propensities",
            "bootstrap intervals, weight caps, overlap, and ESS are undefined until valid propensities exist",
        ],
    }
    pathlib.Path(args.out).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    try:
        main()
    except (OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"off-policy readiness audit error: {error}")
