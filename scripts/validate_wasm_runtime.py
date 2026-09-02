#!/usr/bin/env python3
"""Validate the CI artifact emitted by run_wasm_build_smoke.sh."""
import json
import pathlib
import re
import sys


def main():
    if len(sys.argv) != 2:
        raise ValueError("usage: validate_wasm_runtime.py wasm-runtime-report.json")
    report = json.loads(pathlib.Path(sys.argv[1]).read_text())
    if report.get("protocol") != "wasm-build-smoke-v1":
        raise ValueError("unexpected WASM smoke protocol")
    if report.get("project_version") != "0.11.1":
        raise ValueError("WASM report is not for fixed version 0.11.1")
    if not re.fullmatch(r"[0-9a-f]{40}", report.get("git_commit", "")):
        raise ValueError("git_commit must be a 40-character lowercase commit hash")
    if report.get("target") != "wasm32-unknown-unknown":
        raise ValueError("unexpected WASM target")
    if not isinstance(report.get("toolchain"), str) or not report["toolchain"].strip():
        raise ValueError("toolchain must be a non-empty string")
    if report.get("checks") != ["cargo-build-locked"]:
        raise ValueError("WASM check list changed unexpectedly")
    print("WASM runtime artifact contract: ok")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"WASM runtime artifact error: {error}")
