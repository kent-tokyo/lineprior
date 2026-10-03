# Changelog

All notable workspace changes are recorded here. The five crates share one version. Publishing is a
separate release step; a version is not considered published until the tag, workflow, registry, and
GitHub Release have been checked.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Before `1.0`, a minor
release may include Rust source-breaking changes; such changes are called out explicitly.

## [Unreleased]

## [0.12.1] - 2026-10-03

### Added

- Added schema-v1 self-describing prior-book headers containing the producer version, complete
  `BuildConfig` JSON, and the legacy config fingerprint.
- Added `load_prior_book_with_metadata` and public metadata types. Headerless and fingerprint-only
  books remain readable.
- Added one version-contract check for workspace manifests, Cargo.lock, measurement Schemas,
  generated fixtures, and current-version documentation. Runtime tools now derive the version from
  the workspace manifest instead of repeating it.
- Added a formal web-target `lineprior-wasm` package contract. CI now packs the npm tarball,
  installs it into an empty consumer, and checks browser build/query, deterministic output, and the
  malformed-config error shape. Registry publication and Python bindings remain out of scope.
- Added a reproducible Sekirei opening-book case study. The book was used for all 20 eligible
  initial turns, while the 40-game on/off result was exactly 50% with a wide interval, so the
  integration stays optional and no downstream improvement is claimed.
- Added the first real held-out similarity decision artifact. The tested Sekirei feature adapter
  raised coverage but sharply worsened MRR and covered-query false recommendations, so exact match
  plus abstention remains the supported decision.
- Added a real-log IPS/DR readiness audit. Six Sekirei analysis logs contained 183 decisions but no
  decision-time propensities or declared rewards, so overlap, ESS, bootstrap, and causal effects are
  explicitly recorded as not estimable rather than fabricated.
- Added a GateModel real-history readiness audit. Twenty-three aggregate Sekirei result files had
  no shared pre-gate feature map or group IDs, so baseline/model/calibration/OOD/acquisition
  comparisons are recorded as not computable and the experimental scope remains frozen.

### Fixed

- Separated off-policy `policy_version` lineage from `lineprior_version`; the integrated runner no
  longer records the policy version as if it were the producing lineprior version.
- Kept immutable measurement artifacts pinned to their actual `0.12.0` producer while allowing
  stable-semver lineage in JSON Schema and explicit historical-version validation.
- Included the canonical MIT and Apache-2.0 license texts in the generated WASM npm tarball.

### Compatibility

- Identical book, config, and producer-version inputs remain byte-deterministic.
- Unsupported schema versions and malformed schema-v1 headers return typed errors.
- All five workspace crates and internal dependency requirements are version-locked at `0.12.1`.

## [0.12.0] - 2026-09-23

### Added

- Added `IncrementalPriorBuilder` for typed, bounded ingestion without a JSONL intermediate.
- Added context support and calibration diagnostics to `summary`, `eval`, and `tune`.
- Added deterministic similarity and paired IPS/DR measurement runners, lineage envelopes, JSON
  Schemas, and semantic validators. These establish artifact contracts, not real-data improvement.
- Added runtime inventory artifacts for the Python 3.12/3.13 × Node.js 22/24 CLI matrix and a
  separate WASM build artifact.
- Added the experimental `lineprior gate` CLI for verdict probabilities, acquisition output, and
  monotonic constraints.
- Added UI-automation fixtures plus Python and Node.js CLI round trips.
- Added license, audit, locked-build, rustdoc, candidate-contract, and measurement-schema gates.

### Fixed

- Updated the development-only Playwright dependency for its TLS verification advisory.
- Fixed the Python round-trip fixture path.
- Removed the generated-doc collision between the same-named library and CLI binary.

### Release

- Published all five workspace crates at `0.12.0` through the tagged workflow.

## [0.11.1] - 2026-09-02

- Added post-`0.11.0` WASM browser smoke and deterministic Trie/macro-action measurements.
- Corrected publishing documentation and CI. The three new crates completed their one-time
  token-based bootstrap; subsequent releases use OIDC.
- No scoring guarantee, causal capability, or counterfactual action generation was added.

## [0.11.0] - 2026-09-02

- Added opt-in Bayesian, UCB, and Softmax scoring while retaining weighted-sum as the default.
- Added LPB v1 persistence and `pack`/`unpack`.
- Added bounded macro-actions, weighted multi-source merge, terminal credit, and `PriorTrie`.
- Added `lineprior-adapters` for Sekirei, UI automation, LLM agents, and retrosynthesis.
- Added a reproducible veridict prior on/off protocol. No downstream result was claimed.
- Added wasm-pack and headless Chromium smoke; npm publication was not included.

## [0.10.0] - 2026-08-11

- Exposed the existing count/success/score weights through `build`, `eval`, and `tune`.
- Re-exported `GateStatus` and `PredictionStatus`.
- Added `cargo audit` to CI and fixed transitive `RUSTSEC-2026-0204` in the benchmark dependency
  tree.
- **Rust source compatibility:** `TuneParam` gained three variants. Exhaustive external matches
  require an update.

## [0.9.0] - 2026-07-26

- Added uncertainty-aware `GateModel` fitting, group-aware validation, OOD diagnostics, prediction
  support status, and gate-history provenance.
- Added typed validation for conflicting or incomplete gate uncertainty.
- **Rust source compatibility:** several public gate structs and `Error` gained fields or variants;
  exhaustive struct literals and matches may require an update. JSON input compatibility was
  preserved.

## [0.7.1] - 2026-07-20

- Fixed GateModel predictive variance to include intercept uncertainty and the correct degrees of
  freedom.
- Replaced hash-mod folds with deterministic balanced group folds.

## [0.7.0] - 2026-07-19

- Added the library-only `GateModel`: weighted ridge fitting, group-aware lambda selection,
  predictive intervals, and out-of-fold audit rows.

## [0.6.0] - 2026-07-12

- Added variable-order context with deterministic backoff.
- Added sequence-level path scoring with conservative minimum confidence.

## [0.5.1] - 2026-07-09

- Added crates.io and GitHub discoverability metadata only.

## [0.5.0] - 2026-07-09

- Added heuristic, Wilson-lower-bound, and hybrid confidence modes.
- Added calibration bins, threshold sweeps, time decay, source reliability, and `lineprior tune`.
- Added whole-config load/save for `build` and `eval`.
- Changing confidence mode can change `--min-confidence` filtering behavior.

## [0.4.0] - 2026-07-06

- Added deterministic flat candidate iteration and detailed build-filter statistics.
- Added config fingerprints and stale-cache detection.

## [0.3.0] - 2026-07-05

- Added `lineprior eval` with deterministic sequence-level train/test splitting and held-out ranking
  metrics.

## [0.2.0] - 2026-07-05

- Added streaming JSONL aggregation with memory proportional to unique state/action pairs.
- The streaming path returns an empty book for empty or fully filtered input so parse warnings are
  preserved; the eager API retains its original error behavior.

## [0.1.0] - 2026-07-05

- Initial domain-agnostic Rust library and CLI.
- Added `build`, `query`, `summary`, and `validate` with deterministic JSONL output, smoothing,
  confidence, entropy, thresholds, and strict/non-strict input handling.
