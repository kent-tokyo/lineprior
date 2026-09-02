#!/usr/bin/env python3
"""Validate the CI artifact emitted by run_ecosystem_matrix_smoke.sh."""
import json
import pathlib
import re
import sys


def main():
    if len(sys.argv) != 2:
        raise ValueError("usage: validate_ecosystem_runtime.py runtime-report.json")
    report = json.loads(pathlib.Path(sys.argv[1]).read_text())
    if report.get("protocol") != "ecosystem-matrix-smoke-v1":
        raise ValueError("unexpected ecosystem runtime protocol")
    if report.get("project_version") != "0.11.1":
        raise ValueError("ecosystem runtime report is not for fixed version 0.11.1")
    if not re.fullmatch(r"[0-9a-f]{40}", report.get("git_commit", "")):
        raise ValueError("git_commit must be a 40-character lowercase commit hash")
    runtimes = report.get("runtimes")
    if not isinstance(runtimes, dict) or set(runtimes) != {"rustc", "cargo", "python", "node"}:
        raise ValueError("runtime inventory must contain rustc, cargo, python, and node")
    if any(not isinstance(value, str) or not value.strip() for value in runtimes.values()):
        raise ValueError("runtime inventory values must be non-empty strings")
    matrix = report.get("matrix")
    if matrix is not None:
        if not isinstance(matrix, dict) or set(matrix) != {"python", "node"}:
            raise ValueError("matrix must contain python and node when present")
        if not re.fullmatch(r"\d+\.\d+", matrix["python"]):
            raise ValueError("matrix python must be a major.minor version")
        if not re.fullmatch(r"\d+", matrix["node"]):
            raise ValueError("matrix node must be a major version")
        actual_python = re.search(r"Python (\d+\.\d+)", runtimes["python"])
        actual_node = re.search(r"v(\d+)(?:\.|$)", runtimes["node"])
        if actual_python is None or actual_python.group(1) != matrix["python"]:
            raise ValueError("matrix python does not match the recorded runtime")
        if actual_node is None or actual_node.group(1) != matrix["node"]:
            raise ValueError("matrix node does not match the recorded runtime")
    expected_checks = ["cli-roundtrip", "node-example", "python-example", "offpolicy", "measurement"]
    if report.get("checks") != expected_checks:
        raise ValueError("ecosystem check list changed unexpectedly")
    print("ecosystem runtime artifact contract: ok")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"ecosystem runtime artifact error: {error}")
