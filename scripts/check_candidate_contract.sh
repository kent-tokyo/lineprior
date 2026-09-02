#!/usr/bin/env sh
# Static candidate checks intentionally avoid publishing, tagging, or changing the version.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

cargo fmt --all -- --check
metadata=$(cargo metadata --offline --no-deps --format-version 1)
printf '%s' "$metadata" | python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
packages = {
    package["name"]: package["version"]
    for package in metadata["packages"]
    if package["name"].startswith("lineprior")
}
expected = {
    "lineprior": "0.11.0",
    "lineprior-adapters": "0.11.0",
    "lineprior-cli": "0.11.0",
    "lineprior-similarity": "0.11.0",
    "lineprior-wasm": "0.11.0",
}
if packages != expected:
    raise SystemExit(f"unexpected lineprior package versions: {packages!r}")
print("workspace contract: ok")
'

python3 -c 'import json, pathlib, sys; rows=[json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]; assert all(isinstance(row, dict) for row in rows)' examples/offpolicy.jsonl
python3 -c 'import json, pathlib, sys; rows=[json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]; assert all(isinstance(row, dict) for row in rows)' examples/ui_automation.jsonl
python3 -c 'import json, pathlib, sys; rows=[json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]; assert all(isinstance(row, dict) for row in rows)' crates/lineprior-similarity/tests/fixtures/unseen_states.jsonl
python3 -c 'import json, pathlib; json.loads(pathlib.Path("examples/veridict_prior_comparison.json").read_text())'
test -s examples/adapters.md

node --check examples/node/roundtrip.mjs
node --check examples/wasm/browser-smoke.mjs
python3 -c 'import ast; ast.parse(open("examples/python/roundtrip.py").read())'
git diff --check
echo "candidate contract: ok (version fixed at 0.11.0)"
