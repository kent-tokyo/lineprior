# Runtime and WASM compatibility evidence

This document records the compatibility checks maintained for `lineprior` `0.12.1`. It defines
the current evidence boundary; it is not a promise that every runtime version is supported.

## Maintained CI matrix

The `examples-smoke` job runs the CLI integration examples in four cells:

| Python | Node.js | Evidence |
| --- | --- | --- |
| 3.12 | 22 | CLI round-trip, examples, OPE, measurement smoke |
| 3.12 | 24 | CLI round-trip, examples, OPE, measurement smoke |
| 3.13 | 22 | CLI round-trip, examples, OPE, measurement smoke |
| 3.13 | 24 | CLI round-trip, examples, OPE, measurement smoke |

Each cell emits a validated `ecosystem-matrix-smoke-v1` artifact containing the active toolchain,
requested matrix labels, commit, project version, and executed checks. Validation rejects a matrix
label that does not match the recorded runtime. This catches CLI/example integration drift; it does
not create formal Python or npm bindings.

## Rust and WASM boundary

The normal CI workflow runs formatting, clippy, tests, rustdoc, license checks, dependency audit,
candidate-contract checks, and a locked `wasm32-unknown-unknown` build. The WASM build job emits a
validated `wasm-build-smoke-v1` artifact with its target, toolchain, commit, and project version.

The separate `WASM browser smoke` workflow builds `lineprior-wasm` with `wasm-pack --target web`,
creates the `lineprior-wasm` npm tarball, installs that tarball into an empty consumer directory,
and runs build/query in headless Chromium. It also checks byte-identical repeated output and the
malformed-config error prefix. Its validated artifact records the source commit, tool versions,
package hash, and whether the source tree was dirty.

The maintained package boundary is:

- package name: `lineprior-wasm`;
- owner: the repository maintainers;
- build target: `web` only;
- tooling: Node.js 22 and 24 are maintained for package construction; runtime execution is covered
  only by the workflow's Playwright Chromium;
- API: initialized `build_json` and `query_json` exports, with Rust as the scoring source of truth;
- license: the tarball includes the workspace's canonical MIT and Apache-2.0 texts;
- release form: a clean-installable npm tarball generated from a tagged source tree.

The source-package smoke is separate from registry publication. There is no maintained Python
binding: Python support means invoking the CLI, as exercised by the runtime matrix.

## What remains open

The following are deliberately not claimed by this matrix:

- every supported Rust, Python, Node.js, or WASM runtime version;
- a maintained Python binding or a registry-published npm package;
- browser execution beyond the maintained headless Chromium smoke;
- downstream quality, similarity, IPS/DR, GateModel, or memory performance;
- real-data compatibility or improvement.

The source-package gate is closed by the clean-install tarball workflow above. Registry publication
remains a separate release decision and must not be inferred from this smoke test.
