#!/usr/bin/env python3
"""Validate a negative or ready off-policy log-readiness artifact."""

import argparse
import hashlib
import json
import pathlib
import re

from version_contract import workspace_version


ROOT = pathlib.Path(__file__).resolve().parent.parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("report")
    parser.add_argument("--expected-lineprior-version", default=workspace_version())
    args = parser.parse_args()
    report = json.loads(pathlib.Path(args.report).read_text())
    require(report.get("protocol") == "offpolicy-readiness-audit-v1", "unexpected protocol")
    require(
        report["measurement"].get("lineprior_version") == args.expected_lineprior_version,
        "version mismatch",
    )
    require(report.get("status") in ("ready", "not_estimable"), "unexpected status")
    audit = report["audit"]
    if report["status"] == "not_estimable":
        require(not audit["causal_claim_permitted"], "non-estimable logs cannot permit a causal claim")
        require(audit["effective_sample_size"] is None, "ESS must be absent when support is unknown")
        require(report["decision"] == "do_not_run_ips_dr_or_make_causal_claim", "unsafe decision")
    require(audit["fields_present"]["decision_rows"] > 0, "audit contains no decisions")

    measurement = report["measurement"]
    runner = (ROOT / measurement["runner_path"]).resolve()
    require(ROOT in runner.parents and runner.is_file(), "runner path is missing or outside repository")
    require(hashlib.sha256(runner.read_bytes()).hexdigest() == measurement["runner_sha256"], "runner hash mismatch")
    for name, digest in measurement["input_sha256"].items():
        require(bool(re.fullmatch(r"[0-9a-f]{64}", digest)), f"invalid input hash for {name}")
    print("off-policy readiness artifact contract: ok")


if __name__ == "__main__":
    try:
        main()
    except (KeyError, OSError, TypeError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"off-policy readiness artifact error: {error}")
