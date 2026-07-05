# AGENTS.md

## Project: lineprior

`lineprior` is a Rust library and CLI for building domain-agnostic action priors from historical action sequences.

It is not a shogi opening book library.
It is not a chess-specific book format.
It is not a planner, solver, or game engine.

It is a small reusable component that answers:

> Given this state, what actions have historically worked well from here?

The core idea is inspired by:

* case-based planning
* plan reuse
* sequence prediction
* variable-order Markov models
* policy priors
* search-control knowledge
* macro-actions / temporal abstraction

`lineprior` should be useful for games, search, automation, agents, optimization, and other domains where past successful action sequences can guide future decisions.

## Core Mission

Build a lightweight Rust library that consumes historical sequence data and produces a prior book.

A prior book maps:

```text
state -> ranked candidate actions
```

Each candidate action may include:

* count
* weighted count
* success rate
* mean score
* confidence
* entropy contribution
* prior probability
* tags / metadata

Example domains:

```text
Shogi:
  state  = SFEN or Zobrist key
  action = USI move

Chess:
  state  = FEN or position hash
  action = UCI move

UI automation:
  state  = screen hash / DOM state / OCR state
  action = click / type / shortcut / wait

LLM agents:
  state  = task state / tool context
  action = tool call / plan step

Retrosynthesis:
  state  = molecule or intermediate fingerprint
  action = reaction template

Optimization:
  state  = partial solution
  action = branch / candidate expansion
```

The core library must not depend on any one of these domains.

## Design Principles

### 1. Domain-agnostic core

Core names must use generic terms:

* state
* action
* sequence
* step
* outcome
* score
* prior
* confidence
* observation
* book
* policy

Avoid domain-specific names in core APIs:

* move
* game
* board
* SFEN
* CSA
* chess
* shogi
* prompt
* document
* molecule

Domain-specific integrations belong in examples, adapters, or separate crates.

### 2. Prior, not oracle

`lineprior` does not decide the best action by itself.

It provides prior knowledge to another system.

Correct usage:

```text
lineprior suggests candidate actions.
The caller may use search, evaluation, rules, or verification before acting.
```

Incorrect usage:

```text
lineprior always knows the best action.
```

When data is sparse or low-confidence, `lineprior` should prefer fallback behavior.

### 3. Conservative by default

Bad priors can make systems worse.

The library should avoid over-trusting weak data.

Default behavior should include:

* minimum count threshold
* smoothing
* optional confidence filtering
* entropy-aware warnings
* fallback when state is unseen
* clear metadata about sample size

A low-sample action must not appear as highly certain just because it has a 100% success rate from one sample.

### 4. JSONL first

The first stable input and output format should be JSONL.

JSONL is easy to generate from many systems and easy to process in pipelines.

Input observation example:

```json
{"sequence_id":"case-001","step":0,"state":"state_a","action":"action_x","outcome":"success","score":1.0,"weight":1.0}
```

Output prior entry example:

```json
{"state":"state_a","actions":[{"action":"action_x","count":42,"weighted_count":39.5,"success_rate":0.71,"mean_score":0.62,"prior":0.54,"confidence":0.82}]}
```

### 5. Small core, useful CLI

The Rust library should expose the core logic.

The CLI should be a thin wrapper around the library.

A single crate is acceptable at first.
A workspace split is acceptable once the API stabilizes.

Possible layout:

```text
lineprior       # library
lineprior-cli   # command line wrapper
```

### 6. Explain why

For non-trivial code, comments should explain why the behavior exists, not merely what the code does.

Good comment:

```rust
// We apply smoothing because rare actions can otherwise get a misleading
// 100% success rate from one lucky observation.
```

Bad comment:

```rust
// Increment count.
```

The goal is maintainability for future contributors and AI agents.

## Non-goals

Do not build these initially:

* shogi-specific opening book format
* chess Polyglot compatibility
* CSA parser
* SFEN parser
* USI integration
* GUI
* web dashboard
* database server
* distributed training system
* reinforcement learning framework
* neural network policy model
* full planner
* full search algorithm

These may be adapters or downstream projects later.

`lineprior` should build and query priors.
It should not own every way of producing or consuming actions.

## Core Data Model

Suggested public types:

```rust
pub struct Observation {
    pub sequence_id: String,
    pub step: u32,
    pub state: String,
    pub action: String,
    pub outcome: Outcome,
    pub score: Option<f64>,
    pub weight: f64,
    pub tags: Vec<String>,
}

pub enum Outcome {
    Success,
    Failure,
    Draw,
    Unknown,
}

pub struct BuildConfig {
    pub min_count: u64,
    pub max_step: Option<u32>,
    pub smoothing_alpha: f64,
    pub score_weight: f64,
    pub success_weight: f64,
    pub count_weight: f64,
    pub max_actions_per_state: Option<usize>,
}

pub struct PriorAction {
    pub action: String,
    pub count: u64,
    pub weighted_count: f64,
    pub success_rate: Option<f64>,
    pub mean_score: Option<f64>,
    pub prior: f64,
    pub confidence: f64,
}

pub struct PriorEntry {
    pub state: String,
    pub actions: Vec<PriorAction>,
}

pub struct PriorBook {
    pub entries: std::collections::HashMap<String, Vec<PriorAction>>,
}
```

The exact API can evolve, but keep the conceptual model simple.

## Input Schema

Initial JSONL input should support these fields:

```json
{
  "sequence_id": "case-001",
  "step": 0,
  "state": "state_key",
  "action": "action_id",
  "outcome": "success",
  "score": 0.8,
  "weight": 1.0,
  "tags": ["trusted"]
}
```

Required fields:

* `sequence_id`
* `step`
* `state`
* `action`

Optional fields:

* `outcome`
* `score`
* `weight`
* `tags`

Defaults:

```text
outcome = unknown
score   = null
weight  = 1.0
tags    = []
```

Invalid records should produce clear errors in strict mode.
In non-strict mode, invalid records may be skipped with warnings.

## Output Schema

Output JSONL should contain one prior entry per state:

```json
{
  "state": "state_key",
  "actions": [
    {
      "action": "action_a",
      "count": 120,
      "weighted_count": 113.5,
      "success_rate": 0.64,
      "mean_score": 0.71,
      "prior": 0.58,
      "confidence": 0.91
    }
  ]
}
```

The output must be stable enough for downstream tools to consume.

Breaking schema changes should be intentional and documented.

## Prior Scoring

The first implementation may use a simple weighted scoring model.

Recommended initial formula:

```text
raw_score =
  count_weight   * log(1 + weighted_count)
+ success_weight * smoothed_success_rate
+ score_weight   * smoothed_mean_score
```

Then normalize actions per state:

```text
prior = raw_score / sum(raw_score for actions in state)
```

Use smoothing so low-count actions do not dominate.

Example smoothing:

```text
smoothed_success_rate =
  (successes + alpha * global_success_rate) / (trials + alpha)
```

If no outcome data exists, fall back to count-based priors.

If no score data exists, ignore score contribution.

If both outcome and score are missing, the prior should be based on weighted count only.

## Confidence

Confidence should reflect sample reliability.

Start simple:

```text
confidence = weighted_count / (weighted_count + k)
```

where `k` is configurable.

This is not a statistical guarantee.
Document it as a heuristic confidence score.

Later versions may add Wilson intervals, Bayesian estimates, or bootstrap confidence.

## Entropy and Diversity

The library should optionally report action entropy for each state.

High entropy means many actions are similarly common or similarly successful.

This can help callers decide whether to trust the prior.

Example:

```text
low entropy:
  one action dominates, prior may be useful

high entropy:
  many actions compete, fallback search may be safer
```

Do not force entropy filtering in MVP, but design the report so it can be added.

## Fallback Philosophy

If a state is unseen, return no action candidates.

If all candidates fail thresholds, return no action candidates.

Do not invent actions.

The caller is responsible for fallback behavior, such as normal search or default policy.

## CLI

The CLI should be simple.

### Build a prior book

```bash
lineprior build observations.jsonl \
  --out prior.jsonl \
  --min-count 20 \
  --smoothing-alpha 5.0
```

### Query a prior book

```bash
lineprior query prior.jsonl \
  --state state_key \
  --top-k 5
```

### Summarize a prior book

```bash
lineprior summary prior.jsonl
```

### Validate input

```bash
lineprior validate observations.jsonl
```

### Example with step filtering

```bash
lineprior build observations.jsonl \
  --max-step 40 \
  --min-count 10 \
  --out prior_opening.jsonl
```

`--max-step` must remain domain-neutral.
Do not call it `--max-ply` in the core CLI.

## Exit Codes

CLI exit codes should be deterministic:

```text
0 = success
1 = completed with warnings
2 = no usable data
3 = invalid input or configuration
4 = internal error
```

Do not require users to parse prose to know whether a command succeeded.

## Rust Guidelines

Use stable Rust.

Preferred dependencies:

* `serde`
* `serde_json`
* `clap`
* `thiserror`
* `anyhow` only in binaries
* `indexmap` only if deterministic order is needed
* `rand` only if sampling is added

Avoid heavy dependencies in the core library.

Core library should not print to stdout or stderr.

CLI owns user-facing output.

## Error Handling

Never panic on user input.

Return typed errors from the library.

Examples of invalid input:

* invalid JSON
* missing required field
* empty state
* empty action
* negative weight
* NaN score
* unsupported outcome value
* duplicate malformed records
* no observations after filtering

In strict mode, fail fast.
In non-strict mode, collect warnings and continue when safe.

## Determinism

The same input and config should produce the same output.

Sort output deterministically:

* states lexicographically
* actions by descending prior
* tie-break by action string

Determinism matters for CI, diffs, and reproducibility.

## Performance

The library should handle large JSONL files.

MVP target:

* 1 million observations on a typical developer machine
* streaming input parse
* bounded memory proportional to unique `(state, action)` pairs
* no unnecessary cloning of large strings where avoidable

Optimization should not make the code unreadable.

Prefer clear implementation first.
Optimize after profiling.

## Testing

Every feature must have tests.

Required tests:

* parse valid JSONL
* reject malformed JSONL
* default missing optional fields
* aggregate counts
* aggregate weighted counts
* compute success rate
* compute mean score
* apply min count filter
* smooth low-count success rate
* normalize priors
* deterministic output ordering
* query unseen state
* query known state
* strict vs non-strict invalid record handling
* CLI build command
* CLI query command

Edge cases:

* empty input
* all unknown outcomes
* all failures
* all successes
* one observation only
* NaN score
* negative weight
* zero weight
* extremely large counts
* duplicate sequence IDs
* multiple actions per state
* high entropy state

## Fixtures

Include small fixtures in `tests/fixtures`.

Suggested fixtures:

```text
simple_success.jsonl
mixed_outcomes.jsonl
weighted_observations.jsonl
missing_optional_fields.jsonl
invalid_records.jsonl
high_entropy.jsonl
```

Each fixture should be tiny and human-readable.

## Suggested Repository Layout

```text
.
├── AGENTS.md
├── README.md
├── Cargo.toml
├── crates
│   ├── lineprior
│   │   └── src
│   │       ├── lib.rs
│   │       ├── input.rs
│   │       ├── model.rs
│   │       ├── build.rs
│   │       ├── score.rs
│   │       ├── query.rs
│   │       ├── report.rs
│   │       └── error.rs
│   └── lineprior-cli
│       └── src
│           └── main.rs
├── examples
│   ├── observations.jsonl
│   └── prior.jsonl
└── tests
    ├── cli.rs
    └── fixtures
```

A single-crate layout is acceptable for the first commit if it keeps the API clean.

## README Requirements

The README should explain:

* what `lineprior` is
* what it is not
* why action priors are useful
* how to build a prior book
* how to query a prior book
* input schema
* output schema
* limitations
* examples for at least two domains

Use domain-neutral examples first.

Then optionally show:

* shogi opening prior
* UI automation action prior
* agent tool-call prior

Do not make the project look shogi-specific.

## Academic Positioning

The README may mention that `lineprior` is inspired by:

* case-based planning
* plan reuse
* sequence prediction
* variable-order Markov models
* policy-guided search
* temporal abstraction

Do not overclaim novelty.

Correct wording:

```text
lineprior is an engineering-oriented Rust implementation inspired by existing ideas in case-based planning, sequence prediction, and policy-guided search.
```

Incorrect wording:

```text
lineprior is a new theoretical algorithm.
```

## Integration Philosophy

`lineprior` should remain independent.

Downstream projects may adapt their domain data into lineprior observations.

Dependency direction:

```text
sekirei / robost / renkin / agent tools
        depend on
lineprior
```

`lineprior` must not depend on those projects.

## Sekirei Integration Example

Sekirei can use `lineprior` for opening-phase action priors.

But keep this in examples or adapter code, not in core.

Mapping:

```text
state  = SFEN or Zobrist key
action = USI move
step   = ply
score  = optional game result or engine-evaluated quality
```

Possible Sekirei flow:

```text
CSA games
→ domain adapter converts to lineprior observations
→ lineprior build
→ prior JSONL
→ Sekirei queries prior for early-game candidate moves
→ fallback to normal search when no reliable prior exists
```

`lineprior` should not parse CSA or USI in core.

## Feature Roadmap

### Phase 1: MVP

1. Create Rust crate and CLI.
2. Define observation model.
3. Parse JSONL.
4. Aggregate `(state, action)` statistics.
5. Compute count-based prior.
6. Add outcome-based success rate.
7. Add score-based mean score.
8. Add smoothing.
9. Add min-count filtering.
10. Emit prior JSONL.
11. Implement query command.
12. Add tests and fixtures.

### Phase 2: Better Priors

1. Add confidence score.
2. Add entropy per state.
3. Add tag filtering.
4. Add max-step filtering.
5. Add top-k output.
6. Add weighted source support.
7. Add summary report.
8. Add compact binary format if needed.

### Phase 3: Advanced Sequence Support

1. Prefix-tree representation.
2. Variable-order context fallback.
3. Sequence-level priors.
4. Macro-action suggestions.
5. Confidence intervals.
6. Time-decay weighting.
7. Multi-source merging.

### Phase 4: Integrations

1. Sekirei adapter example.
2. UI automation example.
3. LLM agent example.
4. Retrosynthesis route example.
5. `veridict` evaluation recipe for prior on/off comparison.

## Quality Bar

A feature is not complete until:

* it has tests
* it has a fixture or example
* invalid input behavior is tested
* output is deterministic
* README or docs are updated
* assumptions are documented

## Development Commands

Every change should pass:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

If a workspace is used, run checks from the workspace root.

## Code Style

Prefer clarity over cleverness.

Statistical and scoring code should be explicit and readable.

Avoid magic constants.
If a constant is needed, name it and explain why it exists.

Example:

```rust
const DEFAULT_CONFIDENCE_K: f64 = 20.0;
// We use k=20 so confidence grows slowly for low-sample actions.
// This prevents one-off successes from dominating the prior.
```

## Security and Safety

`lineprior` processes user-supplied files.

Do not execute input content.

Do not load remote resources.

Do not create files outside requested output paths.

Avoid path traversal in future archive or batch features.

## Documentation Tone

Be honest.

Good:

```text
lineprior can improve candidate ordering when historical sequences are relevant and representative.
```

Bad:

```text
lineprior guarantees better decisions.
```

Good:

```text
If historical data is biased, the prior will also be biased.
```

Bad:

```text
lineprior learns common sense automatically.
```

## Project Identity

`lineprior` is a small, sharp, domain-agnostic prior book.

It should help developers reuse historical action sequences without locking them into a specific game, model, planner, or agent framework.

When in doubt, keep the core generic and push domain logic to adapters.
