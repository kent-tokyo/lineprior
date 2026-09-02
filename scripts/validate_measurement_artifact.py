#!/usr/bin/env python3
"""Validate the stable envelope of a lineprior measurement artifact.

This checks report shape and lineage only. It does not turn fixture output into
real-data evidence or decide whether a quality gate should pass.
"""
import argparse
import json
import math
import pathlib

EXPECTED_VERSION = "0.11.1"


def require(mapping, keys, label):
    if not isinstance(mapping, dict):
        raise ValueError(f"{label} must be an object")
    missing = [key for key in keys if key not in mapping]
    if missing:
        raise ValueError(f"{label}: missing {', '.join(missing)}")


def validate_hashes(measurement, expected_keys, label):
    hashes = measurement["input_sha256"]
    require(hashes, expected_keys, f"{label}.input_sha256")
    for key in expected_keys:
        value = hashes[key]
        if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
            raise ValueError(f"{label}.input_sha256.{key} must be a lowercase SHA-256 hex digest")


def validate_unit_interval(value, label, allow_none=True):
    if value is None and allow_none:
        return
    if not isinstance(value, (int, float)) or not math.isfinite(float(value)) or not 0.0 <= float(value) <= 1.0:
        raise ValueError(f"{label} must be finite and in [0, 1]")


def validate_nonnegative_integer(value, label):
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"{label} must be a non-negative integer")


def validate_lineage(report, label, require_explicit):
    require(report, ("protocol", "measurement"), label)
    measurement = report["measurement"]
    required = ["dataset_id", "split", "lineprior_version", "input_sha256"]
    if label == "similarity":
        required.append("prior_config_fingerprint")
    require(measurement, required, f"{label}.measurement")
    if measurement["lineprior_version"] != EXPECTED_VERSION:
        raise ValueError(
            f"{label}.measurement.lineprior_version must be {EXPECTED_VERSION}"
        )
    validate_hashes(measurement, ("prior", "queries") if label == "similarity" else ("off", "on"), label)
    if require_explicit:
        explicit = ["dataset_id", "split"]
        if label == "similarity":
            explicit += ["feature_version", "prior_config_fingerprint"]
        else:
            explicit.append("policy_version")
        for key in explicit:
            value = measurement.get(key)
            if key == "prior_config_fingerprint":
                if isinstance(value, bool) or not isinstance(value, (str, int)) or value == "unspecified":
                    raise ValueError(f"{label}.measurement.{key} must be explicit when required")
                continue
            if not isinstance(value, str) or not value.strip() or value == "unspecified":
                raise ValueError(f"{label}.measurement.{key} must be explicit when required")


def validate_similarity(report, require_explicit):
    validate_lineage(report, "similarity", require_explicit)
    if report["protocol"] != "similarity-real-data-v1":
        raise ValueError("unexpected similarity protocol")
    require(report, ("num_queries", "arms"), "similarity")
    validate_nonnegative_integer(report["num_queries"], "similarity.num_queries")
    arms = report["arms"]
    for name in ("exact", "similarity", "no_prior"):
        require(
            arms.get(name),
            ("coverage", "abstention_rate", "top1_hit_rate", "mrr", "calibration_brier"),
            f"similarity.arms.{name}",
        )
        arm = arms[name]
        for metric in ("coverage", "abstention_rate", "top1_hit_rate", "mrr", "calibration_brier"):
            validate_unit_interval(arm[metric], f"similarity.arms.{name}.{metric}")


def validate_offpolicy(report, require_explicit):
    validate_lineage(report, "offpolicy", require_explicit)
    if report["protocol"] != "offpolicy-integrated-arms-v1":
        raise ValueError("unexpected integrated off-policy protocol")
    require(report, ("arms", "paired"), "offpolicy")
    for name in ("off", "on"):
        arm = report["arms"].get(name)
        require(arm, ("ips", "doubly_robust", "bootstrap"), f"offpolicy.arms.{name}")
        require(arm["ips"], ("ips", "self_normalized_ips"), f"offpolicy.arms.{name}.ips")
        validate_unit_interval(arm["ips"].get("support_fraction"), f"offpolicy.arms.{name}.ips.support_fraction")
        if arm["ips"].get("effective_sample_size") is not None and float(arm["ips"]["effective_sample_size"]) < 0:
            raise ValueError(f"offpolicy.arms.{name}.ips.effective_sample_size must be non-negative")
        for estimate_name in ("ips", "doubly_robust"):
            estimate = arm[estimate_name]
            value_key = "estimate" if estimate_name == "doubly_robust" else "ips"
            require(estimate, (value_key,), f"offpolicy.arms.{name}.{estimate_name}")
            if estimate[value_key] is not None and not math.isfinite(float(estimate[value_key])):
                raise ValueError(f"offpolicy.arms.{name}.{estimate_name}.{value_key} must be finite")
    paired = report["paired"]
    require(paired, ("protocol", "measurement", "paired_rows", "off", "on"), "offpolicy.paired")
    if paired["protocol"] != "offpolicy-paired-arms-v1":
        raise ValueError("unexpected paired off-policy protocol")
    validate_nonnegative_integer(paired["paired_rows"], "offpolicy.paired.paired_rows")
    paired_measurement = paired["measurement"]
    require(paired_measurement, ("dataset_id", "split", "lineprior_version", "input_sha256"), "offpolicy.paired.measurement")
    validate_hashes(paired_measurement, ("off", "on"), "offpolicy.paired")
    for key in ("dataset_id", "split", "lineprior_version"):
        if paired_measurement[key] != report["measurement"][key]:
            raise ValueError(f"offpolicy paired {key} does not match the integrated report")
    if require_explicit:
        for key in ("dataset_id", "split"):
            value = paired_measurement[key]
            if not isinstance(value, str) or not value.strip() or value == "unspecified":
                raise ValueError(f"offpolicy.paired.measurement.{key} must be explicit when required")
    if paired["measurement"]["input_sha256"] != report["measurement"]["input_sha256"]:
        raise ValueError("offpolicy paired input hashes do not match the integrated report")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=("similarity", "offpolicy"))
    parser.add_argument("artifact")
    parser.add_argument("--require-explicit-lineage", action="store_true")
    args = parser.parse_args()
    report = json.loads(pathlib.Path(args.artifact).read_text())
    if args.kind == "similarity":
        validate_similarity(report, args.require_explicit_lineage)
    else:
        validate_offpolicy(report, args.require_explicit_lineage)
    print(f"measurement artifact contract: ok ({args.kind})")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"measurement artifact error: {error}")
