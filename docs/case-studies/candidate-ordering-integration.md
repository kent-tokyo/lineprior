# Candidate-ordering integration guide

This guide explains how to evaluate a `lineprior` integration without treating a ranking metric or
a small diagnostic as evidence of downstream improvement. It is deliberately domain-neutral.

## Responsibility boundary

The downstream application owns candidate generation, legality, execution, verification, search,
evaluation, and fallback. `lineprior` consumes opaque historical observations and returns either a
ranking of observed actions or no candidates.

Keep the integration fail-closed: an absent, malformed, or low-support prior must produce the same
fallback path as no prior. Record whether each decision used a prior, abstained, or fell back.

## Paired evaluation

Compare two arms over the same held-out cases, binary, candidate budget, and runtime controls.

| Arm | Behaviour |
| --- | --- |
| `off` | Use the downstream application's normal candidate ordering. |
| `on` | Allow `lineprior` only to reorder or filter observed candidates. |

Predeclare the quality condition that must not regress and the cost measure expected to improve.
Typical cost measures are candidate evaluations, search nodes, or wall-clock time. Record quality,
prior use, abstention, fallback, cost, configuration, toolchain, hardware, commit, split, and
artifact hashes for both arms.

## Adoption decision

Adopt an integration only when the held-out paired result meets the declared quality condition and
improves the chosen cost measure with credible uncertainty. A successful build, a deterministic
fixture, broad ranking coverage, or a small ON/OFF run establishes only its own contract.

If the prior is rarely selected, produces low coverage, or fails the quality or cost gate, keep it
optional or remove the integration. Do not change a default ranking strategy from that result.
