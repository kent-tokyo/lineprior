# Similarity real-data measurement protocol

This protocol is the measurement handoff for Phase B. It does not claim that
similarity improves decisions until a real held-out dataset is supplied.

## Input contract

Prepare three immutable artifacts:

1. `train.jsonl`: observations used to build the prior.
2. `queries.jsonl`: one row per held-out query, containing `query_id`, opaque
   `state`, `expected_action`, and the caller-owned feature vector.
3. `neighbors.jsonl`: deterministic nearest-neighbor output for each query,
   containing `query_id`, neighbor `state`, `distance`, and `provenance`.
   Do not generate actions from the feature model.

Keep the split by sequence/case, and record dataset ID, split rule, feature
version, `SimilarityConfig`, toolchain, hardware, and random seed (if any).

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
