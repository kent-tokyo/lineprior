# IPS / DR real-log measurement protocol

Use this protocol to evaluate IPS/DR on real logs. `lineprior` estimates what the supplied
logged-policy assumptions imply; it cannot repair missing propensities or create counterfactual
outcomes.
The machine-readable contract is
[`offpolicy-integrated-arms-v1.schema.json`](offpolicy-integrated-arms-v1.schema.json). The
repository validator additionally checks paired/top-level lineage equality and finite metric
semantics.

Before estimation, validate raw rows against
[`offpolicy-observation-v1.schema.json`](offpolicy-observation-v1.schema.json) and run
`lineprior validate heldout.jsonl --kind offpolicy`. This checks finite rewards, non-empty opaque
keys, logging propensity in `(0, 1]`, and evaluation probability in `[0, 1]`; it does not prove
that the logged propensity was generated correctly.

## Required log fields

Every held-out row must include an immutable case/sequence ID, state, logged
action, reward, logging-policy propensity for that action, and evaluation
policy probability for that same action. For DR, also provide both explicit
reward-model values: evaluation-policy value and logged-action value. Record
policy commit/config, lineprior config/version, dataset/split ID, and schema
version.
The paired runner records `dataset_id`, `split`, and `lineprior_version` in
its output, plus `policy_version` when supplied; pass them explicitly rather
than relying on defaults. Before archiving, use
`validate_measurement_artifact.py offpolicy --require-explicit-lineage` to
reject placeholder lineage fields.

## Preflight and paired arms

Before interpreting an estimate, report positive finite propensities, the
fraction of rows with evaluation-policy support, overlap failures, effective
sample size, importance-weight cap (if used), and the largest observed
importance weight. Stop interpretation when propensities are absent, invalid,
or support is materially missing.

Run `off` and `on` against the same held-out rows, seed, candidate budget, and
downstream verifier. The `on` arm may only reorder/filter candidates through
the prior; it must log abstentions, evaluations, wall time, and the final
observed reward/outcome. Use `lineprior offpolicy` for IPS, self-normalized
IPS, DR, and seeded percentile bootstrap intervals:

```bash
lineprior offpolicy heldout.jsonl --out report.json \
  --doubly-robust --bootstrap-resamples 2000 --bootstrap-seed 42
```

For a single paired artifact that runs the Rust CLI for both arms and adds
the observed-reward pairing audit, use `scripts/measure_offpolicy_arms.py`.

The causal/downstream gate requires valid overlap, uncertainty intervals,
and a paired held-out improvement of `on` over `off` at the declared cost and
abstention budget. A point estimate, a replayable fixture, or a ranking
correlation alone does not pass this gate.

## Sekirei readiness

The checked-in [2026-10-03 audit](offpolicy-sekirei-readiness-2026-10-03.json) inspected 183 real
search decisions from six Floodgate analysis logs. Every decision had a state and selected action,
but none had a declared reward, logging propensity, evaluation-policy probability, or reward-model
values. Support, overlap, ESS, weight caps, bootstrap intervals, and IPS/DR are therefore undefined.

Do not reconstruct propensities from search scores, PV rank, or a post-hoc softmax. The recorded
decision is `not_estimable`: no causal or downstream on/off claim is permitted until a randomized
or otherwise known logging policy records its probability at decision time.

[Sekirei v0.3.68](https://github.com/kent-tokyo/sekirei/releases/tag/v0.3.68) archives a
[hash-verified v0.3.67 deterministic ON/OFF bundle](https://github.com/kent-tokyo/sekirei/blob/v0.3.68/docs/experiments/book_ab_v0.3.67/report.json).
Each arm recorded 128 decisions over four games; the ON arm selected the book twice (1.5625%
coverage), and all games were draws. The score delta was zero with an approximate 95% Elo interval
of [-340.49, +340.49]. The bundle verifies decision-to-terminal joins and fallback accounting, but
its policy recorded no propensities and its game budget is not a downstream-improvement gate. The
2026-10-03 IPS/DR result remains `not_estimable`.

Future logs support IPS/DR only when Sekirei uses an explicitly randomized policy with a known
decision-time distribution. Deterministic paired runs remain useful non-causal diagnostics.
