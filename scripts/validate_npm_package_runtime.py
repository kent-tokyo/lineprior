#!/usr/bin/env python3
"""Validate the clean-install npm/WASM package smoke artifact."""

import json
import pathlib
import re
import sys

from version_contract import workspace_version


EXPECTED_CHECKS = [
    "wasm-pack-build-web",
    "license-files",
    "npm-pack",
    "clean-install",
    "browser-build-query-roundtrip",
    "deterministic-output",
    "error-shape",
]


def main():
    if len(sys.argv) != 2:
        raise ValueError("usage: validate_npm_package_runtime.py npm-package-runtime.json")
    report = json.loads(pathlib.Path(sys.argv[1]).read_text())
    if report.get("protocol") != "npm-wasm-package-smoke-v1":
        raise ValueError("unexpected npm/WASM smoke protocol")
    if report.get("package_name") != "lineprior-wasm":
        raise ValueError("unexpected npm package name")
    if report.get("package_version") != workspace_version():
        raise ValueError("npm package version does not match the workspace version")
    if not re.fullmatch(r"[0-9a-f]{40}", report.get("git_commit", "")):
        raise ValueError("git_commit must be a 40-character lowercase commit hash")
    if not isinstance(report.get("git_dirty"), bool):
        raise ValueError("git_dirty must be a boolean")
    if report.get("target") != "web":
        raise ValueError("unexpected wasm-pack target")
    for key in ("node_version", "npm_version", "wasm_pack_version"):
        if not isinstance(report.get(key), str) or not report[key].strip():
            raise ValueError(f"{key} must be a non-empty string")
    if not re.fullmatch(r"[0-9a-f]{64}", report.get("package_sha256", "")):
        raise ValueError("package_sha256 must be a lowercase SHA-256")
    if report.get("checks") != EXPECTED_CHECKS:
        raise ValueError("npm/WASM check list changed unexpectedly")
    print("npm/WASM package artifact contract: ok")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"npm/WASM runtime artifact error: {error}")
