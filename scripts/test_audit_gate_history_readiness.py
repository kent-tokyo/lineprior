#!/usr/bin/env python3
"""Regression tests for the GateModel real-history readiness audit."""

import json
import pathlib
import tempfile
import unittest

from audit_gate_history_readiness import audit_history


def write_json(root, relative, value):
    path = pathlib.Path(root) / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value) + "\n")


class GateHistoryReadinessAuditTests(unittest.TestCase):
    def test_outcome_without_pre_gate_contract_is_not_estimable(self):
        with tempfile.TemporaryDirectory() as directory:
            write_json(
                directory,
                "data/runs/candidate/result.json",
                {"elo_diff": 12.0, "games": 40, "candidate": {"sha256": "a" * 64}},
            )
            audit = audit_history(directory)
        self.assertEqual(audit["status"], "not_estimable")
        self.assertEqual(audit["summary"]["aggregate_outcome_files"], 1)
        self.assertEqual(audit["summary"]["candidate_identity_rows"], 1)
        self.assertEqual(audit["summary"]["numeric_feature_map_rows"], 0)

    def test_compatible_multi_group_rows_are_ready(self):
        with tempfile.TemporaryDirectory() as directory:
            for index in range(6):
                write_json(
                    directory,
                    f"results/candidate-{index}/final.json",
                    {
                        "candidate_id": f"c-{index}",
                        "group_id": f"g-{index % 2}",
                        "features": {"loss": float(index), "spread": 0.5},
                        "elo_diff": float(index * 10),
                        "games": 100,
                    },
                )
            audit = audit_history(directory)
        self.assertEqual(audit["status"], "ready")
        self.assertEqual(audit["summary"]["eligible_gate_observation_rows"], 6)
        self.assertEqual(audit["summary"]["largest_compatible_feature_schema_groups"], 2)

    def test_incompatible_feature_schemas_do_not_form_a_dataset(self):
        with tempfile.TemporaryDirectory() as directory:
            write_json(
                directory,
                "results/a/final.json",
                {
                    "candidate_id": "a",
                    "group_id": "g-a",
                    "features": {"loss": 1.0},
                    "elo_diff": 1.0,
                    "games": 20,
                },
            )
            write_json(
                directory,
                "results/b/final.json",
                {
                    "candidate_id": "b",
                    "group_id": "g-b",
                    "features": {"spread": 1.0},
                    "elo_diff": 2.0,
                    "games": 20,
                },
            )
            audit = audit_history(directory)
        self.assertEqual(audit["status"], "not_estimable")
        self.assertEqual(audit["summary"]["eligible_gate_observation_rows"], 2)
        self.assertEqual(audit["summary"]["largest_compatible_feature_schema_rows"], 1)

    def test_shards_and_tiny_aggregates_are_excluded(self):
        with tempfile.TemporaryDirectory() as directory:
            write_json(directory, "results/run/shard_0000.json", {"elo_diff": 10.0, "games": 100})
            write_json(directory, "results/run/combined.json", {"elo_diff": 10.0, "games": 2})
            audit = audit_history(directory)
        self.assertEqual(audit["summary"]["aggregate_outcome_files"], 0)


if __name__ == "__main__":
    unittest.main()
