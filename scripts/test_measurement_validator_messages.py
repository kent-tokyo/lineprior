#!/usr/bin/env python3
"""Regression tests for stable semantic measurement-validator diagnostics."""
import copy
import re
import unittest

from validate_measurement_artifact import validate_offpolicy


def artifact():
    measurement = {
        "dataset_id": "fixture-v1",
        "split": "heldout",
        "lineprior_version": "0.11.1",
        "policy_version": "policy-v1",
        "input_sha256": {"off": "0" * 64, "on": "1" * 64},
    }
    arm = {
        "ips": {
            "ips": 0.4,
            "self_normalized_ips": 0.4,
            "support_fraction": 1.0,
            "effective_sample_size": 2.0,
        },
        "doubly_robust": {"estimate": 0.4},
        "bootstrap": {},
    }
    return {
        "protocol": "offpolicy-integrated-arms-v1",
        "measurement": measurement,
        "arms": {"off": arm, "on": copy.deepcopy(arm)},
        "paired": {
            "protocol": "offpolicy-paired-arms-v1",
            "measurement": copy.deepcopy(measurement),
            "paired_rows": 2,
            "off": {},
            "on": {},
        },
    }


class OffpolicyValidatorMessageTests(unittest.TestCase):
    def assert_message(self, report, expected, require_explicit=False):
        with self.assertRaisesRegex(ValueError, re.escape(expected)):
            validate_offpolicy(report, require_explicit)

    def test_support_fraction_message_is_stable(self):
        report = artifact()
        report["arms"]["off"]["ips"]["support_fraction"] = 1.5
        self.assert_message(
            report,
            "offpolicy.arms.off.ips.support_fraction must be finite and in [0, 1]",
        )

    def test_paired_lineage_message_is_stable(self):
        report = artifact()
        report["paired"]["measurement"]["dataset_id"] = "other-fixture"
        self.assert_message(
            report,
            "offpolicy paired dataset_id does not match the integrated report",
        )

    def test_paired_hash_message_is_stable(self):
        report = artifact()
        report["paired"]["measurement"]["input_sha256"]["on"] = "2" * 64
        self.assert_message(
            report,
            "offpolicy paired input hashes do not match the integrated report",
        )

    def test_explicit_policy_lineage_message_is_stable(self):
        report = artifact()
        report["measurement"]["policy_version"] = "unspecified"
        self.assert_message(
            report,
            "offpolicy.measurement.policy_version must be explicit when required",
            require_explicit=True,
        )


if __name__ == "__main__":
    unittest.main()
