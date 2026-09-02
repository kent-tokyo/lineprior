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
    "lineprior": "0.11.1",
    "lineprior-adapters": "0.11.1",
    "lineprior-cli": "0.11.1",
    "lineprior-similarity": "0.11.1",
    "lineprior-wasm": "0.11.1",
}
if packages != expected:
    raise SystemExit(f"unexpected lineprior package versions: {packages!r}")
print("workspace contract: ok")
'

python3 -c 'import json, pathlib, sys; rows=[json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]; assert all(isinstance(row, dict) for row in rows)' examples/offpolicy.jsonl
python3 -c 'import json, pathlib, sys; rows=[json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]; assert all(isinstance(row, dict) for row in rows)' examples/offpolicy_off.jsonl
python3 -c 'import json, pathlib, sys; rows=[json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]; assert all(isinstance(row, dict) for row in rows)' examples/offpolicy_on.jsonl
python3 -c 'import json, pathlib, sys; rows=[json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]; assert all(isinstance(row, dict) for row in rows)' examples/similarity_queries.jsonl
python3 -c 'import json, pathlib, sys; rows=[json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]; assert all(isinstance(row, dict) for row in rows)' examples/ui_automation.jsonl
python3 -c 'import json, pathlib, sys; rows=[json.loads(line) for line in pathlib.Path(sys.argv[1]).read_text().splitlines() if line.strip()]; assert all(isinstance(row, dict) for row in rows)' crates/lineprior-similarity/tests/fixtures/unseen_states.jsonl
python3 -c 'import json, pathlib; json.loads(pathlib.Path("examples/veridict_prior_comparison.json").read_text())'
test -s examples/adapters.md

node --check examples/node/roundtrip.mjs
node --check examples/wasm/browser-smoke.mjs
node -e 'const p=require("./examples/wasm/package.json"); if (p.devDependencies.playwright !== "1.55.0") process.exit(1)'
python3 -c 'import ast; ast.parse(open("examples/python/roundtrip.py").read())'
python3 -c 'import ast; [ast.parse(open(path).read()) for path in ("scripts/measure_similarity.py", "scripts/compare_offpolicy_arms.py", "scripts/measure_offpolicy_arms.py", "scripts/validate_measurement_artifact.py")]'
test -x scripts/run_ecosystem_matrix_smoke.sh
sh -n scripts/run_ecosystem_matrix_smoke.sh
git diff --check
echo "candidate contract: ok (version fixed at 0.11.1)"
