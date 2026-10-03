#!/usr/bin/env python3
"""Audit real gate history before benchmarking GateModel.

The audit deliberately looks only for candidate-level aggregate JSON files.
Per-opening shards and intermediate snapshots are not independent candidates
and would make a tiny history look much larger than it is.
"""

import argparse
import hashlib
import json
import math
import pathlib
import subprocess

from version_contract import workspace_version


ROOT = pathlib.Path(__file__).resolve().parent.parent
AGGREGATE_NAMES = {"combined.json", "final.json", "result.json", "summary.json"}
SEARCH_ROOTS = ("data/runs", "results")
VERDICTS = {"PASS", "FAIL", "INCONCLUSIVE"}


def sha256_file(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def first_number(document, paths):
    for path in paths:
        value = document
        for key in path:
            if not isinstance(value, dict) or key not in value:
                break
            value = value[key]
        else:
            if is_number(value):
                return float(value)
    return None


def candidate_identity(document):
    candidate_id = document.get("candidate_id")
    if isinstance(candidate_id, str) and candidate_id:
        return candidate_id
    candidate = document.get("candidate")
    if isinstance(candidate, dict):
        for key in ("sha256", "id", "path", "name"):
            value = candidate.get(key)
            if isinstance(value, str) and value:
                return value
    return None


def numeric_features(document):
    features = document.get("features")
    if not isinstance(features, dict) or not features:
        return None
    if not all(isinstance(name, str) and name and is_number(value) for name, value in features.items()):
        return None
    return {name: float(value) for name, value in sorted(features.items())}


def gate_status(document):
    for value in (document.get("gate_status"), document.get("verdict"), document.get("status")):
        if isinstance(value, str) and value.upper() in VERDICTS:
            return value.upper()
    return None


def inspect_aggregate(path, source_root, min_games):
    document = json.loads(path.read_text())
    if not isinstance(document, dict):
        return None
    elo = first_number(document, (("elo_diff",), ("summary", "elo_diff"), ("combined", "elo_diff")))
    games = first_number(document, (("games",), ("summary", "games"), ("combined", "games")))
    if elo is None or games is None or games < min_games:
        return None

    features = numeric_features(document)
    candidate_id = candidate_identity(document)
    group_id = document.get("group_id")
    if not isinstance(group_id, str) or not group_id:
        group_id = None
    ci_low = first_number(document, (("elo_ci_low",), ("summary", "elo_ci_low")))
    ci_high = first_number(document, (("elo_ci_high",), ("summary", "elo_ci_high")))
    has_uncertainty = ci_low is not None and ci_high is not None and ci_low <= ci_high
    eligible = candidate_id is not None and group_id is not None and features is not None

    return {
        "path": path.relative_to(source_root).as_posix(),
        "sha256": sha256_file(path),
        "elo_diff": elo,
        "games": games,
        "candidate_id": candidate_id,
        "group_id": group_id,
        "candidate_identity_present": candidate_id is not None,
        "group_id_present": group_id is not None,
        "numeric_feature_map_present": features is not None,
        "feature_names": sorted(features) if features is not None else [],
        "gate_status": gate_status(document),
        "uncertainty_interval_present": has_uncertainty,
        "eligible_gate_observation": eligible,
    }


def manifest_sha256(records):
    manifest = "".join(f"{row['path']}\0{row['sha256']}\n" for row in records)
    return hashlib.sha256(manifest.encode()).hexdigest()


def audit_history(source_root, min_games=20.0):
    source_root = pathlib.Path(source_root)
    records = []
    parse_errors = []
    for directory in SEARCH_ROOTS:
        base = source_root / directory
        if not base.is_dir():
            continue
        for path in sorted(base.rglob("*.json")):
            if path.name not in AGGREGATE_NAMES:
                continue
            try:
                record = inspect_aggregate(path, source_root, min_games)
            except (OSError, TypeError, ValueError, json.JSONDecodeError) as error:
                parse_errors.append({"path": path.relative_to(source_root).as_posix(), "error": str(error)})
                continue
            if record is not None:
                records.append(record)

    records.sort(key=lambda row: row["path"])
    eligible = [row for row in records if row["eligible_gate_observation"]]
    feature_schemas = {}
    feature_schema_groups = {}
    for row in eligible:
        names = tuple(row["feature_names"])
        feature_schemas[names] = feature_schemas.get(names, 0) + 1
        feature_schema_groups.setdefault(names, set()).add(row["group_id"])
    largest_compatible_schema = max(feature_schemas.values(), default=0)
    largest_compatible_groups = max((len(groups) for groups in feature_schema_groups.values()), default=0)
    ready = any(
        count >= max(len(names) + 2, 6) and len(feature_schema_groups[names]) >= 2
        for names, count in feature_schemas.items()
    )
    summary = {
        "aggregate_outcome_files": len(records),
        "candidate_identity_rows": sum(row["candidate_identity_present"] for row in records),
        "group_id_rows": sum(row["group_id_present"] for row in records),
        "numeric_feature_map_rows": sum(row["numeric_feature_map_present"] for row in records),
        "gate_status_rows": sum(row["gate_status"] is not None for row in records),
        "uncertainty_interval_rows": sum(row["uncertainty_interval_present"] for row in records),
        "eligible_gate_observation_rows": len(eligible),
        "largest_compatible_feature_schema_rows": largest_compatible_schema,
        "largest_compatible_feature_schema_groups": largest_compatible_groups,
        "gate_model_fit_contract_ready": ready,
        "parse_errors": len(parse_errors),
    }
    return {
        "status": "ready" if ready else "not_estimable",
        "summary": summary,
        "records": records,
        "parse_errors": parse_errors,
        "aggregate_content_manifest_sha256": manifest_sha256(records),
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


def source_git_lineage(source_root, records):
    source_root = pathlib.Path(source_root)
    try:
        commit = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=source_root, text=True, stderr=subprocess.DEVNULL
        ).strip()
        dirty = bool(
            subprocess.check_output(
                ["git", "status", "--porcelain"],
                cwd=source_root,
                text=True,
                stderr=subprocess.DEVNULL,
            ).strip()
        )
        tracked = 0
        for row in records:
            result = subprocess.run(
                ["git", "ls-files", "--error-unmatch", row["path"]],
                cwd=source_root,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
            tracked += result.returncode == 0
        return commit, dirty, tracked
    except (OSError, subprocess.CalledProcessError):
        return "unavailable", None, 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("source_root")
    parser.add_argument("--out", required=True)
    parser.add_argument("--dataset-id", required=True)
    parser.add_argument("--dataset-source-commit", required=True)
    parser.add_argument("--min-games", type=float, default=20.0)
    args = parser.parse_args()

    audit = audit_history(args.source_root, args.min_games)
    commit, dirty = git_lineage()
    source_commit, source_dirty, tracked_files = source_git_lineage(
        args.source_root, audit["records"]
    )
    if source_commit != args.dataset_source_commit:
        raise ValueError(
            f"dataset source commit mismatch: expected {args.dataset_source_commit}, found {source_commit}"
        )
    report = {
        "protocol": "gate-history-readiness-audit-v1",
        "status": audit["status"],
        "decision": (
            "do_not_benchmark_or_expand_gate_model"
            if audit["status"] == "not_estimable"
            else "history_ready_for_declared_benchmark"
        ),
        "measurement": {
            "dataset_id": args.dataset_id,
            "dataset_source_commit": args.dataset_source_commit,
            "dataset_source_tree_dirty": source_dirty,
            "dataset_files_git_tracked": tracked_files,
            "dataset_files_content_pinned": len(audit["records"]),
            "lineprior_version": workspace_version(),
            "min_games": args.min_games,
            "runner_commit": commit,
            "runner_tree_dirty": dirty,
            "runner_path": "scripts/audit_gate_history_readiness.py",
            "runner_sha256": sha256_file(__file__),
            "aggregate_content_manifest_sha256": audit["aggregate_content_manifest_sha256"],
        },
        "audit": {
            "summary": audit["summary"],
            "records": audit["records"],
            "parse_errors": audit["parse_errors"],
        },
        "requested_comparisons": {
            "mean_baseline": "not_computable",
            "ridge_without_ood": "not_computable",
            "current_gate_model": "not_computable",
            "caller_rule": "not_computable",
            "verdict_calibration": "not_computable",
            "ood_abstention": "not_computable",
            "acquisition_efficiency": "not_computable",
        },
        "exclusion_rules": [
            "per-opening shard files are not independent candidates",
            "combined and final snapshots from one run must not become separate rows",
            "the same candidate under another baseline or time control is not a new candidate",
            "post-gate outcome fields must not be reused as pre-gate features",
        ],
        "required_next_history_fields": [
            "candidate_id",
            "group_id fixed before fitting",
            "shared numeric pre-gate feature map with feature schema version",
            "gate_elo_delta and games played",
            "Elo uncertainty interval or standard deviation",
            "PASS FAIL or INCONCLUSIVE terminal verdict",
            "caller rule decision and expected gate cost",
            "candidate baseline recipe dataset and code lineage",
        ],
        "notes": [
            "aggregate files are discovery candidates, not automatically independent training rows",
            "the source commit pins surrounding code; ignored result files are pinned by per-file hashes",
            "without candidate identity and group_id, group-aware splitting and leakage checks are impossible",
            "without pre-gate features, model, calibration, OOD, and acquisition comparisons are undefined",
        ],
    }
    pathlib.Path(args.out).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    try:
        main()
    except (OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"gate-history readiness audit error: {error}")
