#!/usr/bin/env python3
"""Check the checked-in JSON Schema envelopes for measurement artifacts."""
import json
import pathlib


ROOT = pathlib.Path(__file__).resolve().parent.parent
EXPECTED = {
    "similarity-real-data-v1.schema.json": {
        "protocol": "similarity-real-data-v1",
        "root_required": {"protocol", "num_queries", "measurement", "arms"},
        "measurement_required": {"dataset_id", "split", "feature_version", "lineprior_version", "input_sha256", "prior_config_fingerprint"},
        "defs": {"digest", "unit", "arm", "measurement"},
    },
    "offpolicy-integrated-arms-v1.schema.json": {
        "protocol": "offpolicy-integrated-arms-v1",
        "root_required": {"protocol", "measurement", "arms", "paired"},
        "measurement_required": {"dataset_id", "split", "lineprior_version", "input_sha256"},
        "defs": {"digest", "measurement", "estimate", "arm", "paired"},
    },
}


def main():
    for filename, expected in EXPECTED.items():
        schema = json.loads((ROOT / "docs" / "measurements" / filename).read_text())
        if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
            raise ValueError(f"{filename}: unexpected JSON Schema dialect")
        if not isinstance(schema.get("$id"), str) or not schema["$id"].startswith("https://github.com/kent-tokyo/lineprior/"):
            raise ValueError(f"{filename}: missing canonical repository $id")
        if schema.get("type") != "object" or schema.get("properties", {}).get("protocol", {}).get("const") != expected["protocol"]:
            raise ValueError(f"{filename}: root protocol contract is incomplete")
        if set(schema.get("required", [])) != expected["root_required"]:
            raise ValueError(f"{filename}: root required fields changed unexpectedly")
        definitions = schema.get("$defs", {})
        if set(definitions) != expected["defs"]:
            raise ValueError(f"{filename}: definitions changed unexpectedly")
        measurement = definitions.get("measurement", {})
        if set(measurement.get("required", [])) != expected["measurement_required"]:
            raise ValueError(f"{filename}: measurement required fields changed unexpectedly")
        if measurement.get("properties", {}).get("lineprior_version", {}).get("const") != "0.11.1":
            raise ValueError(f"{filename}: version is not fixed at 0.11.1")
        digest = definitions.get("digest", {})
        if digest.get("type") != "string" or digest.get("pattern") != "^[0-9a-f]{64}$":
            raise ValueError(f"{filename}: digest constraint is incomplete")
        unit = definitions.get("unit")
        if filename.startswith("similarity") and (unit.get("type") != ["number", "null"] or unit.get("minimum") != 0 or unit.get("maximum") != 1):
            raise ValueError(f"{filename}: unit interval constraint is incomplete")
        arm_required = definitions.get("arm", {}).get("required", [])
        expected_arm = {"coverage", "abstention_rate", "top1_hit_rate", "mrr", "calibration_brier"} if filename.startswith("similarity") else {"ips", "doubly_robust", "bootstrap"}
        if set(arm_required) != expected_arm:
            raise ValueError(f"{filename}: arm required fields changed unexpectedly")
    print("measurement JSON Schema contract: ok")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"measurement schema error: {error}")
