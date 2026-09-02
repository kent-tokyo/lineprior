#!/usr/bin/env sh
# Verifies the Rust-to-WASM compilation boundary without packaging or publishing artifacts.
set -eu

cargo build -p lineprior-wasm --target wasm32-unknown-unknown --locked
echo "WASM build smoke: ok (wasm32-unknown-unknown)"
