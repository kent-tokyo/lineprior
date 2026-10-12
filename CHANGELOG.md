# Changelog

All notable workspace changes are recorded here. The five crates share one version. Publishing is a
separate release step; a version is not considered published until the tag, workflow, registry, and
GitHub Release have been checked.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Before `1.0`, a minor
release may include Rust source-breaking changes; such changes are called out explicitly.

## [Unreleased]

### Changed

- Clarified the public product boundary: `lineprior` is an independent, domain-agnostic
  candidate-ordering component. It ranks observed candidates or abstains; downstream applications
  own parsing, execution, verification, search, and final decisions.
- Replaced downstream-specific public examples and case-study wording with a generic paired
  candidate-ordering integration guide. Historical machine-readable evidence remains a record of
  its original producer and is not a general performance claim.

## [0.12.5] - 2026-10-13

### Added

- Added `PriorBook::query_borrowed` with reusable `QueryScratch` index storage for ranked
  read-only lookup without cloning action strings or metadata. The existing owned `query` API and
  ranking semantics remain unchanged.

### Fixed

- Keep IPS, self-normalized IPS, doubly robust means, and Kish effective sample size finite for
  representable extreme inputs, and reject derived importance weights or corrections that exceed
  the numeric representation instead of emitting `NaN` or infinity.

### Compatibility

- Prior-book schema v1, compact LPB v1, and optional artifact manifests remain compatible.
- `OffPolicyError` gains `NonFiniteDerivedValue`; exhaustive Rust matches must handle the new
  variant.
- All five workspace crates and internal dependency requirements are version-locked at `0.12.5`.

## [0.12.4] - 2026-10-11

### Added

- Added a public prior-artifact verifier and `verify-artifact` CLI command for checking manifest
  semantics, exact input/book SHA-256 values, and schema-v1 book metadata.

### Fixed

- Hash and parse a verified prior book through the same file stream so a path replacement cannot
  make `verify-artifact` compare one set of bytes while validating another.
- Avoid NaN similarity candidates when distance weighting underflows to zero, and compute
  Euclidean distances without avoidable intermediate overflow.

### Compatibility

- Prior-book schema v1, compact LPB v1, and optional artifact manifests remain compatible.
- All five workspace crates and internal dependency requirements are version-locked at `0.12.4`.

## [0.12.3] - 2026-10-10

### Added

- Added a pinned downstream `GateObservation` fixture regression that checks both JSON Schema and
  collection-time semantics without treating the fixture as real gate-history evidence.

### Changed

- Condensed `AGENTS.md` to repository-changing rules and moved usage, measurement, and release
  details back to their canonical documents.
- Clarified that the Rust `lineprior-wasm` crate is published on crates.io while the generated web
  package is not published to the npm registry.
- Updated downstream integration notes with an archived paired-prior diagnostic and zero-row
  gate-readiness result, without rewriting older evidence.

### Fixed

- Prevented time-decay arithmetic from overflowing when valid input timestamps span the full
  `i64` range.
- Reject LPB v1 inputs with unknown header flags or duplicate state/context entries instead of
  silently interpreting or overwriting them.
- Avoid large up-front allocations when a truncated LPB input declares an extreme context or
  action count.
- Bound bootstrap resampling before allocation and reuse validated observation references instead
  of cloning complete rows for every resample.
- Make seeded bootstrap index generation consistent between wasm32 and 64-bit targets by reducing
  the full `u64` generator state before conversion to an index.
- Surface final buffered-write failures from `offpolicy` and `gate` report files instead of
  silently losing them during writer destruction.
- Reject prior-book and artifact-manifest path aliases, including hard links and aliases whose
  output files do not exist yet, and surface final buffered-write failures from the prior-book
  output.
- Apply the same input/output collision protection to `pack` and `unpack`, and classify truncated
  LPB data as invalid input instead of an internal I/O failure.
- Reject duplicate state/context entries in JSONL prior books instead of silently making the last
  duplicate replace earlier evidence.
- Avoid integer overflow when a macro-action window limit is larger than the remaining sequence.
- Return an error when multi-source action counts exceed `u64`, and saturate diagnostic context
  totals instead of panicking on extreme caller-constructed books.
- Validate state, context, action uniqueness, finite evidence, priors, and confidence at both JSONL
  and LPB read/write boundaries so malformed books cannot propagate invalid rankings.
- Apply prior-entry validation to caller-constructed merge sources and reject weighted merge
  arithmetic that would produce non-finite evidence.
- Reject empty typed observation identifiers, non-finite observation values, invalid numeric build
  controls, and effective-weight products that cannot be represented safely.
- Keep normalized priors finite and within `[0, 1]` for negative or extreme utilities, and avoid a
  zero-strength Bayesian `0 / 0` when no outcome evidence exists.
- Include terminal-outcome credit in Kish squared-weight totals and avoid intermediate overflow in
  effective-sample-size calculations.

### Compatibility

- Prior-book schema v1, compact LPB v1, and existing readers remain compatible.
- All five workspace crates and internal dependency requirements are version-locked at `0.12.3`.

## [0.12.2] - 2026-10-10

### Added

- Added optional SHA-256 prior-artifact sidecars that link exact input and book bytes while leaving
  the schema-v1 book format unchanged for existing consumers.
- Added JSON Schemas and `validate --kind offpolicy|gate` collection-time checks for raw
  `OffPolicyObservation` and `GateObservation` logs.

### Changed

- Shortened the English and Japanese READMEs around the supported workflow and evidence boundary;
  reorganized the documentation index and adapter summary.

### Fixed

- Rejected logging propensities above one and empty off-policy or gate identifiers before
  estimation or model fitting.

### Compatibility

- Prior-book schema v1 and existing readers remain compatible; artifact manifests are optional
  sidecars.
- All five workspace crates and internal dependency requirements are version-locked at `0.12.2`.

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
  malformed-config error shape. npm registry publication and Python bindings remain out of scope.
- Added a reproducible downstream candidate-ordering case study. The prior was used for all 20 eligible
  initial turns, while the 40-game on/off result was exactly 50% with a wide interval, so the
  integration stays optional and no downstream improvement is claimed.
- Added the first real held-out similarity decision artifact. The tested downstream feature adapter
  raised coverage but sharply worsened MRR and covered-query false recommendations, so exact match
  plus abstention remains the supported decision.
- Added a real-log IPS/DR readiness audit. Six downstream analysis logs contained 183 decisions but no
  decision-time propensities or declared rewards, so overlap, ESS, bootstrap, and causal effects are
  explicitly recorded as not estimable rather than fabricated.
- Added a GateModel real-history readiness audit. Twenty-three aggregate downstream result files had
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
- Added `lineprior-adapters` for structured records, UI automation, tool-using agents, and
  retrosynthesis.
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
