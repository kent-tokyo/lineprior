#!/usr/bin/env python3
"""Validate the stable envelope of a lineprior measurement artifact.

This checks report shape and lineage only. It does not turn fixture output into
real-data evidence or decide whether a quality gate should pass.
"""
import argparse
import hashlib
import json
import math
import pathlib

from version_contract import workspace_version

EXPECTED_VERSION = workspace_version()
ROOT = pathlib.Path(__file__).resolve().parent.parent


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
    if (
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(float(value))
        or not 0.0 <= float(value) <= 1.0
    ):
        raise ValueError(f"{label} must be finite and in [0, 1]")


def validate_nonnegative_integer(value, label):
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"{label} must be a non-negative integer")


def validate_finite_number(value, label):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(float(value)):
        raise ValueError(f"{label} must be finite")


def validate_nonnegative_number(value, label):
    if (
        isinstance(value, bool)
        or not isinstance(value, (int, float))
        or not math.isfinite(float(value))
        or value < 0
    ):
        raise ValueError(f"{label} must be finite and non-negative")


def is_explicit_string(value):
    return (
        isinstance(value, str)
        and bool(value.strip())
        and value.strip() != "unspecified"
    )


def validate_lineage(report, label, require_explicit, expected_version=EXPECTED_VERSION):
    require(report, ("protocol", "measurement"), label)
    measurement = report["measurement"]
    required = ["dataset_id", "split", "lineprior_version", "input_sha256"]
    if label == "similarity":
        required.append("prior_config_fingerprint")
    require(measurement, required, f"{label}.measurement")
    if measurement["lineprior_version"] != expected_version:
        raise ValueError(
            f"{label}.measurement.lineprior_version must be {expected_version}"
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
                if (
                    isinstance(value, bool)
                    or not isinstance(value, (str, int))
                    or isinstance(value, str)
                    and not is_explicit_string(value)
                ):
                    raise ValueError(f"{label}.measurement.{key} must be explicit when required")
                continue
            if not is_explicit_string(value):
                raise ValueError(f"{label}.measurement.{key} must be explicit when required")


def validate_similarity(report, require_explicit, expected_version=EXPECTED_VERSION):
    validate_lineage(report, "similarity", require_explicit, expected_version)
    if report["protocol"] != "similarity-real-data-v1":
        raise ValueError("unexpected similarity protocol")
    require(report, ("num_queries", "arms"), "similarity")
    validate_nonnegative_integer(report["num_queries"], "similarity.num_queries")
    measurement = report["measurement"]
    for path_key, hash_key in (
        ("runner_path", "runner_sha256"),
        ("feature_adapter_path", "feature_adapter_sha256"),
    ):
        if measurement.get(path_key) is not None or measurement.get(hash_key) is not None:
            path_value = measurement.get(path_key)
            digest = measurement.get(hash_key)
            if not isinstance(path_value, str) or not path_value:
                raise ValueError(f"similarity.measurement.{path_key} must be a repository-relative path")
            path = (ROOT / path_value).resolve()
            if ROOT not in path.parents or not path.is_file():
                raise ValueError(f"similarity.measurement.{path_key} is outside the repository or missing")
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
            if digest != actual:
                raise ValueError(f"similarity.measurement.{hash_key} does not match {path_key}")
    arms = report["arms"]
    require(arms, (), "similarity.arms")
    for name in ("exact", "similarity", "no_prior"):
        require(
            arms.get(name),
            ("coverage", "abstention_rate", "top1_hit_rate", "mrr", "calibration_brier"),
            f"similarity.arms.{name}",
        )
        arm = arms[name]
        for metric in ("coverage", "abstention_rate", "top1_hit_rate", "mrr", "calibration_brier"):
            validate_unit_interval(arm[metric], f"similarity.arms.{name}.{metric}")
        for metric in ("top1_hit_rate_covered", "false_recommendation_rate_covered"):
            if metric in arm:
                validate_unit_interval(arm[metric], f"similarity.arms.{name}.{metric}")
        for metric in ("latency_us_p50", "latency_us_p95", "peak_rss_kb"):
            if arm.get(metric) is not None:
                validate_nonnegative_number(
                    arm[metric], f"similarity.arms.{name}.{metric}"
                )


def validate_offpolicy(report, require_explicit, expected_version=EXPECTED_VERSION):
    validate_lineage(report, "offpolicy", require_explicit, expected_version)
    if report["protocol"] != "offpolicy-integrated-arms-v1":
        raise ValueError("unexpected integrated off-policy protocol")
    require(report, ("arms", "paired"), "offpolicy")
    require(report["arms"], (), "offpolicy.arms")
    for name in ("off", "on"):
        arm = report["arms"].get(name)
        require(arm, ("ips", "doubly_robust", "bootstrap"), f"offpolicy.arms.{name}")
        require(arm["ips"], ("ips", "self_normalized_ips"), f"offpolicy.arms.{name}.ips")
        validate_unit_interval(arm["ips"].get("support_fraction"), f"offpolicy.arms.{name}.ips.support_fraction")
        if arm["ips"].get("effective_sample_size") is not None:
            validate_nonnegative_number(
                arm["ips"]["effective_sample_size"],
                f"offpolicy.arms.{name}.ips.effective_sample_size",
            )
        for estimate_name in ("ips", "doubly_robust"):
            estimate = arm[estimate_name]
            value_key = "estimate" if estimate_name == "doubly_robust" else "ips"
            require(estimate, (value_key,), f"offpolicy.arms.{name}.{estimate_name}")
            if estimate[value_key] is not None:
                validate_finite_number(
                    estimate[value_key],
                    f"offpolicy.arms.{name}.{estimate_name}.{value_key}",
                )
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


def validate_candidate_usage(usage, preregistration, label):
    keys = (
        "eligible_decisions",
        "prior_selections",
        "prior_abstentions",
        "unavailable_fallbacks",
        "selection_rate",
        "fallback_reasons",
        "passed_coverage_gate",
    )
    require(usage, keys, label)
    for key in keys[:4]:
        validate_nonnegative_integer(usage[key], f"{label}.{key}")
    if (
        usage["prior_selections"]
        + usage["prior_abstentions"]
        + usage["unavailable_fallbacks"]
        != usage["eligible_decisions"]
    ):
        raise ValueError(f"{label} decision counts must sum to eligible_decisions")
    expected_rate = (
        usage["prior_selections"] / usage["eligible_decisions"]
        if usage["eligible_decisions"]
        else 0.0
    )
    validate_unit_interval(usage["selection_rate"], f"{label}.selection_rate", allow_none=False)
    if not math.isclose(
        float(usage["selection_rate"]), expected_rate, rel_tol=0.0, abs_tol=1e-12
    ):
        raise ValueError(f"{label}.selection_rate does not match the decision counts")
    reasons = usage["fallback_reasons"]
    require(reasons, (), f"{label}.fallback_reasons")
    for name, count in reasons.items():
        if not isinstance(name, str) or not name:
            raise ValueError(f"{label}.fallback_reasons keys must be non-empty")
        validate_nonnegative_integer(count, f"{label}.fallback_reasons.{name}")
    if sum(reasons.values()) != usage["prior_abstentions"] + usage["unavailable_fallbacks"]:
        raise ValueError(f"{label}.fallback_reasons must account for every fallback")
    expected_pass = (
        usage["prior_selections"] >= preregistration["minimum_prior_selections"]
        and usage["selection_rate"] >= preregistration["minimum_prior_selection_rate"]
    )
    if not isinstance(usage["passed_coverage_gate"], bool) or usage["passed_coverage_gate"] != expected_pass:
        raise ValueError(f"{label}.passed_coverage_gate does not match the preregistered thresholds")


def validate_candidate_arm(arm, label):
    require(
        arm,
        ("cases", "quality_estimate", "quality_interval", "cost_estimate", "cost_interval"),
        label,
    )
    validate_nonnegative_integer(arm["cases"], f"{label}.cases")
    if arm["cases"] == 0:
        raise ValueError(f"{label}.cases must be positive")
    for metric in ("quality", "cost"):
        estimate = arm[f"{metric}_estimate"]
        interval = arm[f"{metric}_interval"]
        validate_finite_number(estimate, f"{label}.{metric}_estimate")
        require(interval, ("lower", "upper"), f"{label}.{metric}_interval")
        validate_finite_number(interval["lower"], f"{label}.{metric}_interval.lower")
        validate_finite_number(interval["upper"], f"{label}.{metric}_interval.upper")
        if interval["lower"] > estimate or estimate > interval["upper"]:
            raise ValueError(f"{label}.{metric}_interval must contain its estimate")


def validate_candidate_ordering(report, require_explicit, expected_version=EXPECTED_VERSION):
    require(
        report,
        ("protocol", "measurement", "preregistration", "preflight", "arms", "decision"),
        "candidate-ordering",
    )
    if report["protocol"] != "candidate-ordering-evaluation-v1":
        raise ValueError("unexpected candidate-ordering protocol")
    measurement = report["measurement"]
    measurement_keys = (
        "dataset_id",
        "split",
        "lineprior_version",
        "prior_config_fingerprint",
        "downstream_commit",
        "toolchain",
        "hardware",
        "seed",
        "input_sha256",
    )
    require(measurement, measurement_keys, "candidate-ordering.measurement")
    if measurement["lineprior_version"] != expected_version:
        raise ValueError(
            f"candidate-ordering.measurement.lineprior_version must be {expected_version}"
        )
    validate_nonnegative_integer(measurement["seed"], "candidate-ordering.measurement.seed")
    fingerprint = measurement["prior_config_fingerprint"]
    if (
        isinstance(fingerprint, bool)
        or not isinstance(fingerprint, (str, int))
        or isinstance(fingerprint, str)
        and not is_explicit_string(fingerprint)
        or isinstance(fingerprint, int)
        and fingerprint < 0
    ):
        raise ValueError("candidate-ordering.measurement.prior_config_fingerprint must be explicit")
    if require_explicit:
        for key in ("dataset_id", "split", "downstream_commit", "toolchain", "hardware"):
            value = measurement[key]
            if not is_explicit_string(value):
                raise ValueError(
                    f"candidate-ordering.measurement.{key} must be explicit when required"
                )
    hashes = measurement["input_sha256"]
    hash_keys = ("observations", "heldout_cases", "prior_book", "off_results", "on_results")
    require(hashes, hash_keys, "candidate-ordering.measurement.input_sha256")
    for key in hash_keys:
        value = hashes[key]
        if value is None and key in ("off_results", "on_results"):
            continue
        if not isinstance(value, str) or len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
            raise ValueError(
                f"candidate-ordering.measurement.input_sha256.{key} must be a lowercase SHA-256 hex digest"
            )

    preregistration = report["preregistration"]
    preregistration_keys = (
        "quality_metric",
        "quality_direction",
        "quality_noninferiority_margin",
        "cost_metric",
        "cost_direction",
        "minimum_prior_selections",
        "minimum_prior_selection_rate",
        "maximum_case_budget",
        "uncertainty_method",
        "decision_rule",
    )
    require(preregistration, preregistration_keys, "candidate-ordering.preregistration")
    for key in ("quality_metric", "cost_metric", "uncertainty_method", "decision_rule"):
        value = preregistration[key]
        if not isinstance(value, str) or not value.strip():
            raise ValueError(f"candidate-ordering.preregistration.{key} must be non-empty")
    for key in ("quality_direction", "cost_direction"):
        if preregistration[key] not in ("higher", "lower"):
            raise ValueError(
                f"candidate-ordering.preregistration.{key} must be higher or lower"
            )
    validate_finite_number(
        preregistration["quality_noninferiority_margin"],
        "candidate-ordering.preregistration.quality_noninferiority_margin",
    )
    if preregistration["quality_noninferiority_margin"] < 0:
        raise ValueError(
            "candidate-ordering.preregistration.quality_noninferiority_margin must be non-negative"
        )
    validate_nonnegative_integer(
        preregistration["minimum_prior_selections"],
        "candidate-ordering.preregistration.minimum_prior_selections",
    )
    if preregistration["minimum_prior_selections"] == 0:
        raise ValueError(
            "candidate-ordering.preregistration.minimum_prior_selections must be positive"
        )
    validate_unit_interval(
        preregistration["minimum_prior_selection_rate"],
        "candidate-ordering.preregistration.minimum_prior_selection_rate",
        allow_none=False,
    )
    validate_nonnegative_integer(
        preregistration["maximum_case_budget"],
        "candidate-ordering.preregistration.maximum_case_budget",
    )
    if preregistration["maximum_case_budget"] == 0:
        raise ValueError("candidate-ordering.preregistration.maximum_case_budget must be positive")

    validate_candidate_usage(report["preflight"], preregistration, "candidate-ordering.preflight")
    arms = report["arms"]
    require(arms, ("off", "on"), "candidate-ordering.arms")
    decision = report["decision"]
    require(
        decision,
        ("status", "quality_noninferiority_met", "cost_improvement_met", "rationale"),
        "candidate-ordering.decision",
    )
    if decision["status"] not in ("NOT_READY", "PASS", "FAIL", "INCONCLUSIVE"):
        raise ValueError("candidate-ordering.decision.status is unsupported")
    for key in ("quality_noninferiority_met", "cost_improvement_met"):
        if decision[key] is not None and not isinstance(decision[key], bool):
            raise ValueError(f"candidate-ordering.decision.{key} must be boolean or null")
    if not isinstance(decision["rationale"], str) or not decision["rationale"].strip():
        raise ValueError("candidate-ordering.decision.rationale must be non-empty")
    passed = report["preflight"]["passed_coverage_gate"]
    if not passed:
        if arms["off"] is not None or arms["on"] is not None:
            raise ValueError("candidate-ordering arms must be null when the coverage preflight fails")
        if hashes["off_results"] is not None or hashes["on_results"] is not None:
            raise ValueError("candidate-ordering result hashes must be null when evaluation was not run")
        if decision["status"] != "NOT_READY":
            raise ValueError("candidate-ordering failed preflight must have NOT_READY status")
        if (
            decision["quality_noninferiority_met"] is not None
            or decision["cost_improvement_met"] is not None
        ):
            raise ValueError("candidate-ordering NOT_READY gates must be null")
        return

    if arms["off"] is None or arms["on"] is None:
        raise ValueError("candidate-ordering arms are required after a passed preflight")
    if hashes["off_results"] is None or hashes["on_results"] is None:
        raise ValueError("candidate-ordering result hashes are required after evaluation")
    validate_candidate_arm(arms["off"], "candidate-ordering.arms.off")
    validate_candidate_arm(arms["on"], "candidate-ordering.arms.on")
    if arms["off"]["cases"] != arms["on"]["cases"]:
        raise ValueError("candidate-ordering paired arms must contain the same number of cases")
    if arms["off"]["cases"] > preregistration["maximum_case_budget"]:
        raise ValueError("candidate-ordering paired arms exceed the preregistered case budget")
    if "usage" in arms["off"]:
        raise ValueError("candidate-ordering.arms.off must not report prior usage")
    require(arms["on"], ("usage",), "candidate-ordering.arms.on")
    validate_candidate_usage(
        arms["on"]["usage"], preregistration, "candidate-ordering.arms.on.usage"
    )
    status = decision["status"]
    quality = decision["quality_noninferiority_met"]
    cost = decision["cost_improvement_met"]
    if status == "PASS" and (quality is not True or cost is not True):
        raise ValueError("candidate-ordering PASS requires both preregistered gates to pass")
    if status == "FAIL" and quality is not False and cost is not False:
        raise ValueError("candidate-ordering FAIL requires at least one failed gate")
    if status == "INCONCLUSIVE" and (
        quality is False or cost is False or (quality is not None and cost is not None)
    ):
        raise ValueError("candidate-ordering INCONCLUSIVE requires an unresolved gate and no failed gate")
    if status == "NOT_READY":
        raise ValueError("candidate-ordering passed preflight cannot have NOT_READY status")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=("similarity", "offpolicy", "candidate-ordering"))
    parser.add_argument("artifact")
    parser.add_argument("--require-explicit-lineage", action="store_true")
    parser.add_argument("--expected-lineprior-version", default=EXPECTED_VERSION)
    args = parser.parse_args()
    report = json.loads(pathlib.Path(args.artifact).read_text())
    if args.kind == "similarity":
        validate_similarity(report, args.require_explicit_lineage, args.expected_lineprior_version)
    elif args.kind == "offpolicy":
        validate_offpolicy(report, args.require_explicit_lineage, args.expected_lineprior_version)
    else:
        validate_candidate_ordering(
            report, args.require_explicit_lineage, args.expected_lineprior_version
        )
    print(f"measurement artifact contract: ok ({args.kind})")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"measurement artifact error: {error}")
