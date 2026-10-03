# Similarity real-data measurement protocol

Use this protocol to compare similarity fallback on real held-out data. The protocol does not
claim that similarity improves decisions before such data is supplied.
The machine-readable artifact contract is
[`similarity-real-data-v1.schema.json`](similarity-real-data-v1.schema.json).
The repository validator additionally enforces fixed-version, numeric-range,
and cross-artifact semantics that JSON Schema alone cannot express.

## Input contract

Prepare three immutable artifacts:

1. `train.jsonl`: observations used to build the prior.
2. `queries.jsonl`: one row per held-out query, containing `query_id`, opaque
   `state`, `expected_action`, and deterministic caller-owned `neighbors`.
3. The upstream neighbor-generation input or manifest. Record its hash and
   feature version even though the runner consumes neighbors inline. Do not
   generate actions from the feature model.

Keep the split by sequence/case, and record dataset ID, split rule, feature
version, `SimilarityConfig`, toolchain, hardware, and random seed (if any).
The runner records `dataset_id`, `split`, `feature_version`, and
`lineprior_version` in its output; pass them explicitly for every artifact.
Before archiving a real-data report, run the validator with
`--require-explicit-lineage`; this rejects placeholder metadata such as
`unspecified` while retaining a permissive mode for local fixture development.

## Paired arms

Evaluate every query with the same train book and candidate budget:

- `exact`: `PriorBook::query` on the query state.
- `similarity`: caller nearest-neighbor search followed by
  `PriorBook::query_with_similarity`.
- `no-prior`: empty candidate set, with an explicit abstention.

Report coverage, top-1, MRR, confidence calibration (confidence-bin hit rate
and Brier-style error), abstention rate, p50/p95 latency, and peak RSS for
each arm. Latency and memory must be measured in separate warm-up/repeated
runs with the same process and input order. The no-prior arm is a baseline,
not a failed prediction.

The gate is paired held-out improvement at a declared false-recommendation
and coverage budget. If similarity only increases coverage while worsening
false recommendations or calibration, keep exact-match plus abstention as
the default. Synthetic fixtures and the checked-in unseen-state test verify
the contract only; they are not real-data evidence.

## Sekirei 2023-2025 result

The checked-in [Sekirei artifact](similarity-sekirei-2026-10-03.json) uses 2023-2024 Floodgate
records for training and 2025 records for holdout (210 query states). The current SFEN one-hot
adapter increased coverage from 0.548 to 0.743, but reduced all-query top-1 from 0.505 to 0.148 and
MRR from 0.525 to 0.323. Among covered queries, its false-recommendation rate was 0.801 versus
0.078 for exact match. It therefore fails the declared error and MRR budgets: keep exact match plus
abstention and do not adopt this similarity adapter.
