#!/usr/bin/env python3
"""Check the checked-in JSON Schema envelopes for measurement artifacts."""
import json
import pathlib


ROOT = pathlib.Path(__file__).resolve().parent.parent
EXPECTED = {
    "similarity-real-data-v1.schema.json": "similarity-real-data-v1",
    "offpolicy-integrated-arms-v1.schema.json": "offpolicy-integrated-arms-v1",
}


def main():
    for filename, protocol in EXPECTED.items():
        schema = json.loads((ROOT / "docs" / "measurements" / filename).read_text())
        if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
            raise ValueError(f"{filename}: unexpected JSON Schema dialect")
        if schema.get("type") != "object" or schema.get("properties", {}).get("protocol", {}).get("const") != protocol:
            raise ValueError(f"{filename}: root protocol contract is incomplete")
        if "measurement" not in schema.get("required", []):
            raise ValueError(f"{filename}: measurement must be required")
        measurement = schema.get("$defs", {}).get("measurement", {})
        if measurement.get("properties", {}).get("lineprior_version", {}).get("const") != "0.11.1":
            raise ValueError(f"{filename}: version is not fixed at 0.11.1")
    print("measurement JSON Schema contract: ok")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, json.JSONDecodeError) as error:
        raise SystemExit(f"measurement schema error: {error}")
