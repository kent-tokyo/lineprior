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

Useful flags: `--max-step` (drop observations past a given step), `--max-actions-per-state` (keep only the top N candidates), `--tags` (keep only observations carrying at least one of the given tags, comma-separated), `--confidence-k` (tune how fast confidence grows with sample size), `--confidence-mode` (`heuristic` (default), `wilson-lower-bound`, or `hybrid` — see "Confidence modes" below), `--confidence-z` (z-score for the Wilson lower bound, default `1.96`, ignored under `heuristic`), `--min-weighted-count` / `--min-confidence` (filter on the weighted count or confidence directly, instead of just the raw `--min-count`), `--draw-value` (success credit for a `draw` outcome — default `0.5`, since a draw is a genuine partial outcome in adversarial games, not a loss), `--strict` (fail on the first invalid record instead of skipping it with a warning).

`--min-confidence`'s meaning depends on `--confidence-mode`: under `heuristic` it's a pure sample-size floor, blind to outcome. Under `wilson-lower-bound`/`hybrid` it's success-rate-aware, so a high-count but mostly-failing action that used to pass the filter can now be dropped by it — switching `--confidence-mode` on an existing `--min-confidence` threshold is a real behavior change, not just an additive one.

### Confidence modes

- `heuristic` (default): `weighted_count / (weighted_count + confidence_k)` — a sample-size heuristic, blind to outcome. Not a statistical guarantee, but works even for score-only datasets with no outcome labels at all.
- `wilson-lower-bound`: the Wilson score interval lower bound on the action's success rate — an actual statistical lower bound, useful once `outcome` labels are meaningful. Falls back to `heuristic` for an action with no decisive-outcome observations (nothing to bound).
- `hybrid`: `heuristic * wilson-lower-bound`, so both low sample size *and* a weak success rate pull confidence down. Same fallback as `wilson-lower-bound` when there's no outcome data.

Weighted/fractional observations (`--weight`, `draw` outcomes under `--draw-value`) feed the Wilson bound through an effective sample size (`sum(weight)^2 / sum(weight^2)`, Kish's formula) rather than the raw weighted count — an engineering approximation, exact for uniform weight `1.0` observations.

`build` also prints a one-line summary of what its filters actually did, e.g. `stats: 950/1000 observations kept, 42/50 candidates kept (5 by min_count, ...)` — useful for sanity-checking your own pre-filtering (e.g. a domain-specific ply/depth cutoff) against `--min-count`/etc. without re-deriving the numbers by hand. As a library, this is `BuildOutput.stats` (a `BuildStats`) returned alongside the book by `build_prior_book_from_reader`.

## Querying a prior book

```bash
lineprior query prior.jsonl --state state_a --top-k 5
```

An unseen state prints nothing and still exits `0` — that's the expected fallback behavior, not an error.

As a library, `PriorBook::candidates()` gives you every `(state, action)` candidate across the whole book as a flat `Vec<(String, PriorAction)>`, for callers filtering or sampling candidates directly (e.g. building a domain-specific "opening suite") instead of working through the nested per-state structure `entries_sorted()` returns.

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

`success_rate` and `mean_score` are the raw, unsmoothed observed rates (for transparency); `prior` is the smoothed, normalized ranking score; `confidence` is a heuristic sample-size indicator by default, or a real Wilson-bound statistical lower bound under `--confidence-mode wilson-lower-bound`/`hybrid` (see "Confidence modes" above). `success_rate` credits a `success` outcome as 1.0, a `draw` as `--draw-value` (default 0.5), and a `failure` as 0.0.

`lineprior build`'s CLI output (and the library's `save_prior_book_with_config`) prepends a header line carrying a fingerprint of the `BuildConfig` used to build it, e.g. `{"build_config_fingerprint":7592859384087124328}`. `load_prior_book`/`lineprior query`/`lineprior summary` all skip this line transparently — it doesn't change how you read a prior book day to day.

## Detecting a stale cached prior book

If you cache a prior book on disk and rebuild it later under different `BuildConfig` values (a different `--smoothing-alpha`, `--confidence-k`, etc.), the raw `confidence`/`prior` numbers in the old file were computed under the *old* config's semantics — reusing it silently can be misleading. As a library:

```rust
// When saving, embed the config that produced it:
save_prior_book_with_config(&book, &config, writer)?;

// Later, check a cached file against your current config before trusting it:
match load_prior_book_with_config(reader, &config) {
    Ok(book) => { /* config matches (or the file predates this check) */ }
    Err(Error::BuildConfigMismatch { .. }) => { /* stale -- rebuild */ }
    Err(e) => { /* other error */ }
}
```

A file saved via plain `save_prior_book` (or by a version of lineprior that predates this) has no fingerprint to compare against, so `load_prior_book_with_config` accepts it unconditionally — there's nothing to detect drift against. The fingerprint is stable *within a given lineprior version*, not guaranteed forever-stable across upgrades (it hashes a JSON encoding of `BuildConfig`, and floats' exact byte layout isn't itself a cross-version guarantee) — it's meant to catch a stale cache within one project's lifetime, not serve as a long-term archival checksum.

Upgrading to a lineprior version that adds new `BuildConfig` fields (like `confidence_mode`/`confidence_z`) changes the fingerprint for *every* config, even `heuristic` mode where `confidence_z` is inert — so a prior book cached before upgrading will trip `BuildConfigMismatch` once after upgrading. That's the fingerprint mechanism working as intended, not a regression.

## Limitations

- By default (`--confidence-mode heuristic`), confidence is a sample-size heuristic (`weighted_count / (weighted_count + k)`), not a statistical confidence interval. This remains the default for backward compatibility and for score-only datasets with no outcome labels. `--confidence-mode wilson-lower-bound`/`hybrid` give an actual statistical lower bound on the success rate when outcome data is meaningful (see "Confidence modes" above) — but they're still a lower bound on the *observed* rate, not a guarantee about future actions if the underlying data is biased or non-stationary.
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

## Evaluating a prior

A prior is only useful if it actually ranks the right action highly on data it wasn't built
from. `lineprior eval` holds out part of the observation log, builds a prior from the rest, and
reports ranking-quality metrics on the held-out slice:

```bash
lineprior eval observations.jsonl \
  --split-by sequence --train-ratio 0.8 --top-k 1,3,5 --out eval.json
```

The split is by `sequence_id`, not by individual observation, so every step of the same sequence
lands on the same side — otherwise later steps could leak information about earlier ones across
the train/test boundary. The split is a deterministic hash of the id, so re-running `eval` with
the same `--train-ratio` reproduces the same split.

Headline fields in the JSON report:

- `top1_hit_rate` / `topk_hit_rate`: how often the actual action taken was the prior's #1 pick
  (or within its top-k), among test observations where the prior had any candidate at all.
- `mean_reciprocal_rank`: the same idea averaged over rank (`1/rank`, `0` if the action wasn't
  among the candidates), a softer signal than a hard hit/miss cutoff.
- `coverage` vs. `fallback_rate`: these intentionally do **not** sum to 1. `coverage` is
  state-weighted (the fraction of *distinct* test states for which the prior returned any
  candidate); `fallback_rate` is observation-weighted (the fraction of *test observations* whose
  state had none). One rarely-seen state with no candidates barely moves `fallback_rate` but still
  costs a full point of `coverage` — the report also includes the raw counts each rate is computed
  from, so either framing can be double-checked directly.

`lineprior eval --help` lists the full set of `build`-equivalent tuning flags (`--min-count`,
`--smoothing-alpha`, `--confidence-mode`, etc.) — `eval` builds its train-side prior under the same
knobs a real `build` run would use, so the two stay comparable.

### Confidence calibration and threshold sweep

`--calibration-bins`/`--thresholds` turn `eval` into a selective-prediction tool: instead of just
"how good is the prior overall," they answer "if I only trust the prior above confidence X, how
much of my data can I still act on, and how accurate is it?"

```bash
lineprior eval observations.jsonl \
  --confidence-mode wilson-lower-bound \
  --calibration-bins 10 \
  --thresholds 0.3,0.5,0.7,0.9
```

- `confidence_calibration` (from `--calibration-bins N`): `N` equal-width bins over `[0, 1]`,
  always exactly `N` entries regardless of how many observations landed in each. Each bin reports
  `top1_hit_rate`/`mean_reciprocal_rank` among evaluated test observations whose #1 candidate's
  confidence fell in that bin — a well-calibrated confidence mode should show hit rate tracking bin
  confidence roughly 1:1.
- `threshold_sweep` (from `--thresholds`): one entry per requested threshold, always in the
  requested order. `covered_fraction` is the fraction of *all* test observations where the state
  had a candidate and its #1 confidence was `>= min_confidence`; `abstained_fraction = 1.0 -
  covered_fraction`. **These are a different weighting convention than the top-level
  `coverage`/`fallback_rate` above** — both are observation-weighted here and sum to 1 by
  construction, whereas the top-level pair deliberately doesn't. `top1_hit_rate`/
  `mean_reciprocal_rank` in each entry are computed among *covered* observations only (accuracy
  given a prediction was actually made), the same "conditioned on evaluated" convention the
  headline metrics already use.

Both are omitted (empty arrays) unless explicitly requested, so existing `eval` usage is unaffected.

## Academic positioning

`lineprior` is an engineering-oriented Rust implementation inspired by existing ideas in case-based planning, plan reuse, sequence prediction, variable-order Markov models, and policy-guided search. It is not a new theoretical algorithm.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

See [`AGENTS.md`](./AGENTS.md) for the full design spec and roadmap.
