#!/usr/bin/env sh
# Replays the maintained ecosystem smoke suite with an explicit runtime inventory.
# This is a reproducibility handoff, not evidence that every supported version works.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
report=""
if [ "$#" -eq 2 ] && [ "$1" = "--out" ]; then
  report=$2
elif [ "$#" -ne 0 ]; then
  echo "usage: $0 [--out runtime-report.json]" >&2
  exit 3
fi

command -v cargo >/dev/null 2>&1
command -v rustc >/dev/null 2>&1
command -v python3 >/dev/null 2>&1
command -v node >/dev/null 2>&1

rust_version=$(rustc --version)
cargo_version=$(cargo --version)
python_version=$(python3 --version 2>&1)
node_version=$(node --version)
echo "runtime: $rust_version"
echo "runtime: $cargo_version"
echo "runtime: $python_version"
echo "runtime: $node_version"

cargo build -p lineprior-cli --locked
LINEPRIOR_BIN="$root/target/debug/lineprior" sh "$root/scripts/run_examples_smoke.sh"
LINEPRIOR_BIN="$root/target/debug/lineprior" sh "$root/scripts/run_offpolicy_smoke.sh"
LINEPRIOR_BIN="$root/target/debug/lineprior" sh "$root/scripts/run_measurement_smoke.sh"
if [ -n "$report" ]; then
  python3 -c 'import json, os, pathlib, subprocess, sys; out=pathlib.Path(sys.argv[1]); report={"protocol":"ecosystem-matrix-smoke-v1","project_version":"0.11.1","git_commit":subprocess.check_output(["git","rev-parse","HEAD"], text=True).strip(),"runtimes":{"rustc":sys.argv[2],"cargo":sys.argv[3],"python":sys.argv[4],"node":sys.argv[5]},"checks":["cli-roundtrip","node-example","python-example","offpolicy","measurement"]}; py=os.environ.get("LINEPRIOR_MATRIX_PYTHON"); node=os.environ.get("LINEPRIOR_MATRIX_NODE"); report.update({"matrix":{"python":py,"node":node}} if py and node else {}); out.write_text(json.dumps(report, indent=2, sort_keys=True)+"\n")' "$report" "$rust_version" "$cargo_version" "$python_version" "$node_version"
  python3 "$root/scripts/validate_ecosystem_runtime.py" "$report"
fi
echo "ecosystem matrix smoke: ok (runtime inventory + Rust/Node/Python/OPE/measurement)"
