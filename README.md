# lineprior

[![crates.io](https://img.shields.io/crates/v/lineprior.svg)](https://crates.io/crates/lineprior)
[![docs.rs](https://img.shields.io/docsrs/lineprior)](https://docs.rs/lineprior)
[![CI](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/lineprior.svg)](https://github.com/kent-tokyo/lineprior/blob/main/LICENSE-MIT)

[日本語](./README_ja.md) / English

`lineprior` is an independent Rust library and CLI for building reproducible action priors from
historical sequences:

```text
state -> ranked candidate actions
```

It ranks observed candidate actions that a caller may examine first. It does not choose the best
action, explore online, or replace search, rules, evaluation, execution, or verification. State
and action values are opaque keys; sparse or unseen states return no candidates. The caller owns
legality, fallback behavior, and every final decision.

Use `lineprior` when a system already has a final verifier or search procedure, candidate evaluation
is worth reducing, and relevant historical sequences are available. It is not an action generator,
planner, solver, game engine, online learner, or domain parser.

## Quick start

```bash
cargo add lineprior
cargo install lineprior-cli
```

Create an observation file with one JSON object per line:

```json
{"sequence_id":"case-001","step":0,"state":"state_a","action":"action_x","outcome":"success","score":0.8,"weight":1.0}
```

Then validate, build, inspect, and query the book:

```bash
lineprior validate observations.jsonl
lineprior build observations.jsonl --out prior.jsonl --min-count 2 \
  --artifact-manifest manifest.json
lineprior verify-artifact --manifest manifest.json --input observations.jsonl --book prior.jsonl
lineprior summary prior.jsonl
lineprior query prior.jsonl --state state_a --top-k 5
```

An unseen state prints no candidates and exits successfully. The caller owns fallback behavior.

## Data contract

Every observation requires non-empty `sequence_id`, `state`, and `action` strings plus a
non-negative `step`. Optional fields are:

| Field | Default | Meaning |
| --- | --- | --- |
| `outcome` | `"unknown"` | `success`, `failure`, `draw`, or `unknown` |
| `score` | `null` | Finite caller-defined score |
| `weight` | `1.0` | Finite, non-negative observation weight |
| `tags` | `[]` | Caller-defined filters |
| `observed_at_unix_seconds` | `null` | Used only with time decay |
| `source` | `null` | Used by source reliability weights |

Strict mode stops at the first invalid row. Non-strict mode skips rows that can be isolated safely
and reports warnings.

`build` writes a schema-v1 metadata header followed by deterministically ordered state entries:

```text
{"prior_book_schema_version":1,"producer_version":"0.12.7","build_config":{...},"build_config_fingerprint":7308171529403319118}
{"state":"state_a","actions":[{"action":"action_x","count":3,"weighted_count":3.0,"success_rate":0.667,"mean_score":0.633,"prior":0.557,"confidence":0.130}]}
```

The header records the producer version and complete `BuildConfig`. Headerless books and the older
fingerprint-only header remain readable. `pack` and `unpack` convert JSONL to and from compact LPB
v1.

For archived measurements, add `--artifact-manifest manifest.json`. The sidecar links the exact
input and book bytes by SHA-256 and can record caller-owned dataset, split, adapter, and lineage
fields without changing the book schema. `verify-artifact` checks both byte hashes, manifest
semantics, and schema-v1 book metadata. Legacy or headerless books return a provenance warning.
Artifact schemas and validation commands are indexed in
[`docs/README.md`](./docs/README.md).

For reproducible exact-match telemetry, `batch-query` validates a schema-v1 book against its
manifest before writing one ordered result per versioned query row:

```bash
lineprior batch-query prior.jsonl examples/batch_queries.jsonl \
  --artifact-manifest manifest.json --out batch-report.jsonl --top-k 5
```

The report records exact input and book hashes, the build fingerprint, thresholds, candidate
counts, and explicit `matched`, `unseen_state`, or `insufficient_support` status. It only copies
observed actions from the frozen book; it does not generate actions or predict outcomes. See the
[batch-query contract](./docs/batch-query.md).

## Ranking, confidence, and evaluation

The default ranker combines log weighted count, smoothed success rate, and smoothed mean score,
then normalizes within each state. `--scoring-strategy` also supports `bayesian`, `ucb`, and
`softmax`. These are ranking strategies, not guarantees of future performance.

`--confidence-mode` accepts:

- `heuristic`: sample-size support; the default and usable without outcomes.
- `wilson-lower-bound`: a lower bound on observed success, with heuristic fallback when decisive
  outcomes are absent.
- `hybrid`: the heuristic multiplied by the Wilson lower bound.

Use count, weighted-count, and confidence thresholds to abstain on weak evidence. Time decay and
source weights are opt-in; time decay requires an explicit reference timestamp so builds remain
reproducible.

Held-out evaluation splits by `sequence_id` to avoid train/test leakage:

```bash
lineprior eval observations.jsonl \
  --split-by sequence --train-ratio 0.8 \
  --top-k 1,3,5 --calibration-bins 10 --out eval.json
```

A good ranking score does not prove a downstream benefit. Compare prior-on and prior-off arms in
the real decision loop.

## CLI commands

| Command | Purpose |
| --- | --- |
| `build` | Build a JSONL prior book |
| `query` | Query an exact state or contextual backoff |
| `summary` | Report coverage, confidence, entropy, and context support |
| `validate` | Validate observation, off-policy, or gate-history JSONL |
| `verify-artifact` | Verify a sidecar against input bytes and prior-book metadata |
| `batch-query` | Write deterministic exact-match telemetry for versioned query rows |
| `eval` / `tune` | Evaluate one config or tune it on a fixed split |
| `pack` / `unpack` | Convert JSONL and LPB v1 |
| `offpolicy` | Compute IPS, SNIPS, DR, overlap, and bootstrap intervals |
| `gate` | Run the experimental GateModel diagnostics |

Run `lineprior <command> --help` for all options.

## Optional capabilities

- `PriorBook::query_borrowed` returns ranked `&PriorAction` values through a reusable
  `QueryScratch` index buffer for repeated read-only lookups. The existing owned `query` API
  remains unchanged. Canonically built books reuse their stored ranking; ad-hoc unsorted books are
  normalized at the query boundary.
- Variable-order context, sequence scoring, `PriorTrie`, bounded macro-actions, terminal credit,
  and weighted multi-source book merging are available in the core library.
- `lineprior-similarity` searches caller-supplied finite feature vectors. It does not generate
  actions or embeddings.
- `lineprior-adapters` provides typed conversion boundaries for structured records, UI automation,
  tool-using agents, and scientific workflows. Parsing, legality, execution, and domain validation
  remain with the downstream application.
- `lineprior-wasm` exposes initialized JSON-in/JSON-out build and query functions. Its Rust crate
  is released with the workspace; CI also verifies a clean-installed web-target tarball in
  Chromium. That web package is not published to the npm registry, and there is no Python binding.
- GateModel and off-policy evaluation are diagnostic tools. They cannot repair missing features,
  support, propensities, or outcomes.

The core does not parse domain formats, prompts, UI trees, or molecules. Example JSONL and Python,
Node.js, and browser round trips are under [`examples/`](./examples/).

## Evidence and limitations

- Historical bias and distribution shift can make a prior worse.
- `confidence` describes observed support or an observed-rate bound, not certainty about a future
  action.
- Similar states do not generalize automatically; feature construction belongs to the caller.
- The project does not perform causal inference or counterfactual action generation.
- Synthetic fixtures verify contracts, not decision quality. Historical benchmarks do not
  establish cross-machine performance.
- A held-out reference integration rejected its current similarity configuration. Existing logs
  also lacked the information needed to estimate IPS/DR or validate GateModel. Exact match plus
  abstention remains the supported default; Trie and macro-action downstream gains remain
  unverified.

Streaming build memory is proportional to unique state/action pairs, plus context tuples when
enabled. See the [documentation index](./docs/README.md) for measurement protocols, evidence,
publishing, examples, and schemas.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
sh scripts/check_candidate_contract.sh
```

`lineprior` is an engineering-oriented implementation inspired by case-based planning, plan reuse,
sequence prediction, variable-order Markov models, policy-guided search, and temporal abstraction.
It does not claim a new theoretical algorithm.

Licensed under MIT or Apache-2.0.
