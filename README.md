# lineprior

[![crates.io](https://img.shields.io/crates/v/lineprior.svg)](https://crates.io/crates/lineprior)
[![docs.rs](https://img.shields.io/docsrs/lineprior)](https://docs.rs/lineprior)
[![CI](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/lineprior.svg)](https://github.com/kent-tokyo/lineprior/blob/main/LICENSE-MIT)

[日本語](./README_ja.md) / English

`lineprior` is a Rust library and CLI that builds a reproducible action prior from historical
sequences:

```text
state -> ranked candidate actions
```

It helps a caller decide which action to examine first. It does not decide the best action, explore
online, or replace search, rules, evaluation, or verification. Sparse or unseen states return no
candidates.

## Install

```bash
cargo add lineprior
cargo install lineprior-cli
```

The workspace also contains optional crates for typed adapters, caller-supplied similarity search,
and a thin WASM boundary. They remain independent of the core crate.

## Quick start

An observation is one JSON object per line:

```json
{"sequence_id":"case-001","step":0,"state":"state_a","action":"action_x","outcome":"success","score":0.8,"weight":1.0}
```

Build, inspect, and query a book:

```bash
lineprior validate observations.jsonl
lineprior build observations.jsonl --out prior.jsonl --min-count 2
lineprior summary prior.jsonl
lineprior query prior.jsonl --state state_a --top-k 5
```

An unseen state prints no candidates and exits successfully. The caller owns fallback behavior.

## Data contracts

### Observation JSONL

Required fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `sequence_id` | string | Sequence or case identifier |
| `step` | non-negative integer | Position within the sequence |
| `state` | non-empty string | Opaque state key |
| `action` | non-empty string | Opaque action key |

Optional fields:

| Field | Default | Meaning |
| --- | --- | --- |
| `outcome` | `"unknown"` | `success`, `failure`, `draw`, or `unknown` |
| `score` | `null` | Finite caller-defined score |
| `weight` | `1.0` | Non-negative observation weight |
| `tags` | `[]` | Caller-defined filters |
| `observed_at_unix_seconds` | `null` | Used only when time decay is enabled |
| `source` | `null` | Used by source reliability weights |

Strict mode fails on the first invalid row. Non-strict mode skips safe-to-isolate invalid rows and
returns warnings.

### Prior-book JSONL

`lineprior build` writes a schema-v1 metadata header followed by deterministic state entries:

```text
{"prior_book_schema_version":1,"producer_version":"0.12.1","build_config":{...},"build_config_fingerprint":7308171529403319118}
{"state":"state_a","actions":[{"action":"action_x","count":3,"weighted_count":3.0,"success_rate":0.667,"mean_score":0.633,"prior":0.557,"confidence":0.130}]}
```

The header preserves the producer version and complete `BuildConfig`. The Rust API
`load_prior_book_with_metadata` exposes it; `load_prior_book_with_config` rejects a mismatched
schema-v1 config. Headerless books and the older fingerprint-only header remain readable.

Identical ordered input, config, and producer version produce byte-identical JSONL. `pack` and
`unpack` convert between JSONL and the compact LPB v1 format.

## Ranking and confidence

The default strategy combines log weighted count, smoothed success rate, and smoothed mean score,
then normalizes scores within each state. Missing outcome or score signals are omitted.

`--scoring-strategy` selects `weighted-sum`, `bayesian`, `ucb`, or `softmax`. These are ranking
strategies, not future-performance guarantees.

`--confidence-mode` selects:

- `heuristic`: sample-size heuristic; default and usable without outcomes.
- `wilson-lower-bound`: Wilson lower bound on observed success, with a heuristic fallback when
  decisive outcomes are absent.
- `hybrid`: sample-size heuristic multiplied by the Wilson lower bound.

Use `--min-count`, `--min-weighted-count`, and `--min-confidence` to abstain on weak evidence.
Time decay and source reliability are opt-in. Time decay requires an explicit reference timestamp
so repeated builds remain reproducible. Run `lineprior build --help` for the full configuration
surface.

## Commands

| Command | Purpose |
| --- | --- |
| `build` | Build a JSONL prior book |
| `query` | Query exact state or contextual backoff |
| `summary` | Report coverage, confidence, entropy, and context support |
| `validate` | Validate observation JSONL |
| `eval` | Measure held-out ranking and calibration |
| `tune` | Grid-search `BuildConfig` on one deterministic split |
| `pack` / `unpack` | Convert JSONL and LPB v1 |
| `offpolicy` | Compute IPS, SNIPS, DR, overlap, and optional bootstrap intervals |
| `gate` | Fit the experimental GateModel and optionally predict verdict/acquisition |

`eval` splits by `sequence_id` to avoid leaking one sequence across train and test:

```bash
lineprior eval observations.jsonl \
  --split-by sequence --train-ratio 0.8 \
  --top-k 1,3,5 --calibration-bins 10 --out eval.json
```

`tune` reuses the same split for every candidate and can save the winning config:

```bash
lineprior tune observations.jsonl \
  --param confidence-mode=heuristic,wilson-lower-bound,hybrid \
  --param min-confidence=0.0,0.3,0.5 \
  --objective covered-mrr \
  --out tune.json --save-best-config best_config.json
```

A good held-out ranking score does not prove downstream improvement. Compare prior-on and prior-off
arms in the actual decision loop.

## Optional capabilities

- **Variable-order context:** `--context-order N` learns recent-action context while retaining
  order-zero fallback. Context input must be grouped by sequence with increasing steps.
- **Similarity fallback:** `lineprior-similarity` searches caller-supplied finite feature vectors.
  The core accepts deterministic neighbors and never generates actions.
- **Sequence reuse:** the library can score caller-supplied paths, materialize a `PriorTrie`,
  extract bounded macro-actions, propagate opt-in terminal credit, and merge independently built
  books with explicit source weights.
- **Typed adapters:** `lineprior-adapters` maps Sekirei, UI automation, LLM-agent, and
  retrosynthesis records into generic observations. Parsing, legality, execution, and chemical
  validation stay downstream.
- **WASM:** `lineprior-wasm` exposes JSON-in/JSON-out build and query functions. CI packs and
  clean-installs a web-target npm tarball before a Chromium round trip. It is not registry-published,
  and other browsers are outside the maintained boundary.
- **GateModel:** verdict probabilities, expected-improvement-per-cost acquisition, and monotonic
  constraints are experimental diagnostics. A real-history audit found no admissible shared
  pre-gate feature table, so model comparison remains non-estimable and scope stays frozen.
- **Off-policy evaluation:** IPS/DR consumes caller-supplied propensities and reward-model values.
  Missing support or bad propensities cannot be repaired by the estimator.

## Domain examples

The core treats state and action as opaque keys:

| Domain | State | Action |
| --- | --- | --- |
| UI automation | screen, DOM, or OCR state | click, type, shortcut, wait |
| Optimization | partial-solution key | branch or expansion |
| Games | position string or hash | legal action identifier |
| LLM agent | task/tool context | tool call or plan step |
| Retrosynthesis | molecule/intermediate fingerprint | reaction template |

The checked-in fixtures include
[UI automation](./examples/ui_automation.jsonl),
[a shogi mapping](./examples/shogi_opening.jsonl), and
[Python](./examples/python/roundtrip.py) /
[Node.js](./examples/node/roundtrip.mjs) CLI round trips. Domain formats are adapter concerns; the
core does not parse SFEN, CSA, FEN, PGN, prompts, or molecules.

There is no Python binding. The maintained browser package boundary and clean-install evidence are
defined in the [runtime compatibility document](./docs/measurements/ecosystem-compatibility.md).

## Evidence and limitations

- Historical bias and distribution shift can make a prior worse.
- `confidence` describes observed support or an observed-rate bound. It is not certainty about a
  future action.
- Similar states do not generalize automatically; feature construction belongs to the caller.
- The project does not perform causal inference or counterfactual action generation.
- Synthetic fixtures test contracts, not decision quality.
- A historical benchmark does not establish cross-machine performance.
- A real Sekirei holdout rejected the current similarity adapter. Real-log audits found IPS/DR
  non-estimable because propensities were not logged and GateModel non-estimable because candidate
  identity, group IDs, and shared pre-gate features were not recorded. Trie and macro-action
  improvement gates remain open.

Streaming build memory is proportional to unique state/action pairs, plus context tuples when
enabled. The historical Apple M4 synthetic snapshot is retained under
[`docs/benchmarks/`](./docs/benchmarks/), clearly separated from real-data evidence.

## Documentation

- [Documentation index](./docs/README.md)
- [Similarity real-data protocol](./docs/measurements/similarity-real-data.md)
- [IPS/DR real-log protocol](./docs/measurements/offpolicy-real-data.md)
- [GateModel real-history readiness](./docs/measurements/gate-model-real-history.md)
- [Runtime and WASM evidence](./docs/measurements/ecosystem-compatibility.md)
- [Sekirei downstream case study](./docs/case-studies/sekirei-opening-book.md)
- [Publishing checklist](./docs/publishing.md)
- [Release history](./CHANGELOG.md)
- [Rust API](https://docs.rs/lineprior)

The internal `ROADMAP.md` is intentionally not published.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
sh scripts/check_candidate_contract.sh
```

`lineprior` is an engineering-oriented Rust implementation inspired by case-based planning, plan
reuse, sequence prediction, variable-order Markov models, policy-guided search, and temporal
abstraction. It does not claim a new theoretical algorithm.

Licensed under MIT or Apache-2.0.
