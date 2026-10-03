# GateModel real-history readiness

GateModel consumes one row per completed candidate, with a stable numeric feature map fixed before
the gate, an opaque leakage group, and the eventual Elo result. Opening shards, repeated snapshots,
and the same candidate under another time control are not independent rows.

Run the readiness audit against a pinned downstream checkout before fitting anything:

```bash
python3 scripts/audit_gate_history_readiness.py /path/to/sekirei \
  --dataset-id sekirei-gate-aggregates-ea06a1e-2026-10-03 \
  --dataset-source-commit ea06a1e2c7b87f57d8912c8932cc658283583982 \
  --out docs/measurements/gate-history-sekirei-readiness-2026-10-03.json
```

The audit discovers aggregate `combined.json`, `final.json`, `result.json`, and `summary.json`
files with at least 20 games. This is a broad inventory, not automatic admission into the training
set. Every discovered file is content-hashed; the source commit identifies the surrounding code
because the downstream result files are ignored by its Git repository.

## 2026-10-03 result

The Sekirei checkout contained 23 aggregate outcome files. Only two carried an explicit candidate
identity, none carried a `group_id`, and none carried a shared numeric pre-gate `features` map.
There were therefore zero admissible `GateObservation` rows. Shards or repeated aggregate/final
files were not promoted into fake candidates.

The mean baseline, ridge without OOD, current GateModel, caller rule, verdict calibration, OOD
abstention, and acquisition efficiency are all `not_computable`. The decision is to keep GateModel
experimental, avoid splitting `lineprior-gate`, and make no acquisition-efficiency claim.

## Required next history contract

Future candidate records need a candidate ID, a predeclared group ID, a versioned numeric feature
map captured before gating, Elo and uncertainty, terminal verdict, caller-rule decision, expected
gate cost, and candidate/baseline/recipe/dataset/code lineage. Once several independent groups share
that exact feature schema, the declared model comparison can run without reconstructing features
from post-gate outcomes.
