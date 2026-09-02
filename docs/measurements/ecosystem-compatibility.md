# Ecosystem compatibility evidence

This document defines the compatibility evidence currently maintained for
`lineprior` `0.11.1`. It is an evidence boundary, not a promise that every
runtime version is supported.

## Maintained CI matrix

The `examples-smoke` job runs the CLI integration examples in four cells:

| Python | Node.js | Evidence |
| --- | --- | --- |
| 3.12 | 22 | CLI round-trip, examples, OPE, measurement smoke |
| 3.12 | 24 | CLI round-trip, examples, OPE, measurement smoke |
| 3.13 | 22 | CLI round-trip, examples, OPE, measurement smoke |
| 3.13 | 24 | CLI round-trip, examples, OPE, measurement smoke |

Each cell records the active Rust, Cargo, Python, and Node.js versions in a
validated `ecosystem-matrix-smoke-v1` JSON artifact, including the requested
`matrix.python` and `matrix.node` cell labels. The artifact also records
the commit, fixed project version, and checks that ran. The matrix catches
CLI/example integration drift; it is not evidence of formal Python or npm
bindings.
Validation also compares the requested Python major/minor and Node.js major
labels with the recorded runtime strings, so a mislabeled or misresolved CI
cell is rejected before upload.

## Rust and WASM boundary

The normal CI check uses the stable Rust toolchain for formatting, clippy,
tests, and documentation. A separate `wasm-build-smoke` job compiles
`lineprior-wasm` for `wasm32-unknown-unknown` with the locked dependency set.
It stores and validates a `wasm-build-smoke-v1` artifact containing the target,
Rust toolchain, commit, and fixed project version. Target-drift rejection is
also exercised before upload.

This proves a reproducible Rust-to-WASM compilation boundary only. It does not
prove browser behavior, npm publication, package installation, or performance.

## What remains open

The following are deliberately not claimed by this matrix:

- every supported Rust, Python, Node.js, or WASM runtime version;
- maintained Python or npm packages;
- browser execution across browser versions;
- downstream quality, similarity, IPS/DR, GateModel, or memory performance;
- real-data compatibility or improvement.

The broader runtime install/build/round-trip/error-shape/deterministic-output
measurement gate remains open until a declared support matrix and its measured
artifacts are available.
