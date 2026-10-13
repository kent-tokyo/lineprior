#!/usr/bin/env python3
"""Regression tests for the candidate-ordering evaluation contract."""

import copy
import json
import pathlib
import re
import unittest

from validate_measurement_artifact import validate_candidate_ordering
from version_contract import workspace_version


ROOT = pathlib.Path(__file__).resolve().parent.parent


def not_ready_artifact():
    return json.loads(
        (ROOT / "tests" / "fixtures" / "candidate_ordering_not_ready.json").read_text()
    )


def completed_artifact():
    report = not_ready_artifact()
    report["measurement"]["input_sha256"]["off_results"] = "3" * 64
    report["measurement"]["input_sha256"]["on_results"] = "4" * 64
    usage = {
        "eligible_decisions": 10,
        "prior_selections": 2,
        "prior_abstentions": 8,
        "unavailable_fallbacks": 0,
        "selection_rate": 0.2,
        "fallback_reasons": {"unseen_state": 8},
        "passed_coverage_gate": True,
    }
    report["preflight"] = copy.deepcopy(usage)
    report["arms"] = {
        "off": {
            "cases": 10,
            "quality_estimate": 1.0,
            "quality_interval": {"lower": 0.9, "upper": 1.1},
            "cost_estimate": 100.0,
            "cost_interval": {"lower": 90.0, "upper": 110.0},
        },
        "on": {
            "cases": 10,
            "quality_estimate": 1.0,
            "quality_interval": {"lower": 0.9, "upper": 1.1},
            "cost_estimate": 90.0,
            "cost_interval": {"lower": 80.0, "upper": 100.0},
            "usage": usage,
        },
    }
    report["decision"] = {
        "status": "PASS",
        "quality_noninferiority_met": True,
        "cost_improvement_met": True,
        "rationale": "both preregistered gates passed",
    }
    return report


class CandidateOrderingValidatorTests(unittest.TestCase):
    def assert_message(self, report, expected):
        with self.assertRaisesRegex(ValueError, re.escape(expected)):
            validate_candidate_ordering(report, True, workspace_version())

    def test_not_ready_fixture_is_valid(self):
        validate_candidate_ordering(not_ready_artifact(), True, workspace_version())

    def test_completed_paired_artifact_is_valid(self):
        validate_candidate_ordering(completed_artifact(), True, workspace_version())

    def test_usage_counts_must_partition_decisions(self):
        report = not_ready_artifact()
        report["preflight"]["prior_abstentions"] = 9
        self.assert_message(
            report,
            "candidate-ordering.preflight decision counts must sum to eligible_decisions",
        )

    def test_selection_rate_must_match_counts(self):
        report = not_ready_artifact()
        report["preflight"]["selection_rate"] = 0.1
        self.assert_message(
            report,
            "candidate-ordering.preflight.selection_rate does not match the decision counts",
        )

    def test_boolean_selection_rate_is_rejected(self):
        report = not_ready_artifact()
        report["preflight"]["selection_rate"] = False
        self.assert_message(
            report,
            "candidate-ordering.preflight.selection_rate must be finite and in [0, 1]",
        )

    def test_selection_rate_relative_drift_is_not_accepted(self):
        report = completed_artifact()
        report["preflight"]["selection_rate"] = 0.2000000005
        self.assert_message(
            report,
            "candidate-ordering.preflight.selection_rate does not match the decision counts",
        )

    def test_unspecified_prior_fingerprint_is_rejected(self):
        report = not_ready_artifact()
        report["measurement"]["prior_config_fingerprint"] = " unspecified "
        self.assert_message(
            report,
            "candidate-ordering.measurement.prior_config_fingerprint must be explicit",
        )

    def test_failed_preflight_cannot_contain_full_evaluation(self):
        report = not_ready_artifact()
        report["arms"]["off"] = completed_artifact()["arms"]["off"]
        self.assert_message(
            report,
            "candidate-ordering arms must be null when the coverage preflight fails",
        )

    def test_pass_requires_both_declared_gates(self):
        report = completed_artifact()
        report["decision"]["cost_improvement_met"] = None
        self.assert_message(
            report,
            "candidate-ordering PASS requires both preregistered gates to pass",
        )

    def test_unknown_decision_status_is_rejected(self):
        report = completed_artifact()
        report["decision"]["status"] = "ADOPT"
        self.assert_message(
            report,
            "candidate-ordering.decision.status is unsupported",
        )


if __name__ == "__main__":
    unittest.main()
