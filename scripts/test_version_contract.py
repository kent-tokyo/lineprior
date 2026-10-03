#!/usr/bin/env python3
"""Regression tests for the consolidated workspace version contract."""

import json
import pathlib
import tempfile
import unittest

from version_contract import (
    check_fixture_versions,
    check_schema_versions,
    workspace_version,
)


VERSION = workspace_version()
STALE_VERSION = "0.0.0"


class VersionContractTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.directory.name)
        (self.root / "docs" / "measurements").mkdir(parents=True)
        (self.root / "examples").mkdir()

    def tearDown(self):
        self.directory.cleanup()

    def write_schema(self, name, pattern=r"^[0-9]+\.[0-9]+\.[0-9]+$"):
        schema = {
            "$defs": {
                "measurement": {
                    "properties": {
                        "lineprior_version": {"type": "string", "pattern": pattern}
                    }
                }
            }
        }
        (self.root / "docs" / "measurements" / name).write_text(json.dumps(schema))

    def write_fixtures(self, version=VERSION):
        (self.root / "examples" / "veridict_prior_comparison.json").write_text(
            json.dumps({"lineprior_version": version})
        )
        header = json.dumps({"producer_version": version}) + "\n"
        for name in ("prior.jsonl", "shogi_prior.jsonl"):
            (self.root / "examples" / name).write_text(header)

    def test_schema_drift_is_rejected(self):
        self.write_schema("similarity-real-data-v1.schema.json")
        self.write_schema("offpolicy-integrated-arms-v1.schema.json", r"^0\.12\.0$")
        with self.assertRaisesRegex(ValueError, "offpolicy-integrated-arms-v1.schema.json"):
            check_schema_versions(self.root)

    def test_fixture_drift_is_rejected(self):
        self.write_fixtures(STALE_VERSION)
        with self.assertRaisesRegex(ValueError, "veridict fixture"):
            check_fixture_versions(VERSION, self.root)

    def test_matching_schemas_and_fixtures_pass(self):
        self.write_schema("similarity-real-data-v1.schema.json")
        self.write_schema("offpolicy-integrated-arms-v1.schema.json")
        self.write_fixtures()
        check_schema_versions(self.root)
        check_fixture_versions(VERSION, self.root)


if __name__ == "__main__":
    unittest.main()
