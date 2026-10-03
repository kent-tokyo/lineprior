#!/usr/bin/env python3
"""Regression tests for the off-policy readiness audit."""

import unittest

from audit_offpolicy_readiness import audit_rows


class OffpolicyReadinessAuditTests(unittest.TestCase):
    def test_realistic_search_row_is_not_estimable(self):
        audit = audit_rows([{"type": "search", "sfen": "s", "bestmove_csa": "a", "score_cp": 10}])
        self.assertEqual(audit["ips_status"], "not_estimable")
        self.assertEqual(audit["fields_present"]["logging_propensity"], 0)
        self.assertFalse(audit["causal_claim_permitted"])

    def test_complete_ips_row_is_ready_but_dr_needs_model_values(self):
        audit = audit_rows([
            {
                "state": "s",
                "action": "a",
                "reward": 1.0,
                "logging_propensity": 0.5,
                "evaluation_probability": 0.25,
            }
        ])
        self.assertEqual(audit["ips_status"], "ready")
        self.assertEqual(audit["dr_status"], "not_estimable")

    def test_invalid_propensity_blocks_estimation(self):
        audit = audit_rows([
            {
                "state": "s",
                "action": "a",
                "reward": 1.0,
                "logging_propensity": 0.0,
                "evaluation_probability": 0.25,
            }
        ])
        self.assertEqual(audit["invalid_propensity_pairs"], 1)
        self.assertEqual(audit["ips_status"], "not_estimable")


if __name__ == "__main__":
    unittest.main()
