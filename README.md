# lineprior

`lineprior` is a Rust library and CLI for building domain-agnostic **action priors** from historical action sequences. Given a state, it answers:

> What actions have historically worked well from here?

It is not a shogi opening book library, a chess-specific book format, a planner, a solver, or a game engine. It is a small, reusable component that turns a log of past `(state, action, outcome)` steps into a ranked list of candidate actions per state — useful for games, search, automation, agents, optimization, and any other domain where past successful sequences can guide future decisions.

## What it is not

`lineprior` does not decide the best action by itself. It is a **prior, not an oracle**:

- It suggests candidate actions with a count, rate, and confidence attached.
- The caller is expected to combine this with search, evaluation, rules, or verification before acting.
- When data is sparse or a state is unseen, it returns no candidates rather than inventing one.

If historical data is biased, the prior will be biased too. `lineprior` can improve candidate ordering when historical sequences are relevant and representative — it does not guarantee better decisions.

## Building a prior book

```bash
lineprior build observations.jsonl \
  --out prior.jsonl \
  --min-count 1 \
  --smoothing-alpha 5.0
```

Useful flags: `--max-step` (drop observations past a given step), `--max-actions-per-state` (keep only the top N candidates), `--tags` (keep only observations carrying at least one of the given tags, comma-separated), `--confidence-k` (tune how fast confidence grows with sample size), `--strict` (fail on the first invalid record instead of skipping it with a warning).

## Querying a prior book

```bash
lineprior query prior.jsonl --state state_a --top-k 5
```

An unseen state prints nothing and still exits `0` — that's the expected fallback behavior, not an error.

## Other commands

```bash
lineprior summary prior.jsonl      # coverage, average confidence, per-state entropy
lineprior validate observations.jsonl   # parse and report issues without building
```

## Input schema

One JSON object per line:

```json
{"sequence_id":"case-001","step":0,"state":"state_a","action":"action_x","outcome":"success","score":0.8,"weight":1.0,"tags":["trusted"]}
```

Required: `sequence_id`, `step`, `state`, `action`.
Optional, with defaults: `outcome` (`unknown`), `score` (`null`), `weight` (`1.0`), `tags` (`[]`).

## Output schema

One JSON object per state, actions ranked by descending prior:

```json
{"state":"state_a","actions":[{"action":"action_x","count":3,"weighted_count":3.0,"success_rate":0.667,"mean_score":0.633,"prior":0.557,"confidence":0.130}]}
```

`success_rate` and `mean_score` are the raw, unsmoothed observed rates (for transparency); `prior` is the smoothed, normalized ranking score; `confidence` is a heuristic sample-size indicator, not a statistical guarantee.

## Limitations

- Confidence is a heuristic (`weighted_count / (weighted_count + k)`), not a statistical confidence interval.
- A low-sample action does not get reported as certain just because it has a 100% success rate from one observation — smoothing pulls it toward the dataset's overall rate.
- `lineprior` never invents actions: an unseen state or a state with no candidates above threshold returns an empty result.
- The library does not parse any domain-specific format (SFEN, CSA, USI, FEN, PGN, etc.) — that mapping is the caller's job.

## Examples for two domains

The same `observations.jsonl` shape works whether the "state" is a board position or a UI screen:

```text
Automation:
  state  = "checkout_page"
  action = "click_pay_button"

Optimization:
  state  = "partial_solution_hash_42"
  action = "branch_left"
```

Domain-specific mappings (e.g. a chess/shogi position as `state`, a UCI/USI move as `action`) belong in adapters outside this crate, not in `lineprior` itself.

## Academic positioning

`lineprior` is an engineering-oriented Rust implementation inspired by existing ideas in case-based planning, plan reuse, sequence prediction, variable-order Markov models, and policy-guided search. It is not a new theoretical algorithm.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

See [`AGENTS.md`](./AGENTS.md) for the full design spec and roadmap.
