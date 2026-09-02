#!/usr/bin/env sh
# Replays the maintained ecosystem smoke suite with an explicit runtime inventory.
# This is a reproducibility handoff, not evidence that every supported version works.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

command -v cargo >/dev/null 2>&1
command -v rustc >/dev/null 2>&1
command -v python3 >/dev/null 2>&1
command -v node >/dev/null 2>&1

echo "runtime: $(rustc --version)"
echo "runtime: $(cargo --version)"
echo "runtime: $(python3 --version 2>&1)"
echo "runtime: $(node --version)"

cargo build -p lineprior-cli --locked
LINEPRIOR_BIN="$root/target/debug/lineprior" sh "$root/scripts/run_examples_smoke.sh"
LINEPRIOR_BIN="$root/target/debug/lineprior" sh "$root/scripts/run_offpolicy_smoke.sh"
LINEPRIOR_BIN="$root/target/debug/lineprior" sh "$root/scripts/run_measurement_smoke.sh"
echo "ecosystem matrix smoke: ok (runtime inventory + Rust/Node/Python/OPE/measurement)"
