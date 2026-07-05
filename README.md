# lineprior

[日本語](./README_ja.md) / English

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

Useful flags: `--max-step` (drop observations past a given step), `--max-actions-per-state` (keep only the top N candidates), `--tags` (keep only observations carrying at least one of the given tags, comma-separated), `--confidence-k` (tune how fast confidence grows with sample size), `--min-weighted-count` / `--min-confidence` (filter on the weighted count or heuristic confidence directly, instead of just the raw `--min-count`), `--draw-value` (success credit for a `draw` outcome — default `0.5`, since a draw is a genuine partial outcome in adversarial games, not a loss), `--strict` (fail on the first invalid record instead of skipping it with a warning).

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

`success_rate` and `mean_score` are the raw, unsmoothed observed rates (for transparency); `prior` is the smoothed, normalized ranking score; `confidence` is a heuristic sample-size indicator, not a statistical guarantee. `success_rate` credits a `success` outcome as 1.0, a `draw` as `--draw-value` (default 0.5), and a `failure` as 0.0.

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

For a real domain example: [`examples/shogi_opening.jsonl`](./examples/shogi_opening.jsonl) uses `state` = an SFEN string and `action` = a USI move, the mapping described in AGENTS.md's Sekirei integration notes. Its generated prior ([`examples/shogi_prior.jsonl`](./examples/shogi_prior.jsonl)) ranks `7g7f` above `2g2f` despite `2g2f`'s raw observed rate being higher (100% vs. 83%) — `7g7f` has one more supporting observation, and smoothing correctly refuses to let `2g2f`'s smaller sample outrank it on a single-observation-driven perfect record.

## Performance

Measured on an Apple M4 (macOS 26.5.1), release build, 1,000,000 observations across 50,000 unique `(state, action)` pairs (1,000 states × 50 actions):

```text
wall-clock:        1.71s
peak RSS:          ~15.4 MB
```

Reproduce with:

```bash
awk 'BEGIN{
  for (s=0; s<1000; s++) for (a=0; a<50; a++) for (i=0; i<20; i++)
    printf "{\"sequence_id\":\"seq_%d_%d_%d\",\"step\":0,\"state\":\"state_%05d\",\"action\":\"action_%03d\",\"outcome\":\"%s\",\"score\":%.2f,\"weight\":1.0}\n", \
      s, a, i, s, a, (i % 3 == 0 ? "failure" : "success"), 0.5 + (i % 10) * 0.01
}' > large.jsonl
cargo build --release
time ./target/release/lineprior build large.jsonl --out /dev/null --min-count 1
```

Memory is now genuinely bounded by unique `(state, action)` pairs rather than total observation
count, matching AGENTS.md's MVP performance goal: the CLI's `build` command streams straight from
the input file into the prior book via `build_prior_book_from_reader`, folding each observation
into a bounded accumulator as it's parsed instead of collecting a `Vec<Observation>` first. Peak
RSS on the measurement above dropped from ~199MB (the old, fully-materializing path) to ~15.4MB —
about 13x less, for the same 1,000,000-observation input and identical output.

Smaller, checked-in benchmarks live in `crates/lineprior/benches/scoring.rs` (run with `cargo bench -p lineprior`), covering both the eager `build_prior_book` and the streaming `build_prior_book_from_reader` at 1k/10k/50k-observation scales. A dedicated regression test (`crates/lineprior/tests/streaming_memory.rs`, Linux-only, runs in CI) fails if peak memory ever creeps back up toward the old per-observation scaling.

## Academic positioning

`lineprior` is an engineering-oriented Rust implementation inspired by existing ideas in case-based planning, plan reuse, sequence prediction, variable-order Markov models, and policy-guided search. It is not a new theoretical algorithm.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

See [`AGENTS.md`](./AGENTS.md) for the full design spec and roadmap.
