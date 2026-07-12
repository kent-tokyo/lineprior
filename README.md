# lineprior

[![crates.io](https://img.shields.io/crates/v/lineprior.svg)](https://crates.io/crates/lineprior)
[![docs.rs](https://img.shields.io/docsrs/lineprior)](https://docs.rs/lineprior)
[![CI](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/lineprior/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/lineprior.svg)](https://github.com/kent-tokyo/lineprior/blob/main/LICENSE-MIT)

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

Useful flags: `--max-step` (drop observations past a given step), `--max-actions-per-state` (keep only the top N candidates), `--tags` (keep only observations carrying at least one of the given tags, comma-separated), `--confidence-k` (tune how fast confidence grows with sample size), `--confidence-mode` (`heuristic` (default), `wilson-lower-bound`, or `hybrid` — see "Confidence modes" below), `--confidence-z` (z-score for the Wilson lower bound, default `1.96`, ignored under `heuristic`), `--min-weighted-count` / `--min-confidence` (filter on the weighted count or confidence directly, instead of just the raw `--min-count`), `--draw-value` (success credit for a `draw` outcome — default `0.5`, since a draw is a genuine partial outcome in adversarial games, not a loss), `--time-decay-half-life-days` / `--time-decay-reference-unix-seconds` / `--missing-timestamp-policy` (age-based weight decay — see "Time decay and source reliability" below), `--source-weights` / `--default-source-weight` (per-source reliability multipliers, same section), `--config <path.json>` (load the whole `BuildConfig` from a file instead of individual flags, e.g. one saved by `lineprior tune --save-best-config` — see "Tuning" below; errors if combined with any flag above), `--strict` (fail on the first invalid record instead of skipping it with a warning).

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
(accepts `none`), `default-source-weight`. `--time-decay-reference-unix-seconds` is a single value
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

## Academic positioning

`lineprior` is an engineering-oriented Rust implementation inspired by existing ideas in case-based planning, plan reuse, sequence prediction, variable-order Markov models, and policy-guided search. It is not a new theoretical algorithm.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```
