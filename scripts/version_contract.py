#!/usr/bin/env python3
"""Read and validate lineprior's version contract from one source of truth."""

import argparse
import json
import pathlib
import tomllib


ROOT = pathlib.Path(__file__).resolve().parent.parent
WORKSPACE_CRATES = {
    "lineprior",
    "lineprior-adapters",
    "lineprior-cli",
    "lineprior-similarity",
    "lineprior-wasm",
}


def read_toml(path):
    return tomllib.loads(path.read_text())


def workspace_version(root=ROOT):
    return read_toml(root / "Cargo.toml")["workspace"]["package"]["version"]


def require_equal(actual, expected, label):
    if actual != expected:
        raise ValueError(f"{label}: expected {expected!r}, found {actual!r}")


def check_manifest_versions(expected, root=ROOT):
    root_manifest = read_toml(root / "Cargo.toml")
    members = root_manifest["workspace"]["members"]
    package_names = set()
    for member in members:
        manifest = read_toml(root / member / "Cargo.toml")
        package = manifest["package"]
        package_names.add(package["name"])
        require_equal(package.get("version", {}).get("workspace"), True, f"{member} package.version.workspace")
        dependency = manifest.get("dependencies", {}).get("lineprior")
        if dependency is not None:
            require_equal(dependency.get("version"), expected, f"{member} lineprior dependency")
    require_equal(package_names, WORKSPACE_CRATES, "workspace crate set")

    lock = read_toml(root / "Cargo.lock")
    locked = {
        package["name"]: package["version"]
        for package in lock["package"]
        if package["name"] in WORKSPACE_CRATES
    }
    require_equal(locked, {name: expected for name in WORKSPACE_CRATES}, "Cargo.lock workspace versions")


def check_schema_versions(root=ROOT):
    for name in (
        "similarity-real-data-v1.schema.json",
        "offpolicy-integrated-arms-v1.schema.json",
    ):
        schema = json.loads((root / "docs" / "measurements" / name).read_text())
        version = schema["$defs"]["measurement"]["properties"]["lineprior_version"]
        require_equal(version.get("type"), "string", f"{name} lineprior_version type")
        require_equal(
            version.get("pattern"),
            r"^[0-9]+\.[0-9]+\.[0-9]+$",
            f"{name} lineprior_version pattern",
        )


def check_fixture_versions(expected, root=ROOT):
    manifest = json.loads((root / "examples" / "veridict_prior_comparison.json").read_text())
    require_equal(manifest["lineprior_version"], expected, "veridict fixture lineprior_version")

    for name in ("prior.jsonl", "shogi_prior.jsonl"):
        first_line = next(
            line for line in (root / "examples" / name).read_text().splitlines() if line.strip()
        )
        header = json.loads(first_line)
        require_equal(header["producer_version"], expected, f"{name} producer_version")


def check_document_versions(expected, root=ROOT):
    required_mentions = {
        "README.md": f'"producer_version":"{expected}"',
        "README_ja.md": f'"producer_version":"{expected}"',
        "docs/measurements/ecosystem-compatibility.md": f"`lineprior` `{expected}`",
        "docs/publishing.md": f"Version `{expected}`",
        "examples/veridict_prior_comparison.md": f"(`{expected}`)",
    }
    for name, marker in required_mentions.items():
        if marker not in (root / name).read_text():
            raise ValueError(f"{name}: current version marker is missing or stale")


def check_evidence_versions(root=ROOT):
    evidence = {
        "Sekirei case study": (
            "docs/case-studies/sekirei-opening-book-2026-10-03.json",
            ("lineage", "lineprior_version"),
            "0.12.0",
        ),
        "Sekirei similarity": (
            "docs/measurements/similarity-sekirei-2026-10-03.json",
            ("measurement", "lineprior_version"),
            "0.12.0",
        ),
        "Sekirei off-policy readiness": (
            "docs/measurements/offpolicy-sekirei-readiness-2026-10-03.json",
            ("measurement", "lineprior_version"),
            "0.12.0",
        ),
        "Sekirei GateModel readiness": (
            "docs/measurements/gate-history-sekirei-readiness-2026-10-03.json",
            ("measurement", "lineprior_version"),
            "0.12.0",
        ),
    }
    for label, (name, keys, expected) in evidence.items():
        value = json.loads((root / name).read_text())
        for key in keys:
            value = value[key]
        require_equal(value, expected, f"{label} lineprior_version")


def check_version_contract(root=ROOT):
    expected = workspace_version(root)
    check_manifest_versions(expected, root)
    check_schema_versions(root)
    check_fixture_versions(expected, root)
    check_document_versions(expected, root)
    check_evidence_versions(root)
    return expected


def main():
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--print", action="store_true", dest="print_version")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.print_version:
        print(workspace_version())
        return
    version = check_version_contract()
    print(f"version contract: ok ({version}, {len(WORKSPACE_CRATES)} workspace crates)")


if __name__ == "__main__":
    try:
        main()
    except (KeyError, OSError, StopIteration, TypeError, ValueError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        raise SystemExit(f"version contract error: {error}")
