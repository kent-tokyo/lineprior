# GateModel real-history readiness

GateModel consumes one row per completed candidate, with a stable numeric feature map fixed before
the gate, an opaque leakage group, and the eventual Elo result. Opening shards, repeated snapshots,
and the same candidate under another time control are not independent rows.

New exports should follow
[`gate-observation-v1.schema.json`](gate-observation-v1.schema.json) and pass
`lineprior validate history.jsonl --kind gate` before model fitting. Collection-time validation
checks identifiers, finite values, uncertainty fields, and one shared feature-name set without
requiring enough rows to fit a model.

Run the readiness audit against a pinned downstream result directory before fitting anything:

```bash
python3 scripts/audit_gate_history_readiness.py /path/to/downstream-results \
  --dataset-id candidate-gate-history-2026-10-03 \
  --dataset-source-commit <commit> \
  --out gate-history-readiness.json
```

The audit discovers aggregate `combined.json`, `final.json`, `result.json`, and `summary.json`
files with at least 20 games. This is a broad inventory, not automatic admission into the training
set. Every discovered file is content-hashed; the source commit identifies the surrounding code
because the downstream result files are ignored by its Git repository.

## Readiness decision

The checked-in reference audit found aggregate outcomes without a shared pre-gate feature map and
independent group IDs. No rows were therefore admissible for model fitting. Aggregate or repeated
results must not be promoted into synthetic candidates.

Keep GateModel experimental and do not claim verdict calibration or acquisition efficiency until a
history contains several independent groups, stable numeric features declared before each gate, and
the required uncertainty fields. Archive the admitted and rejected counts with their hashes before
fitting. Do not reconstruct features from post-gate outcomes.
