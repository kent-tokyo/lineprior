#!/usr/bin/env sh
# Runs the maintained cross-language CLI examples against one built binary.
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

LINEPRIOR_BIN="$binary" node "$root/examples/node/roundtrip.mjs"
LINEPRIOR_BIN="$binary" python3 "$root/examples/python/roundtrip.py"
echo "examples smoke: ok (Rust CLI + Node + Python)"
