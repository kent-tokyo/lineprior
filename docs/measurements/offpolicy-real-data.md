# IPS / DR real-log measurement protocol

This protocol is the measurement handoff for Phase C. `lineprior` estimates
what the supplied logged-policy assumptions imply; it cannot repair missing
propensities or create counterfactual outcomes.

## Required log fields

Every held-out row must include an immutable case/sequence ID, state, logged
action, reward, logging-policy propensity for that action, and evaluation
policy probability for that same action. For DR, also provide both explicit
reward-model values: evaluation-policy value and logged-action value. Record
policy commit/config, lineprior config/version, dataset/split ID, and schema
version.
The paired runner records `dataset_id`, `split`, and `lineprior_version` in
its output; pass them explicitly rather than relying on defaults.

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
