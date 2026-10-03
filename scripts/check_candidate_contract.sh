#!/usr/bin/env sh
# Static candidate checks intentionally avoid publishing, tagging, or changing the version.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

cargo fmt --all -- --check
project_version=$(python3 scripts/version_contract.py --print)
python3 scripts/version_contract.py --check
python3 scripts/test_version_contract.py
python3 -c 'import pathlib, tomllib; manifest=tomllib.loads(pathlib.Path("crates/lineprior-cli/Cargo.toml").read_text()); assert manifest["bin"] == [{"name": "lineprior", "path": "src/main.rs", "doc": False}]'

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
node -e 'const p=require("./examples/wasm/package.json"); if (p.devDependencies.playwright !== "1.55.1") process.exit(1)'
python3 -c 'import ast; ast.parse(open("examples/python/roundtrip.py").read())'
python3 scripts/check_measurement_schemas.py
python3 scripts/test_measurement_validator_messages.py
python3 scripts/test_measure_similarity.py
python3 scripts/test_prepare_sekirei_similarity.py
python3 scripts/test_audit_offpolicy_readiness.py
python3 scripts/test_audit_gate_history_readiness.py
python3 scripts/validate_measurement_artifact.py similarity docs/measurements/similarity-sekirei-2026-10-03.json --require-explicit-lineage --expected-lineprior-version 0.12.0
python3 scripts/validate_offpolicy_readiness.py docs/measurements/offpolicy-sekirei-readiness-2026-10-03.json --expected-lineprior-version 0.12.0
python3 scripts/validate_gate_history_readiness.py docs/measurements/gate-history-sekirei-readiness-2026-10-03.json --expected-lineprior-version 0.12.0
python3 scripts/validate_sekirei_case_study.py docs/case-studies/sekirei-opening-book-2026-10-03.json --expected-lineprior-version 0.12.0
test -s crates/lineprior-cli/tests/measurement_schema.rs
python3 -c 'import ast; [ast.parse(open(path).read()) for path in ("scripts/version_contract.py", "scripts/test_version_contract.py", "scripts/measure_similarity.py", "scripts/test_measure_similarity.py", "scripts/prepare_sekirei_similarity.py", "scripts/test_prepare_sekirei_similarity.py", "scripts/audit_offpolicy_readiness.py", "scripts/test_audit_offpolicy_readiness.py", "scripts/validate_offpolicy_readiness.py", "scripts/audit_gate_history_readiness.py", "scripts/test_audit_gate_history_readiness.py", "scripts/validate_gate_history_readiness.py", "scripts/compare_offpolicy_arms.py", "scripts/measure_offpolicy_arms.py", "scripts/validate_measurement_artifact.py", "scripts/check_measurement_schemas.py", "scripts/test_measurement_validator_messages.py", "scripts/validate_ecosystem_runtime.py", "scripts/validate_wasm_runtime.py", "scripts/validate_npm_package_runtime.py", "scripts/validate_sekirei_case_study.py")]'
test -x scripts/run_ecosystem_matrix_smoke.sh
sh -n scripts/run_ecosystem_matrix_smoke.sh
test -x scripts/run_npm_package_smoke.sh
sh -n scripts/run_npm_package_smoke.sh
git diff --check
echo "candidate contract: ok (version fixed at $project_version)"
