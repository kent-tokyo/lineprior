# Changelog

## [0.11.1] - 2026-09-02

- Patch release for the post-v0.11.0 WASM browser smoke, deterministic Trie/macro-action
  measurements, and publishing documentation/CI corrections.
- No new scoring guarantee, causal capability, or counterfactual action generation is introduced.
- The workspace remains version-locked at `0.11.1` for this release.
- All five workspace crates were published through GitHub Actions: `lineprior` and `lineprior-cli`
  via OIDC, and the three new crates via the configured `CARGO_REGISTRY_TOKEN` bootstrap fallback
  (runs `33625228643`, `33625228724`, and `33625228726`).

## [0.11.0] - 2026-09-02

- Added pluggable Bayesian, UCB, and Softmax scoring strategies with the legacy weighted-sum
  behavior retained as the default.
- Added deterministic LPB v1 compact binary persistence and CLI `pack`/`unpack` commands.
- Added a veridict prior on/off comparison recipe and manifest; no downstream result is claimed.
- Added bounded macro-action extraction, deterministic weighted multi-source book merge, and the
  official `lineprior-adapters` crate for four domain-neutral integration boundaries.
- Added opt-in terminal-outcome credit propagation and deterministic `PriorTrie` materialization;
  both remain conservative opt-ins with real-data/performance measurement gates open.
- Added and passed a wasm-pack plus headless Chromium browser smoke workflow (`33624039263`);
  this validates package/runtime wiring, not npm publication or downstream quality.

All notable changes to the workspace crates are documented here. They share one workspace version
(`version.workspace = true`), so this file covers the library, CLI, adapters, similarity, and WASM
crates.

`lineprior` and `lineprior-cli` v0.11.0 were published to crates.io through the tagged GitHub
Actions workflow. The newly introduced `lineprior-adapters`, `lineprior-similarity`, and
`lineprior-wasm` packages passed package/dry-run/authentication, but crates.io rejected their first
publication because Trusted Publishing cannot create a new crate; each requires one manual first
publication before the workflow can publish subsequent versions.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versioning follows
[Semantic Versioning](https://semver.org/), with the pre-1.0 caveat SemVer itself states: while the
crate is `0.x`, a minor version bump (`0.x.0`) may include breaking changes to the public Rust API,
not just additions — each entry below says explicitly when that's the case. JSON/JSONL input and
`serde` (de)serialization compatibility is tracked separately from Rust source compatibility, since
the two can diverge (a new `Option<T>` struct field is serde-compatible but source-breaking for a
caller using an exhaustive struct literal).

Not every version below has been published to crates.io -- publishing is a separate explicit step
in this project, not automatic on every version bump. See each entry for its publish status.

## [Unreleased] - next version candidate (not yet versioned or published)

### Added

- Added context prefix-support diagnostics to `summary`/`SummaryReport`, exposing distinct
  prefixes, `(prefix, state)` entries, action entries, and raw count by context order.
- Added context coverage, fallback rate, and mean matched order to `EvalReport` so context support
  can be separated from context ranking quality.
- Exposed the same context diagnostics in each `lineprior tune` candidate result for direct
  per-configuration comparison.
- Added real-data measurement handoff documents for similarity arms and IPS/DR paired on/off
  evaluation; no result is claimed without supplied real logs.
- Extended the fixed-version candidate contract to validate measurement fixtures and runner syntax.
- Added a CI measurement smoke for deterministic similarity-arm and paired on/off runner replay.
- Measurement reports now carry explicit dataset/split and version metadata for artifact lineage.
- Added an integrated paired OPE runner that combines per-arm Rust IPS/DR reports with the
  observed-reward pairing audit.
- Added a measurement-artifact contract checker for protocol, lineage, required metrics, and the
  fixed `0.11.1` version envelope; it validates shape only and does not pass a quality gate.
- Added context-path Brier calibration diagnostics to `eval` and `tune`, separate from order-0
  calibration bins and unchanged when context is disabled.
- Added matched-context-order Brier diagnostics so calibration can be audited separately for each
  backoff depth.
- Added an ecosystem matrix smoke harness that records Rust, Python, and Node runtime versions
  before replaying the maintained CLI, OPE, and measurement checks.
- Similarity measurement artifacts now retain the prior's BuildConfig fingerprint for comparison
  lineage and reject conflicting fingerprint headers.
- Similarity and paired OPE artifacts now include SHA-256 hashes of their input JSONL files for
  immutable dataset lineage.
- Artifact validation now checks SHA-256 syntax and rejects integrated OPE reports whose paired
  audit hashes differ from the top-level inputs.

### Fixed

- Fixed the Python CLI round-trip example to resolve the checked-in UI-automation fixture from
  `examples/ui_automation.jsonl` when executed from the repository root.

- Opt-in `PriorBook::query_with_similarity` for caller-supplied similar states with deterministic
  distance weighting, provenance-preserving evidence, and no invented actions.
- `lineprior-similarity`, a dependency-free Euclidean feature-vector nearest-neighbor adapter with
  finite-value/dimension validation and deterministic tie-breaking.
- A checked-in unseen-state similarity measurement fixture that keeps exact-match/no-prior
  abstention separate from opt-in neighbor recovery; it is not real-data quality evidence.
- Separate self-normalized IPS evaluation with explicit propensity inputs, overlap diagnostics,
  importance-weight caps, and effective sample size reporting.
- Doubly robust evaluation using caller-supplied reward-model predictions; no reward model is
  trained or inferred by the library.
- Deterministic percentile-bootstrap intervals for IPS and self-normalized IPS.
- `lineprior offpolicy` CLI command for JSONL logs, with optional DR and bootstrap output.
- A small valid OPE JSONL boundary fixture, public IPS/DR replay test, and documented deterministic
  CLI invocation.
- A repeatable candidate-contract script and CI job for version, fixture, syntax, formatting, and
  diff checks; it does not replace runtime or real-data release gates.
- A maintained CLI example smoke workflow that runs the Node.js and Python round trips against one
  built Rust binary and checks deterministic output plus the expected query result.
- The CLI smoke helper now accepts either an executable path or a PATH-resolved `LINEPRIOR_BIN`.
- A locked `wasm32-unknown-unknown` compilation smoke workflow for `lineprior-wasm`; packaging and
  browser execution remain separate, intentionally open gates.
- A replayable OPE CLI smoke workflow that compares two complete IPS/DR/bootstrap reports from the
  checked-in fixture; it is not causal-improvement evidence.
- CI candidate checks now use Cargo's `--locked` mode for clippy, tests, and the CLI smoke build.
- Candidate contract validation now covers all checked-in OPE, UI-automation, and similarity JSONL
  fixtures.
- CI now includes a locked workspace rustdoc check with warnings denied for public API documentation.
- Public boundary tests for non-strict WASM warning propagation and same-seed bootstrap replayability.
- Experimental GateModel layers: PASS/FAIL/INCONCLUSIVE posterior probabilities, expected
  improvement per expected gate cost, opt-in monotonic coefficient constraints, and a strict JSONL
  `lineprior gate` CLI. These remain diagnostic and are not real-data scheduling evidence.
- UI-automation JSONL fixture plus Python and Node.js CLI round-trip examples.
- `cargo-deny` license policy and CI check for the current dependency graph.
- Initial `lineprior-wasm` JSON-in/JSON-out build and query boundary; npm/wasm-pack packaging is
  intentionally not included yet.

The workspace version is `0.11.0`; this release includes public Rust API additions. The two
established crates are published; the three new crates remain pending their manual first publish.

## [0.10.0] - 2026-08-11

Minor version, not a patch: `TuneParam` (public, non-`#[non_exhaustive]`) gains three new
variants, which is source-breaking for external exhaustive matches -- see the source-compatibility
note below, and this file's own intro on why a `0.x` minor bump can include that.

### Added

- `GateStatus`/`PredictionStatus` (added in 0.9.0, never reachable externally) are now
  re-exported from the crate root.
- `--count-weight` / `--success-weight` / `--score-weight` CLI flags on `build`/`eval`
  (`BuildConfig` fields that already existed in the library, newly wired to the CLI).
- The same three as `lineprior tune --param` keys (`count-weight`, `success-weight`,
  `score-weight`), so they can be swept and saved like every other tunable `BuildConfig` field.
- `cargo audit` CI job.

### Changed

- **Rust source compatibility**: `TuneParam` is a public, non-`#[non_exhaustive]` enum; this
  release adds three new variants (`CountWeight`, `SuccessWeight`, `ScoreWeight`). Additive for
  construction, but source-breaking for any external code that `match`es on `TuneParam`
  exhaustively without a wildcard arm -- the same pattern already documented for `GateObservation`/
  `GatePrediction`/`Error` in the 0.9.0 entry below. `#[non_exhaustive]` was deliberately not added
  to `TuneParam` in this change (a separate, larger decision, not folded into a 3-variant addition).
- Fixed a real dependency-tree security advisory, RUSTSEC-2026-0204 (`crossbeam-epoch`, transitive
  via the `criterion` dev-dependency used only by benches -- never reached a published build).

## [0.9.0] - 2026-07-26

Gate outcome prediction (`gate.rs`, library-only, no CLI surface): Elo observation uncertainty
weighting and out-of-distribution abstention. Pre-real-data-validation, per this project's
established gate.rs convention -- Phase 3 (gate-verdict PASS/FAIL/INCONCLUSIVE probability) and the
acquisition function remain deferred, blocked on the `veridict` project's stopping-rule spec.

### Added

- `GateObservation`: `actual_elo_stddev`, `elo_ci_low`/`elo_ci_high`, `completed_pairs`,
  `gate_status` (new `GateStatus` enum: `Pass`/`Fail`/`Inconclusive`, audit-only), `provenance`
  (opaque caller-composed `BTreeMap<String, String>`, never parsed by this crate). When a measured
  (or CI-implied) stddev is present, it becomes the ridge fit's per-row inverse-variance reliability
  weight in place of `gate_games_played`.
- `GateModelConfig`: `observation_ci_z` (decodes a caller's CI width independently of the model's
  own output-interval `interval_z`), `max_weight_ratio` (clamps each observation's normalized
  reliability weight so one near-noiseless measurement can't dominate the fit),
  `ood_leverage_ratio_threshold`, `ood_missing_fraction_threshold`.
- `GateFitReport`: `dispersion_factor` (an out-of-fold calibration check on the caller's stated
  stddevs themselves), `min_observation_weight`, `max_observation_weight`, `effective_sample_size`,
  `clamped_observation_count`.
- `GatePrediction` / `GateOofPrediction`: `leverage`, `support_distance`, `nearest_group_distance`,
  `missing_feature_fraction`, `prediction_status` (new `PredictionStatus` enum:
  `Supported`/`Extrapolation`/`Unsupported`), `recommend_for_gate` (exactly
  `prediction_status == Supported`, nothing else). Out-of-distribution queries are flagged, never
  silently answered with false confidence -- `expected_elo`/`interval_low`/`interval_high`/
  `probability_positive` are always the model's real prediction, never faked or zeroed because a
  query is unsupported. Every out-of-fold diagnostic is computed from a support model
  (standardizer, group centroids, mean leverage) fit on only that outer CV fold's own training
  rows, proven by dedicated regression tests, not just by inspection.
- `Error::NonPositiveGateStddev`, `ConflictingGateUncertaintySources`,
  `IncompleteGateConfidenceInterval`, `GateEloOutsideConfidenceInterval`: reject an
  `actual_elo_stddev` <= 0; reject specifying both a stddev and a complete confidence interval on
  one observation (exactly one uncertainty source, never a silent priority between them); reject a
  partial confidence interval (only one of `elo_ci_low`/`elo_ci_high`); reject a `gate_elo_delta`
  outside its own stated confidence interval.

### Changed

- **JSON/serde input compatibility is preserved.** Every new `GateObservation` field is
  `Option<T>` or `#[serde(default)]`, so older-shaped JSONL continues to deserialize unchanged
  (tested directly against a pre-existing JSON shape).
- **The Rust API is source-breaking for some usage patterns**, despite the minor version bump
  (acceptable pre-1.0). `GateObservation`, `GatePrediction`, `GateOofPrediction`, and
  `GateFitReport` are public structs with all-public fields, no `Default`, and no
  `#[non_exhaustive]`; each gained public fields this release, so external code constructing them
  via an exhaustive struct literal (naming every field) will not compile until updated.
  `GateModelConfig` also gained fields; it has a `Default` impl, so callers using
  `..Default::default()` are unaffected, but a caller listing every field explicitly is not.
  `Error` gained four new variants; external code exhaustively matching on it without a wildcard
  arm will not compile until updated.
- Two new enums, `GateStatus` and `PredictionStatus`, are used in these structs' field types but
  are **not** re-exported from the crate root (`gate` is a private module, and `lib.rs`'s
  `pub use gate::{...}` omits both) -- external code cannot currently name or match on either type
  at all. Flagged here as a known gap in this release's own API surface, not fixed in this release.
- The `gate` module's public API has been part of a published crates.io release since v0.7.0
  (published 2026-07-19) and v0.7.1 (published 2026-07-20) -- not first here, correcting this
  entry's earlier claim to the contrary. The source-compatibility notes above are a real
  constraint against that already-published v0.7.1 API surface, not just a forward-looking one.

## [0.7.1] - 2026-07-20

Statistical-correctness patch to `gate.rs`'s Round A, found before moving on to real gate-history
validation or an acquisition function.

### Fixed

- Predictive variance dropped intercept uncertainty entirely: a query at the training feature mean
  (including a fully-missing-feature query) collapsed to variance `0.0`, treating the fitted
  intercept as known exactly rather than itself estimated from finite data.
- `sigma2`'s denominator didn't count the intercept's own degree of freedom.
- Hash-mod fold assignment (`fnv1a(group_id) % cv_folds`) had no guarantee every fold got at least
  one group; a hash collision could silently leave a fold empty or several folds overloaded.
  Replaced with deterministic balanced GroupKFold assignment.

## [0.7.0] - 2026-07-19

Adds `gate.rs`: a small, regularized surrogate (`GateModel`) predicting a training candidate's
real-gate Elo delta -- and how much to trust that prediction -- from cheap validation-time
diagnostics, so expensive gate runs can be reserved for candidates likely to be worth them.
Library-only, no CLI surface yet.

### Added

- `GateModel::fit`/`predict`: weighted ridge regression (hand-rolled normal equations, no
  linear-algebra dependency) with `gate_games_played` as the per-row reliability weight, group-aware
  k-fold cross-validation for lambda selection (leave-one-group-out fallback at low group counts),
  closed-form Bayesian-ridge posterior variance for `interval_low`/`interval_high`, and a
  hand-rolled standard normal CDF for `probability_positive`.
- `GateModel::fit_with_validation`: everything `fit` returns, plus a per-candidate out-of-fold
  audit table (`GateOofPrediction`) built from nested cross-validation (an inner CV, scoped to each
  outer fold's own training rows, selects that fold's lambda -- avoiding the optimistic bias of
  reusing the same CV pass that picked the deployed model's lambda).
- `GateObservation`/`GateQuery`/`GatePrediction`: caller-named `BTreeMap<String, f64>` features
  rather than a fixed schema, so the diagnostic set can evolve without a schema break.

## [0.6.0] - 2026-07-12

### Added

- Variable-order context with backoff (`BuildConfig::context_order`): learns
  `(recent-k-actions, state) -> action` for order `1..=k` alongside the always-present order-0
  prior, derived automatically from `sequence_id`/`step`. `query --recent-actions` for
  context-aware CLI queries; new `EvalReport` fields for context-vs-order-0 lift comparison.
- Sequence-level priors via path scoring: `PriorBook::score_sequence` -- given a caller-supplied
  candidate multi-step plan, how much historical precedent backs it, aggregated by minimum (not
  average) confidence across the path.

## [0.5.1] - 2026-07-09

### Added

- crates.io/GitHub discoverability metadata only (keywords, categories, repository link). No code
  changes.

## [0.5.0] - 2026-07-09

### Added

- `--confidence-mode` (`heuristic` (default) / `wilson-lower-bound` / `hybrid`): an actual
  statistical lower bound on an action's success rate, in addition to the original sample-size
  heuristic. `eval --calibration-bins` / `--thresholds` for confidence calibration and
  selective-prediction threshold sweeps.
- Time-decay and source-reliability weighting: `effective_weight = weight * time_decay_multiplier
  * source_reliability_multiplier`, computed once and picked up automatically by `build`, `eval`,
  confidence, and calibration. Both factors default to a no-op (opt-in).
- `lineprior tune`: grid-search `BuildConfig` candidates against held-out `eval` metrics
  (`--objective`, `--param key=v1,v2,...`, `--save-best-config`); `--config <path.json>` on
  `build`/`eval` to load a whole `BuildConfig` from a file.

### Changed

- `--min-confidence`'s meaning now depends on `--confidence-mode`: under `wilson-lower-bound`/
  `hybrid` it becomes success-rate-aware, so a high-count but mostly-failing action that used to
  pass the filter under `heuristic` can now be dropped by it. A real behavior change when switching
  modes on an existing threshold, not purely additive.

## [0.4.0] - 2026-07-06

### Added

- `PriorBook::candidates()`: flat, deterministically-ordered `(state, action)` candidates across
  the whole book, for filtering/sampling without manually nesting through `PriorEntry`/`PriorAction`.
- `BuildStats` (`BuildOutput.stats`, streaming path): counts of what a build's filters actually
  rejected, per threshold (`min_count`/`min_weighted_count`/`min_confidence`/
  `max_actions_per_state`).
- `build_config_fingerprint` + `save_prior_book_with_config`/`load_prior_book_with_config` +
  `Error::BuildConfigMismatch`: detects a cached prior book built under different `BuildConfig`
  values than a caller currently expects.

## [0.3.0] - 2026-07-05

### Added

- `lineprior eval`: holds out part of a JSONL log by `sequence_id` (deterministic hash split),
  builds a prior from the train split, ranks the test split's actual actions against it.
  `EvalReport` metrics: `coverage`, `fallback_rate`, `top1_hit_rate`, `topk_hit_rate`,
  `mean_reciprocal_rank`, `avg_rank_when_found`, `avg_confidence_on_hit`/`on_miss`, `score_lift`.

## [0.2.0] - 2026-07-05

### Added

- Streaming build path (`build_prior_book_from_reader`): fuses parsing and aggregation into one
  pass, bounding memory by unique `(state, action)` pairs rather than total observations
  (~13x peak-RSS reduction on a 1M-observation benchmark, measured and documented).

### Changed

- `PriorAccumulator::finish()` (streaming path only) returns an empty `PriorBook` rather than
  `Error::NoObservations` on empty/all-filtered input, so warnings collected before an empty result
  are never silently discarded. The eager `build_prior_book` path is unchanged (still errors on
  empty input, for compatibility).

## [0.1.0] - 2026-07-05

Initial release: a Rust library and CLI for building domain-agnostic action priors from historical
`(state, action, outcome)` sequences.

### Added

- `build`/`query`/`summary`/`validate` CLI subcommands; JSONL streaming parse with strict/
  non-strict validation.
- Aggregate -> smooth -> normalize -> confidence -> per-state entropy pipeline; deterministic
  output ordering.
- `--draw-value` (partial success credit for draws), `--min-weighted-count`/`--min-confidence`
  filters, `save_prior_book`/`load_prior_book` (read/write API symmetry).
- CI workflow (fmt/clippy/test on push and PR).
