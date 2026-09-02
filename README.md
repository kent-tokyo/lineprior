# lineprior

[![crates.io](https://img.shields.io/crates/v/lineprior.svg)](https://crates.io/crates/lineprior)
[![docs.rs](https://img.shields.io/docsrs/lineprior)](https://docs.rs/lineprior)
[![CI](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/lineprior.svg)](https://github.com/kent-tokyo/lineprior/blob/main/LICENSE-MIT)

[日本語](./README_ja.md) / English

`lineprior` is a Rust library and CLI for **explainable action ranking**: given historical `(state, action, outcome)` sequences, it builds a reproducible **action prior** for action selection — a ranked, confidence-scored list of candidate actions per state, learned offline from historical evidence, not online exploration. Given a state, it answers:

> What actions have historically worked well from here?

It's built for search, planning, agents, games, and optimization — move/candidate ordering, opening-book-like historical guidance, or ranking which expensive experiment to try first. It's confidence-aware and abstention-friendly: sparse or unseen states return no candidates rather than a guess (see "Confidence modes" below for the selective-prediction machinery behind that). It is **not** a shogi opening book library, a chess-specific book format, a planner, a solver, a game engine, or a contextual-bandit/reinforcement-learning/online-learning library — no policy learning, no exploration, no online updates. See "lineprior vs. contextual bandit" below if that's the gap you're checking.

## When to use lineprior

Use `lineprior` when you have historical `(state, action, outcome)` sequences and want an explainable, reproducible prior for ranking candidate actions before search, planning, simulation, or verification:

- **Search move ordering** — rank candidate moves/actions to explore first.
- **Planner candidate ordering** — prioritize which candidate step to expand first.
- **Agent action priors** — give an agent a starting ranking over its candidate actions.
- **Optimization branch ordering** — use past successful paths to order which branch to try first.
- **Opening-book-like historical guidance** — reuse patterns from past games/runs; `lineprior` itself has no game- or domain-specific knowledge built in.
- **Expensive experiment/gate candidate prioritization** — rank which candidate is worth an expensive run or verification first.

## What it is not

`lineprior` does not decide the best action by itself. It is a **prior, not an oracle**:

- It suggests candidate actions with a count, rate, and confidence attached.
- The caller is expected to combine this with search, evaluation, rules, or verification before acting.
- When data is sparse or a state is unseen, it returns no candidates rather than inventing one.

If historical data is biased, the prior will be biased too. `lineprior` can improve candidate ordering when historical sequences are relevant and representative — it does not guarantee better decisions.

## lineprior vs. contextual bandit

If you landed here searching for a contextual bandit, here's the actual difference:

- A **contextual bandit** (e.g. LinUCB, Thompson Sampling) learns a policy online: it explores, updates from feedback, and balances exploration vs. exploitation as it runs.
- `lineprior` builds a prior **offline**, once, from a static historical log. It implements no bandit or reinforcement-learning algorithm, does no online exploration, and does no online policy updates.
- `lineprior` is not a policy or a solver on its own — it's a ranking/confidence component meant to sit *before* a search, planner, bandit, or solver, the same way a chess opening book informs (without replacing) a search engine.

They're complementary, not competing: candidates ranked by `lineprior` could seed a bandit's initial arm set or a planner's move ordering, but `lineprior` itself never explores or learns online.

## Building a prior book

```bash
lineprior build observations.jsonl \
  --out prior.jsonl \
  --min-count 1 \
  --smoothing-alpha 5.0
```

Useful flags: `--max-step` (drop observations past a given step), `--max-actions-per-state` (keep only the top N candidates), `--tags` (keep only observations carrying at least one of the given tags, comma-separated), `--confidence-k` (tune how fast confidence grows with sample size), `--confidence-mode` (`heuristic` (default), `wilson-lower-bound`, or `hybrid` — see "Confidence modes" below), `--confidence-z` (z-score for the Wilson lower bound, default `1.96`, ignored under `heuristic`), `--min-weighted-count` / `--min-confidence` (filter on the weighted count or confidence directly, instead of just the raw `--min-count`), `--draw-value` (success credit for a `draw` outcome — default `0.5`, since a draw is a genuine partial outcome in adversarial games, not a loss), `--time-decay-half-life-days` / `--time-decay-reference-unix-seconds` / `--missing-timestamp-policy` (age-based weight decay — see "Time decay and source reliability" below), `--source-weights` / `--default-source-weight` (per-source reliability multipliers, same section), `--count-weight` / `--success-weight` / `--score-weight` (relative weight of each term in the raw prior score — `count_weight * ln(1 + weighted_count) + success_weight * success_rate + score_weight * mean_score` — all default `1.0`; also available as `tune --param` keys, see "Tuning" below), `--config <path.json>` (load the whole `BuildConfig` from a file instead of individual flags, e.g. one saved by `lineprior tune --save-best-config` — see "Tuning" below; errors if combined with any flag above), `--strict` (fail on the first invalid record instead of skipping it with a warning).

`--min-confidence`'s meaning depends on `--confidence-mode`: under `heuristic` it's a pure sample-size floor, blind to outcome. Under `wilson-lower-bound`/`hybrid` it's success-rate-aware, so a high-count but mostly-failing action that used to pass the filter can now be dropped by it — switching `--confidence-mode` on an existing `--min-confidence` threshold is a real behavior change, not just an additive one.

### Confidence modes

- `heuristic` (default): `weighted_count / (weighted_count + confidence_k)` — a sample-size heuristic, blind to outcome. Not a statistical guarantee, but works even for score-only datasets with no outcome labels at all.
- `wilson-lower-bound`: the Wilson score interval lower bound on the action's success rate — an actual statistical lower bound, useful once `outcome` labels are meaningful. Falls back to `heuristic` for an action with no decisive-outcome observations (nothing to bound).
- `hybrid`: `heuristic * wilson-lower-bound`, so both low sample size *and* a weak success rate pull confidence down. Same fallback as `wilson-lower-bound` when there's no outcome data.

Weighted/fractional observations (`--weight`, `draw` outcomes under `--draw-value`) feed the Wilson bound through an effective sample size (`sum(weight)^2 / sum(weight^2)`, Kish's formula) rather than the raw weighted count — an engineering approximation, exact for uniform weight `1.0` observations.

### Time decay and source reliability

Not every observation deserves equal trust. `build`/`eval` can compute an `effective_weight` per observation — `weight * time_decay_multiplier * source_reliability_multiplier` — feeding everything downstream (`prior`, `confidence`, eval calibration) automatically. Both factors default to a no-op, so this is entirely opt-in.

Stale data, decayed by age:

```bash
lineprior build observations.jsonl \
  --out prior.jsonl \
  --time-decay-half-life-days 30 \
  --time-decay-reference-unix-seconds 1783540000
```

`--time-decay-reference-unix-seconds` is **required** whenever `--time-decay-half-life-days` is set — there's no implicit "now," since that would make identical build/eval invocations produce different priors (and a different `build_config_fingerprint`) depending on when you happened to run them. An observation's `weight` decays as `0.5 ^ (age_days / half_life_days)`; a future-dated observation (`observed_at_unix_seconds` after the reference) clamps to age `0`, silently. `--missing-timestamp-policy` (`keep-base-weight`, the default, or `drop`) decides what happens to an observation with no `observed_at_unix_seconds` — inert when decay is disabled.

Multiple sources of differing reliability:

```bash
lineprior build observations.jsonl \
  --out prior.jsonl \
  --source-weights engine_v012=1.0,engine_v010=0.6,human=0.8 \
  --default-source-weight 1.0
```

An observation's `source` field looks itself up in `--source-weights`; an absent or unrecognized source falls back to `--default-source-weight` (default `1.0`, i.e. trust it same as any other). This is independent of time decay — you can use either, both, or neither.

**Caveat:** Kish's effective sample size (the same formula the Wilson bound above uses) is invariant to uniformly scaling every one of an action's own weights by the same factor. So when every observation behind an action shares the same age/source, pure `wilson-lower-bound` confidence does **not** reflect decay at all — only `weighted_count` (and therefore `prior`, and `heuristic`/`hybrid` confidence) does. Use `hybrid`, not bare `wilson-lower-bound`, if you want the `confidence` number itself to drop for stale or unreliable data.

You could always precompute `weight` yourself before feeding it to `lineprior` — this feature exists so the common case (decay by age, discount by source) is reproducible and folded into the config fingerprint, not as a replacement for custom weighting logic.

`build` also prints a one-line summary of what its filters actually did, e.g. `stats: 950/1000 observations kept, 42/50 candidates kept (5 by min_count, ...)` — useful for sanity-checking your own pre-filtering (e.g. a domain-specific ply/depth cutoff) against `--min-count`/etc. without re-deriving the numbers by hand. As a library, this is `BuildOutput.stats` (a `BuildStats`) returned alongside the book by `build_prior_book_from_reader`.

## Querying a prior book

```bash
lineprior query prior.jsonl --state state_a --top-k 5
```

An unseen state prints nothing and still exits `0` — that's the expected fallback behavior, not an error.

Add `--recent-actions action_x,action_y` for a context-aware query (see "Variable-order context" below) — output becomes `{"matched_order": N, "candidates": [...]}` instead of one line per candidate.

As a library, `PriorBook::candidates()` gives you every `(state, action)` candidate across the whole book as a flat `Vec<(String, PriorAction)>`, for callers filtering or sampling candidates directly (e.g. building a domain-specific "opening suite") instead of working through the nested per-state structure `entries_sorted()` returns.

## Other commands

```bash
lineprior summary prior.jsonl      # coverage, average confidence, per-state entropy
lineprior validate observations.jsonl   # parse and report issues without building
```

## Input schema

One JSON object per line:

```json
{"sequence_id":"case-001","step":0,"state":"state_a","action":"action_x","outcome":"success","score":0.8,"weight":1.0,"tags":["trusted"],"observed_at_unix_seconds":1783540000,"source":"engine_v012"}
```

Required: `sequence_id`, `step`, `state`, `action`.
Optional, with defaults: `outcome` (`unknown`), `score` (`null`), `weight` (`1.0`), `tags` (`[]`), `observed_at_unix_seconds` (`null`, only consulted when time decay is enabled — see "Time decay and source reliability" above), `source` (`null`, only consulted via `--source-weights`).

## Output schema

One JSON object per state, actions ranked by descending prior:

```json
{"state":"state_a","actions":[{"action":"action_x","count":3,"weighted_count":3.0,"success_rate":0.667,"mean_score":0.633,"prior":0.557,"confidence":0.130}]}
```

`success_rate` and `mean_score` are the raw, unsmoothed observed rates (for transparency); `prior` is the smoothed, normalized ranking score; `confidence` is a heuristic sample-size indicator by default, or a real Wilson-bound statistical lower bound under `--confidence-mode wilson-lower-bound`/`hybrid` (see "Confidence modes" above). `success_rate` credits a `success` outcome as 1.0, a `draw` as `--draw-value` (default 0.5), and a `failure` as 0.0.

`lineprior build`'s CLI output (and the library's `save_prior_book_with_config`) prepends a header line carrying a fingerprint of the `BuildConfig` used to build it, e.g. `{"build_config_fingerprint":7592859384087124328}`. `load_prior_book`/`lineprior query`/`lineprior summary` all skip this line transparently — it doesn't change how you read a prior book day to day.

With `--context-order` > 0, some lines additionally carry a `context` field — see "Variable-order context" below.

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

Upgrading to a lineprior version that adds new `BuildConfig` fields (like `confidence_mode`/`confidence_z`, `time_decay_half_life_days`/`source_weights`, or `context_order`) changes the fingerprint for *every* config, even when the new fields are at their inert defaults (`heuristic` mode, decay disabled, no source weights) — so a prior book cached before upgrading will trip `BuildConfigMismatch` once after upgrading. That's the fingerprint mechanism working as intended, not a regression.

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

The non-game fixture [`examples/ui_automation.jsonl`](./examples/ui_automation.jsonl) maps screen
states to UI actions such as `click:add-to-cart`. The same CLI round-trip is available in
[`examples/python/roundtrip.py`](./examples/python/roundtrip.py) and
[`examples/node/roundtrip.mjs`](./examples/node/roundtrip.mjs); both assert that repeated builds
are byte-deterministic and that querying the built book returns the expected action. These are
CLI integration examples, not language bindings: the Rust CLI remains the authoritative
implementation until a maintained Python or WASM package is justified.

## WASM / JavaScript boundary

The `lineprior-wasm` workspace crate exposes two thin `wasm-bindgen` functions: `build_json` takes
JSONL observations plus a serialized `BuildConfig` and returns JSON containing sorted entries,
warnings, and build stats; `query_json` takes a JSONL prior book and returns ranked candidates.
They keep Rust scoring authoritative and return JavaScript errors for invalid input. The crate has
no file I/O or domain-specific state representation. npm/wasm-pack packaging and browser smoke
testing are not claimed yet.

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
- `success_weighted_top1_hit_rate` / `success_weighted_mean_reciprocal_rank`: the same two metrics,
  but each test observation is weighted by its outcome credit (a win counts fully, a draw counts
  for `--draw-value`, a loss or unrecorded outcome counts for nothing and drops out of the average
  entirely) instead of counted equally. `top1_hit_rate` can be inflated by matching actions that
  went on to fail — this restricts "did the prior agree with what was actually taken" to trials
  that actually worked. `None` when nothing in the test set earned positive credit.
- `failure_agreement_top1_hit_rate`: the counterweight — `top1_hit_rate` restricted to test
  observations whose outcome was exactly `failure`. A high value here is a warning sign: the
  prior's top pick agrees with actions that are known to have failed.
  **Caveat:** all three of these credit/blame each observation by *its own* `outcome` field, not
  by a sequence's eventual result — if your data records a terminal outcome by copying it onto
  every step, an early good move in an eventually-lost sequence is scored as a failure too. This
  is a property of how `outcome` was recorded, not something these metrics can correct for. `None` when the test set has
  no `failure` observations.
- `coverage` vs. `fallback_rate`: these intentionally do **not** sum to 1. `coverage` is
  state-weighted (the fraction of *distinct* test states for which the prior returned any
  candidate); `fallback_rate` is observation-weighted (the fraction of *test observations* whose
  state had none). One rarely-seen state with no candidates barely moves `fallback_rate` but still
  costs a full point of `coverage` — the report also includes the raw counts each rate is computed
  from, so either framing can be double-checked directly.

`lineprior eval --help` lists the full set of `build`-equivalent tuning flags (`--min-count`,
`--smoothing-alpha`, `--confidence-mode`, `--time-decay-half-life-days`, `--source-weights`, etc.) —
`eval` builds its train-side prior under the same knobs a real `build` run would use, so the two
stay comparable.

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

## Off-policy evaluation (explicitly opt-in)

The library also exposes `evaluate_self_normalized_ips` in a separate evaluation module. The
caller must provide the logging policy propensity and the evaluated policy's probability for the
action that was actually logged; `lineprior` never infers a counterfactual reward from a prior.
The report includes ordinary IPS, self-normalized IPS, support fraction, overlap failures, and
Kish effective sample size. Zero-support rows and importance weights above an optional cap are
reported as overlap failures rather than silently treated as losses.

`evaluate_doubly_robust` is also available when the caller supplies both a reward-model estimate
for the evaluated policy's expected reward and a reward-model estimate for the logged action. It
adds the propensity-weighted residual correction to the model baseline; rows without overlap use
the baseline only and remain visible in the support diagnostics.

`bootstrap_self_normalized_ips` adds deterministic percentile intervals for IPS and self-normalized
IPS. Its seed, resample count, and confidence level are explicit; resamples with no supported
rows are counted as skipped. This supports replayable uncertainty checks but does not replace a
held-out, real-data evaluation.

The same diagnostics are available from the CLI with `lineprior offpolicy log.jsonl --out report.json`.
Add `--doubly-robust` when every row contains the two reward-model fields, and
`--bootstrap-resamples N --bootstrap-seed S` for deterministic intervals. The JSONL input is one
`OffPolicyObservation` per line; malformed rows or invalid propensities exit with code 3.

The checked-in [`examples/offpolicy.jsonl`](./examples/offpolicy.jsonl) is a small valid input
boundary fixture. For example:

```bash
lineprior offpolicy examples/offpolicy.jsonl --out /tmp/lineprior-offpolicy.json \
  --doubly-robust --bootstrap-resamples 128 --bootstrap-seed 42
```

These are estimators and audit surfaces, not proof of causal improvement. Valid propensities,
overlap, uncertainty intervals, and a held-out downstream comparison remain the caller's
responsibility. A reward model is supplied by the caller and is never trained or inferred by
`lineprior`.

## Variable-order context

By default the prior is order-0: `state -> action`, with no memory of what happened earlier in a
sequence. `--context-order k` additionally learns `(recent-k-actions, state) -> action` for order
`1..=k`, derived automatically from each sequence's own `sequence_id`/`step` history — no schema
change, no new observation field. `0` (the default) disables this entirely; every existing book,
config, and query behaves exactly as before.

```bash
lineprior build observations.jsonl --out prior.jsonl --context-order 2
lineprior query prior.jsonl --state state_a --recent-actions action_x,action_y
lineprior eval observations.jsonl --context-order 2
```

**Backoff and transparency.** A context-aware query tries the longest available context first,
then "stupid backoff" — no interpolation smoothing — to shorter context, down to the plain
order-0 lookup as the final rung. `lineprior query --recent-actions` prints
`{"matched_order": N, "candidates": [...]}`; `N` is which depth actually answered the query (`0`
meaning plain state-only), the same "how much evidence backs this" transparency `confidence`
already gives per action. Without `--recent-actions`, `query` is byte-for-byte unchanged.

**Sortedness precondition.** Deriving a sequence's own recent-action window while streaming
requires that sequence's rows be contiguous in the input, with strictly increasing `step` — only
enforced when `--context-order` is nonzero. A violation is a hard error (`SequenceNotSorted`,
exit code 3) **independent of `--strict`**: it's a structural precondition on the whole stream,
not a per-record validity question `--strict`/non-strict already governs. If your data isn't
already grouped this way, sort it first (`jq -s 'sort_by(.sequence_id, .step)[]'` or similar).

**Output schema.** A context entry adds a `context` field (the recent-action window, oldest
first) to the usual `{"state": ..., "actions": [...]}` line: `{"state":"state_a",
"context":["action_x"],"actions":[...]}`. Order-0 entries never carry this field, so a book built
with `--context-order 0` (the default) serializes identically to before this feature existed.

**Memory.** Peak memory grows from "bounded by unique `(state, action)` pairs" to "bounded by
unique `(state, action)` pairs at order 0, *plus* unique `(context, state, action)` tuples across
every order `1..=k`" — an inherent cost of the feature (more precision needs more storage), not a
regression. `crates/lineprior/tests/streaming_memory.rs` has a regression test for this shape too.

**Prefix support diagnostics.** `lineprior summary prior.jsonl` reports the number of
context-conditioned entries and, for each context order, the number of distinct action prefixes,
`(prefix, state)` entries, action entries, and stored raw observation counts. These are static
support diagnostics for spotting sparse/deep contexts; they are not confidence guarantees or
evidence that a context improves downstream decisions. The same values are available from the
library's `SummaryReport::context_orders`.

**Evaluating whether context actually helps.** `lineprior eval --context-order k` reports two new
top-level fields alongside the usual order-0 ones, computed over the *same* test observations in
the same run: `context_top1_hit_rate` / `context_mean_reciprocal_rank` (the context-aware
counterparts of `top1_hit_rate`/`mean_reciprocal_rank`, which themselves stay order-0). The
difference is the lift (or cost) context provides — a single-run, apples-to-apples comparison
rather than two separate runs whose headline field would otherwise silently mean different
things. `hit_rate_by_matched_order` breaks accuracy down *by* the depth backoff actually reached
(not just how often each depth was reached), answering "is deeper context more accurate when
available, or just rarer." All three are empty/`None` at `--context-order 0`. `lineprior tune`
surfaces the same two fields per candidate in `all_results`, so `--param
context-order=0,1,2,3` sweeps show the lift directly — no new `--objective` needed, since the
existing objectives already read the order-0 fields those sweeps vary.

**Credit-assignment caveat, same shape as the outcome-weighted eval metrics above:** context is
derived purely from *step order* — it has no opinion on whether deeper context is causally
meaningful for your domain, only on whether it's statistically predictive on your held-out data.
Always check `context_top1_hit_rate` against the plain `top1_hit_rate` baseline before trusting a
context-aware prior; a domain where `state` already encodes recent history (e.g. a full board
position) may see little or no lift, and that's a legitimate, informative result — not a bug.

## Opt-in similarity fallback

`PriorBook::query_with_similarity` accepts neighbors supplied by the caller, each with a state,
non-negative distance, and opaque provenance label. It applies deterministic exponential distance
weighting and returns only actions observed in those neighbor states; unknown actions are never
invented. `SimilarityConfig` can cap neighbors or distance, and each result retains its evidence
so callers can audit which states supported an action.

This is an integration boundary, not an embedding or vector-database implementation. Exact-match
querying and abstention remain the default. The returned confidence is a weighted summary of the
source confidences, not a new statistical guarantee, so callers should validate similarity
fallback against an unseen-state split before enabling it in a decision loop.

The deterministic boundary fixture at
`crates/lineprior-similarity/tests/fixtures/unseen_states.jsonl` and its integration test compare
exact-match, no-prior abstention, and opt-in similarity recovery. It is a contract check only; it
does not establish real-data quality or justify enabling similarity by default.

## Sequence-level priors

`PriorBook::score_sequence(path: &[(String, String)]) -> SequencePriorScore` scores a *caller-
supplied* candidate multi-step plan — how much historical precedent backs each step, and the plan
as a whole — by walking [context-aware backoff](#variable-order-context) at each step:

```rust
let path = vec![
    ("state_a".to_string(), "action_x".to_string()),
    ("state_b".to_string(), "action_y".to_string()),
];
let score = book.score_sequence(&path);
// score.steps[i]: { state, action, matched_order, found, prior, confidence }
// score.min_confidence: the weakest-linked step's confidence, or None if none matched
// score.unseen_steps: how many steps had no historical precedent at all
```

Each step's context is the *plan's own* prior steps' actions (oldest first, mirroring how
`--context-order` derives context while building) — not something the caller passes separately.
`lineprior` has no model of environment dynamics: given `(state, action)` it doesn't know what
state results, so the caller (who owns that mapping — their own planner or simulator) must supply
both state and action at every step.

**Aggregation is `min`, not an average.** A chain is only as strong as its weakest link;
averaging would let one very-weakly-supported step hide behind stronger ones, which cuts against
"prior, not oracle" transparency. `min_confidence` is `None` (not `0.0`) when every step is
unseen — the same "absent data isn't a bad score" rule used elsewhere. Check `steps` directly,
not just the aggregate, when `unseen_steps > 0`.

**Backoff-shadowing caveat.** Each step reuses `query_with_context` verbatim: whichever context
depth resolves is the *only* depth searched for the caller's action. A sparse deep-context match
on *other* actions can shadow abundant order-0 support for the action actually asked about,
reading as `found: false` even though the action is well-supported at a shallower depth. This is
the safe direction (under-reporting support, never over-reporting) and matches what
`query_with_context` itself would have suggested to a caller asking "what should I do here" — not
a bug, but worth knowing before treating `found: false` as "truly never seen."

**Deliberately library-only.** No CLI subcommand and no `eval`/`tune` integration in this round —
a `(state, action)` path doesn't fit a comma-separated CLI flag, and scoring held-out sequences
against their outcome would require inventing a "sequence's terminal outcome" concept the core
model deliberately doesn't have an opinion on (see the [credit-assignment
caveat](#evaluating-a-prior) above). Both are natural upgrade paths if real demand shows up.

## Tuning: choosing a BuildConfig automatically

`eval` scores one config at a time; `tune` grid-searches many and picks the best one, using the
*same* deterministic train/test split for every candidate so they're directly comparable:

```bash
lineprior tune observations.jsonl \
  --split-by sequence --train-ratio 0.8 \
  --param confidence-mode=heuristic,wilson-lower-bound,hybrid \
  --param min-confidence=0.0,0.3,0.5,0.7 \
  --param smoothing-alpha=1.0,5.0,10.0 \
  --param time-decay-half-life-days=none,30,90 \
  --time-decay-reference-unix-seconds 1783540000 \
  --objective covered-mrr --min-covered-fraction 0.4 \
  --out tune.json --save-best-config best_config.json
```

Each `--param key=v1,v2,...` sweeps one `BuildConfig` field (repeat `--param` for more than one);
any field never named in a `--param` stays at its `BuildConfig::default()` for every candidate.
Supported keys: `confidence-mode`, `min-confidence`, `smoothing-alpha`, `confidence-k`,
`confidence-z`, `min-count`, `min-weighted-count`, `draw-value`, `time-decay-half-life-days`
(accepts `none`), `default-source-weight`, `count-weight`, `success-weight`, `score-weight`.
`--time-decay-reference-unix-seconds` is a single value
applied to every candidate (never swept) — required whenever a swept `time-decay-half-life-days`
value isn't `none`, same reproducibility rule `build`/`eval` already use.

`--objective` (default `covered-mrr`) is what candidates are ranked by:

| objective | meaning |
|---|---|
| `mrr` | `mean_reciprocal_rank`, among covered test observations only |
| `top1` | `top1_hit_rate`, among covered test observations only |
| `covered-mrr` (default) | `covered_fraction * mean_reciprocal_rank` — MRR averaged across *all* test observations, an uncovered one contributing `0` |
| `top1-at-min-coverage` | same as `top1`, but requires `--min-covered-fraction` also be set |
| `success-weighted-mrr` | `success_weighted_mean_reciprocal_rank` — like `mrr`, but a failed or unrecorded-outcome test observation contributes nothing |
| `success-weighted-top1` | `success_weighted_top1_hit_rate`, the same idea applied to `top1` |

`covered-mrr` is the default because optimizing `mrr` alone tends to pick configs that abstain
(report no candidate) except when very confident, while optimizing coverage alone tolerates a
sloppy prior — `covered-mrr` penalizes both.

`--min-covered-fraction` / `--max-fallback-rate` / `--min-top1-hit-rate` reject a candidate from
being `best`, but it still shows up in the JSON report's `all_results` (with `meets_constraints:
false`) so you can see what got excluded and why, rather than it silently vanishing.

The JSON report's `pareto_front` is the non-dominated set over `(mrr, covered_fraction)` — every
config on it is the best *some* MRR/coverage tradeoff, independent of `--objective`, in case you'd
rather eyeball the tradeoff yourself than trust the single `best` pick.

`--save-best-config best_config.json` writes the winning candidate's `BuildConfig` as JSON; `build`
and `eval` both accept it back via `--config best_config.json` (errors if combined with any
individual build-config flag like `--min-count`, since it's a whole-config replacement, not an
overlay) — so a config chosen once by `tune` is reused exactly, not re-typed by hand:

```bash
lineprior build observations.jsonl --out prior.jsonl --config best_config.json
```

`tune` is exactly as domain-agnostic as the rest of `lineprior` (it only ever sees `state`/
`action`/`sequence_id`/outcome data) and doesn't change what `lineprior` fundamentally is — a
**prior, not an oracle**. It automates what you'd otherwise do by hand-sweeping `eval`; it doesn't
make the resulting prior any less something the caller should verify before acting on.

## Gate outcome prediction (library only)

A different question from the rest of this crate: not "what action should I take," but "is this
training candidate worth an expensive real evaluation (a 'gate' run of many games) at all?"
`GateModel::fit`/`GateModel::predict` (in `gate.rs`) fit a small, regularized surrogate that
predicts a candidate's real-gate Elo delta -- and how much to trust that prediction -- from cheap
validation-time diagnostics, so gate runs can be reserved for candidates likely to be worth them.

```rust
let output = GateModel::fit(&observations, &GateModelConfig::default())?;
// output.report: selected_lambda, weighted_rmse, and a probability_positive calibration report --
// check this before trusting predictions from output.model.

let prediction = output.model.predict(&GateQuery { features });
// prediction.expected_elo, .interval_low/.interval_high, .probability_positive,
// .leverage, .support_distance, .nearest_group_distance, .missing_feature_fraction,
// .prediction_status, .recommend_for_gate
```

`predict_verdict` maps the Gaussian latent-Elo posterior into PASS, FAIL, and INCONCLUSIVE regions
using explicit `GateVerdictConfig` thresholds. `acquire` computes standard expected improvement over
an incumbent `baseline_elo`, divided by a caller-supplied expected gate cost; it does not multiply EI
by `probability_positive` a second time. Both surfaces preserve the model's OOD recommendation flag.

```bash
lineprior gate gate_history.jsonl --feature valid_cp_mse_delta=0.12 \
  --feature output_std=0.03 --monotonic valid_cp_mse_delta=increasing \
  --expected-gate-cost 100 --out gate-report.json
```

The `gate` CLI fits the experimental model from strict JSONL `GateObservation` rows and optionally
emits prediction, three-way verdict probabilities, acquisition score, and fit diagnostics. The
GateModel remains in the main crate: the CLI currently has no independent schema or dependencies,
so extracting `lineprior-gate` would add packaging surface without a demonstrated consumer boundary.

- **Monotonic constraints are opt-in.** `GateModelConfig::monotonic_constraints` projects named
  coefficients to the requested increasing/decreasing sign orthant. The constrained fit is a
  conservative shape constraint, and its closed-form ridge uncertainty is only an approximation;
  validate it against real gate history before using it for scheduling.

- **Named features, not a fixed schema.** `GateObservation.features`/`GateQuery.features` are a
  caller-named `BTreeMap<String, f64>` (e.g. `valid_cp_mse_delta`, `output_std`, `conflict_rate`),
  so the diagnostic set can evolve without a schema break. Deliberately excludes anything like a
  training seed -- a categorical id, not a quantity a linear model can treat as "more" or "less."
- **Group-aware, not a random split.** `GateObservation.group_id` is an opaque caller-composed key
  (e.g. an experiment family/recipe/lineage/dataset version joined together) used for k-fold
  cross-validation when selecting the ridge regularization strength -- never parsed by this crate.
  Falls back to leave-one-group-out when fewer than the requested fold count has distinct groups.
- **Uncertainty is latent-strength confidence, not next-gate-run noise.** `interval_low`/
  `interval_high` -- on both `GatePrediction` and `GateOofPrediction` below -- describe how much to
  trust the point estimate as a read on the candidate's *true* strength (a closed-form
  Bayesian-ridge posterior variance), not the added sampling noise of one hypothetical future gate
  match. This holds everywhere in the module; it is not a per-call opt-in. A missing feature at
  query time is imputed as its training-set mean and reported via `missing_features`, never invented
  silently.
- **Validating Round A itself, before building anything on top of it.** `GateModel::fit_with_validation`
  returns everything `fit` does, plus a per-candidate out-of-fold audit table:

  ```rust
  let validated = GateModel::fit_with_validation(&observations, &GateModelConfig::default())?;
  // validated.interval_level: the two-sided confidence level interval_low/interval_high represent
  // (e.g. ~0.95 at the default interval_z), stated once here rather than repeated per row.
  for row in &validated.oof_predictions {
      // row.candidate_id, .group_id, .actual_elo, .predicted_elo, .residual, .prediction_stddev,
      // .interval_low/.interval_high, .probability_positive, .outer_fold, .inner_selected_lambda,
      // .leverage, .support_distance, .nearest_group_distance, .missing_feature_fraction,
      // .prediction_status, .recommend_for_gate
  }
  ```

  Every row comes from the *same* nested group cross-validation `report.weighted_rmse`/
  `report.calibration` are built from -- not a second CV run solely to populate the table, so the
  aggregate metrics and the per-row audit can never describe a different population of predictions.
  Rows are sorted deterministically by `(outer_fold, group_id, candidate_id)`, and a repeated
  `candidate_id` in the input is preserved as separate rows rather than collapsed. `fit` itself is
  a thin wrapper over `fit_with_validation` that discards the table (still computed either way --
  this only spares a caller who only wants the model from receiving/reading it) -- both share one
  fitting path, so the two entry points can never disagree about the model or its aggregate metrics.
- **Elo observation uncertainty: not every label is equally trustworthy.** A `GateObservation` may
  carry `actual_elo_stddev` (or `elo_ci_low`/`elo_ci_high`, from which a stddev is implied assuming
  a symmetric normal interval at `GateModelConfig::observation_ci_z` -- a *separate* knob from
  `interval_z`, since the caller's CI was computed at whatever confidence level they used,
  independent of how wide this model's own output intervals should be) alongside `gate_elo_delta`
  -- a 20-pair burn-in Elo and a 1700-pair formal-gate Elo are not equally trustworthy teacher
  labels. When present, this becomes the ridge fit's reliability weight (`1 / stddev^2`,
  inverse-variance) in place of the `gate_games_played`-based weight, per row -- mixed datasets
  (some rows with a stated stddev, some without) combine correctly. `completed_pairs` and
  `gate_status` (this candidate's actual historical
  PASS/FAIL/INCONCLUSIVE verdict) are also accepted, audit-only for now -- never fed into `features`
  or the fit, same reasoning as the existing `training_seed` exclusion. A `provenance:
  BTreeMap<String, String>` field carries opaque caller-composed provenance (experiment/dataset/
  teacher-manifest ids, seeds, schema version, ...), same "never parsed by this crate" convention as
  `group_id`.
- **Exactly one uncertainty source per observation, never a silent priority.** Specifying both
  `actual_elo_stddev` and a complete `elo_ci_low`/`elo_ci_high` pair on the same observation is
  rejected (`Error::ConflictingGateUncertaintySources`), as is providing only one bound of a CI
  (`Error::IncompleteGateConfidenceInterval`) or a `gate_elo_delta` outside its own stated
  `[elo_ci_low, elo_ci_high]` (`Error::GateEloOutsideConfidenceInterval`). An observation with
  neither falls back to the `gate_games_played`-based weight, unchanged.
- **Extreme per-row weights can't dominate the fit.** After normalizing to mean `1.0`, each row's
  reliability weight is clamped to `[1 / max_weight_ratio, max_weight_ratio]`
  (`GateModelConfig::max_weight_ratio`, default `100.0`, must be `>= 1.0`) -- otherwise one
  observation with a tiny stated `actual_elo_stddev` (a data-entry slip, or a genuinely
  near-noiseless measurement) could produce an inverse-variance weight thousands of times any other
  row's and effectively dictate the fit alone. This clamp is deliberately not followed by a second
  renormalization, since re-normalizing clamped weights back to mean `1.0` could push a
  just-clamped weight back outside the bound it was promised; the trade-off is that the weight mean
  can drift (only when the clamp actually engages) instead. `GateFitReport` surfaces
  `min_observation_weight`/`max_observation_weight`/`effective_sample_size`/
  `clamped_observation_count` so this isn't a silent safety net.
- **`GateFitReport.dispersion_factor`: a calibration check on the stated stddevs themselves.**
  `Some` only when every observation in the fit supplied a usable stddev -- an out-of-fold reduced
  chi-square (`sum((actual_elo - predicted_elo)^2 / stddev^2) / n`, from the same nested-CV
  predictions `weighted_rmse`/`calibration` are built from). Roughly `1.0` means the stated stddevs
  are well-calibrated against how far predictions actually land from real outcomes; `>> 1` means
  real noise exceeds what's being reported *or* the linear model is missing structure (the two
  aren't separable by this statistic alone); `<< 1` means stated stddevs are overstated.
- **Out-of-distribution abstention: reported, not enforced.** In `GateOofPrediction`, every OOD
  quantity below is computed from a support model (standardizer, group centroids, mean leverage)
  fit on *only that outer fold's training rows* -- the same rows the fold's coefficients came from,
  never the held-out row itself or any other fold's rows. The final deployed `GateModel` (after all
  CV) fits its own support model on the complete training set, same as its coefficients. Every
  `GatePrediction`/`GateOofPrediction` also carries `leverage` (the ridge-analogue hat/leverage term, growing
  without bound the further a query sits from the training feature mean), `support_distance`
  (`leverage.sqrt()`), `nearest_group_distance` (standardized-space distance to the nearest
  training group's centroid), `missing_feature_fraction`, and `prediction_status`
  (`Supported`/`Extrapolation`/`Unsupported`). `expected_elo`/`probability_positive` are still
  computed and returned regardless of status -- same "report, don't refuse" convention as
  `missing_features` -- a caller decides for itself whether to act on `Extrapolation`/`Unsupported`.
  Classification checks `missing_feature_fraction` first and unconditionally
  (`GateModelConfig::ood_missing_fraction_threshold`, default `0.5`): an all-missing query imputes
  to the training feature mean, the *lowest* possible leverage, which would otherwise look like
  maximum support while carrying zero real information. Otherwise `Extrapolation` fires when
  `leverage` exceeds `ood_leverage_ratio_threshold` (default `3.0`) times the model's own mean
  leverage (`df / n_eff`, the ridge-correct analogue of the OLS hat matrix's uniform `p/n` -- not
  the classical `2p/n`/`3p/n` rule of thumb, which assumes uniform OLS leverage and doesn't hold
  once `lambda > 0`), or when `nearest_group_distance` exceeds the largest nearest-neighbor
  distance among the training groups' own centroids (a self-calibrating reference scale, no
  arbitrary constant).
- **`recommend_for_gate`: the yes/no gating decision, without faking the estimate.** Exactly
  `prediction_status == Supported`, nothing else -- a caller that only wants a boolean reads this
  instead of matching on `prediction_status` itself. `expected_elo`/`interval_low`/`interval_high`/
  `probability_positive` are always the model's real prediction, even when `recommend_for_gate` is
  `false`: an out-of-distribution query is flagged, never silently zeroed or replaced.
- **This remains an experimental diagnostic surface.** Verdict probabilities, acquisition, and
  monotonic constraints are implemented, but real gate-history calibration and downstream validation
  are still required before scheduling expensive runs. The CLI is intentionally thin and the model
  remains in the main crate until an independent schema/dependency boundary appears.

## Academic positioning

`lineprior` is an engineering-oriented Rust implementation inspired by existing ideas in case-based planning, plan reuse, sequence prediction, variable-order Markov models, and policy-guided search. It is not a new theoretical algorithm.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo deny check licenses
sh scripts/check_candidate_contract.sh
sh scripts/run_examples_smoke.sh
sh scripts/run_wasm_build_smoke.sh
```

Dependency licenses are checked against the narrow SPDX allowlist in [`deny.toml`](./deny.toml);
adding a new license requires an explicit policy review.

The candidate contract script checks the fixed workspace version, JSON fixtures, language-example
syntax, formatting, and whitespace. It does not replace runtime tests, WASM packaging, or real-data
measurement gates.

After building `lineprior-cli`, `sh scripts/run_examples_smoke.sh` runs the maintained Node.js and
Python round-trip examples against the same binary. CI runs this smoke workflow; it checks the
Rust-CLI integration boundary, not WASM packaging or real-data quality.

The same built binary can run `sh scripts/run_offpolicy_smoke.sh` to evaluate the checked-in OPE
fixture twice and compare the complete JSON report, including IPS, DR, and the bootstrap seed. This
is a replayability check, not evidence of causal improvement.

With the `wasm32-unknown-unknown` target installed, `sh scripts/run_wasm_build_smoke.sh` verifies
that the `lineprior-wasm` crate compiles with the locked dependency graph. This is a compilation
boundary only; npm/wasm-pack packaging and browser execution remain separate gates.

See [`CHANGELOG.md`](./CHANGELOG.md) for release history, including which versions are published to
crates.io and, from 0.9.0 on, notes on Rust source compatibility (distinct from JSON/serde input
compatibility) for public API changes.
## Pluggable scoring strategies

`BuildConfig::scoring_strategy` supports `WeightedSum` (the backward-compatible
default), `Bayesian`, `Ucb`, and `Softmax`. The CLI exposes these through
`--scoring-strategy` and strategy parameters. These are ranking strategies, not
quality guarantees.

## Compact binary books

JSONL remains the interchange format. Deterministic LPB v1 is available for
local caching via `save_prior_book_binary` / `load_prior_book_binary`, or the
CLI's `lineprior pack` and `lineprior unpack`. It preserves context entries,
has a magic/version header and allocation caps, and rejects trailing bytes.

## veridict on/off recipe

See [`examples/veridict_prior_comparison.md`](examples/veridict_prior_comparison.md)
and its manifest. The checked-in recipe is protocol-only until a real
`veridict` run supplies downstream evidence.
## Macro-actions and multi-source merge

`build_macro_actions` extracts bounded contiguous action windows from ordered
histories. It is intentionally eager because a sequence window must be held in
memory; the normal streaming builder is unchanged. Independently-built books
can be combined with `merge_prior_books` and explicit `PriorBookSource` weights,
including context entries.

The official typed domain boundaries are in
[`lineprior-adapters`](crates/lineprior-adapters) and cover Sekirei, UI
automation, LLM-agent tool traces, and retrosynthesis. They keep domain values
opaque and do not claim legality, execution success, or chemical validity.
## Terminal credit and Trie representation

Set `BuildConfig::terminal_credit_weight` (or
`--terminal-credit-weight`) to propagate the final known outcome of each
sequence back to its kept steps. The default `0.0` preserves per-step labels;
the opt-in mode buffers only the current sequence and requires grouped input.

`PriorBook::to_trie()` materializes context entries into a deterministic
`PriorTrie` for repeated longest-suffix queries. The flat book remains the
canonical serialization format, and trie performance is still a measurement
item rather than a quality claim.

## Limits and evidence gates

`confidence` is a reliability signal, not a promise of future performance. The Bayesian, UCB,
and Softmax strategies change ranking behavior only; none guarantees a better policy, calibration,
or downstream result. Run a held-out comparison before enabling one by default.

States and actions are opaque keys in the core. Exact matching is therefore the safe default;
similarity recovery is caller-supplied and opt-in, and does not create unseen actions. The core does
not perform causal inference, generate counterfactual actions, or infer rewards for actions that were
not logged. IPS/DR are audit estimators whose validity depends on propensities, overlap, uncertainty,
and a held-out downstream test.

Python, npm, and WASM support currently consists of maintained CLI examples and a Rust/WASM boundary.
The browser/package gate is separate and must pass the workflow in `.github/workflows/wasm-browser.yml`
before being described as a supported distribution. Trie and macro-action implementations have
deterministic Criterion benchmarks, but their latency/memory and downstream benefit remain empirical
questions; no improvement claim is made from the benchmark alone.

Run these measurements with `cargo bench -p lineprior --bench scoring`. Record the machine,
toolchain, sample sizes, and Criterion output; numbers from different environments are not a
downstream experiment.

The three new workspace crates require a one-time manual crates.io bootstrap because Trusted
Publishing cannot create a crate. See [`docs/publishing.md`](docs/publishing.md); after that first
publish, the existing OIDC workflow can publish subsequent versions without a long-lived token.
