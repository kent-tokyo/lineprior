# Candidate-ordering integration guide

This guide explains how to evaluate a `lineprior` integration without treating a ranking metric or
a small diagnostic as evidence of downstream improvement. It is deliberately domain-neutral.

## Responsibility boundary

The downstream application owns candidate generation, legality, execution, verification, search,
evaluation, and fallback. `lineprior` consumes opaque historical observations and returns either a
ranking of observed actions or no candidates.

Keep the integration fail-closed: an absent, malformed, or low-support prior must produce the same
fallback path as no prior. Record whether each decision used a prior, abstained, or fell back.

## Artifact contract

Use
[`candidate-ordering-evaluation-v1.schema.json`](../measurements/candidate-ordering-evaluation-v1.schema.json)
for an archived evaluation. It records the dataset and split, exact input and result hashes,
lineprior version and config fingerprint, downstream commit, toolchain, hardware, seed,
preregistered quality/cost gates, coverage preflight, paired arms, and terminal decision.

Validate both structure and cross-field semantics:

```bash
python3 scripts/check_measurement_schemas.py
python3 scripts/validate_measurement_artifact.py candidate-ordering report.json \
  --require-explicit-lineage
```

The checked-in
[`candidate_ordering_not_ready.json`](../../tests/fixtures/candidate_ordering_not_ready.json) is a
synthetic contract fixture. It is not downstream evidence.

## Coverage preflight

Before spending the full case budget, run the ON path over a small held-out sample. Predeclare the
minimum number and rate of actual prior selections. Partition every eligible decision into a prior
selection, prior abstention, or unavailable-prior fallback, and account for every fallback reason.

If either threshold fails, leave both evaluation arms and result hashes null and record
`NOT_READY`. Do not run the expensive paired evaluation or interpret zero observed difference as
evidence of no effect.

## Paired evaluation

Compare two arms over the same held-out cases, binary, candidate budget, and runtime controls.

| Arm | Behaviour |
| --- | --- |
| `off` | Use the downstream application's normal candidate ordering. |
| `on` | Allow `lineprior` only to reorder or filter observed candidates. |

Predeclare the quality condition that must not regress, its direction and margin, and the cost
measure expected to improve.
Typical cost measures are candidate evaluations, search nodes, or wall-clock time. Record quality,
prior use, abstention, fallback, cost, configuration, toolchain, hardware, commit, split, and
artifact hashes for both arms.

## Adoption decision

Adopt an integration only when the held-out paired result meets the declared quality condition and
improves the chosen cost measure with credible uncertainty. A successful build, a deterministic
fixture, broad ranking coverage, or a small ON/OFF run establishes only its own contract.

Use `PASS` only when both preregistered gates pass, `FAIL` when either gate fails, and
`INCONCLUSIVE` when uncertainty leaves at least one gate unresolved without a demonstrated
failure. These statuses describe the declared experiment; they are not predictions of future
performance.

If the prior is rarely selected, produces low coverage, or fails the quality or cost gate, keep it
optional or remove the integration. Do not change a default ranking strategy from that result.
