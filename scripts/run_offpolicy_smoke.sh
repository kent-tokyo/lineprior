#!/usr/bin/env sh
# Runs the checked-in OPE fixture twice and verifies replayable JSON output.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
binary=${LINEPRIOR_BIN:-$root/target/debug/lineprior}

if [ -x "$binary" ]; then
  :
elif command -v "$binary" >/dev/null 2>&1; then
  binary=$(command -v "$binary")
else
  echo "lineprior executable not found or not executable: $binary" >&2
  exit 2
fi

directory=$(mktemp -d "${TMPDIR:-/tmp}/lineprior-offpolicy-XXXXXX")
trap 'rm -rf "$directory"' EXIT
first="$directory/report-1.json"
second="$directory/report-2.json"

run() {
  "$binary" offpolicy "$root/examples/offpolicy.jsonl" --out "$1" \
    --policy-name candidate --policy-version fixture-v1 --doubly-robust \
    --bootstrap-resamples 64 --bootstrap-seed 42
}

run "$first"
run "$second"
cmp -s "$first" "$second"

python3 -c 'import json, pathlib, sys; report=json.loads(pathlib.Path(sys.argv[1]).read_text()); assert report["ips"]["ips"] == 0.5; assert abs(report["doubly_robust"]["estimate"]-0.85) < 1e-12; assert report["bootstrap"]["seed"] == 42' "$first"
echo "off-policy smoke: ok (replayable IPS/DR/bootstrap)"
