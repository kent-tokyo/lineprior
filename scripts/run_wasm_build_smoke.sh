#!/usr/bin/env sh
# Verifies the Rust-to-WASM compilation boundary without packaging or publishing artifacts.
set -eu

report=""
if [ "$#" -eq 2 ] && [ "$1" = "--out" ]; then
  report=$2
elif [ "$#" -ne 0 ]; then
  echo "usage: $0 [--out wasm-runtime-report.json]" >&2
  exit 3
fi

cargo build -p lineprior-wasm --target wasm32-unknown-unknown --locked
if [ -n "$report" ]; then
  python3 -c 'import json, pathlib, subprocess, sys; out=pathlib.Path(sys.argv[1]); report={"protocol":"wasm-build-smoke-v1","project_version":"0.11.1","git_commit":subprocess.check_output(["git","rev-parse","HEAD"], text=True).strip(),"target":"wasm32-unknown-unknown","toolchain":subprocess.check_output(["rustc","--version"], text=True).strip(),"checks":["cargo-build-locked"]}; out.write_text(json.dumps(report, indent=2, sort_keys=True)+"\n")' "$report"
  python3 scripts/validate_wasm_runtime.py "$report"
  directory=$(mktemp -d "${TMPDIR:-/tmp}/lineprior-wasm-contract-XXXXXX")
  trap 'rm -rf "$directory"' EXIT
  bad_target="$directory/bad-target.json"
  python3 -c 'import json, pathlib, sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); r["target"] = "wasm32-wasi"; pathlib.Path(sys.argv[2]).write_text(json.dumps(r))' "$report" "$bad_target"
  if python3 scripts/validate_wasm_runtime.py "$bad_target" >/dev/null 2>&1; then exit 1; fi
fi
echo "WASM build smoke: ok (wasm32-unknown-unknown)"
