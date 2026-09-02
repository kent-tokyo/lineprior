#!/usr/bin/env python3
"""Small CLI integration example for a UI-automation action prior.

The domain adapter owns the screen/action vocabulary; lineprior only sees
JSONL.  Set LINEPRIOR_BIN when the executable is not on PATH.
"""

import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile


ROOT = pathlib.Path(__file__).resolve().parents[2]
INPUT = ROOT / "ui_automation.jsonl"
STATE = "cart-empty"


def run(*args: str) -> subprocess.CompletedProcess[str]:
    binary = os.environ.get("LINEPRIOR_BIN", "lineprior")
    return subprocess.run([binary, *args], check=True, text=True, capture_output=True)


def main() -> int:
    binary = os.environ.get("LINEPRIOR_BIN", "lineprior")
    if shutil.which(binary) is None and not pathlib.Path(binary).exists():
        print(f"lineprior executable not found: {binary}", file=sys.stderr)
        return 2

    with tempfile.TemporaryDirectory(prefix="lineprior-python-") as directory:
        first = pathlib.Path(directory) / "prior-1.jsonl"
        second = pathlib.Path(directory) / "prior-2.jsonl"
        run("build", str(INPUT), "--out", str(first))
        run("build", str(INPUT), "--out", str(second))
        if first.read_bytes() != second.read_bytes():
            raise RuntimeError("repeated builds were not deterministic")

        result = run("query", str(first), "--state", STATE, "--top-k", "1")
        rows = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
        if not rows or rows[0]["action"] != "click:add-to-cart":
            raise RuntimeError(f"unexpected query result: {rows!r}")
        print(json.dumps(rows[0], sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
