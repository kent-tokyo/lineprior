# GateModel real-history readiness

GateModel consumes one row per completed candidate, with a stable numeric feature map fixed before
the gate, an opaque leakage group, and the eventual Elo result. Opening shards, repeated snapshots,
and the same candidate under another time control are not independent rows.

New exports should follow
[`gate-observation-v1.schema.json`](gate-observation-v1.schema.json) and pass
`lineprior validate history.jsonl --kind gate` before model fitting. Collection-time validation
checks identifiers, finite values, uncertainty fields, and one shared feature-name set without
requiring enough rows to fit a model.

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

## Next validation

[Sekirei v0.3.68](https://github.com/kent-tokyo/sekirei/releases/tag/v0.3.68) retains the
fail-closed exporter and archives a
[v0.3.67 readiness bundle](https://github.com/kent-tokyo/sekirei/blob/v0.3.68/docs/experiments/gate_model_readiness_v0.3.67.json).
Its only terminal strength-gate manifest lacked the required pre-gate declaration, so the exporter
admitted zero rows and quarantined one. The zero-row artifact preserves the source hash and blocker
instead of presenting the old result as model-ready. GateModel fitting, verdict calibration, and
acquisition therefore remain `not_computable`.

The repository pins Sekirei's v0.3.68 `gate_observation_v1.jsonl` fixture as a schema and semantic
regression. The upstream tag resolves to `6b00f0eaea9b3bc1153c25f4b6ddcd33a0cfdbc7`; the source fixture
SHA-256 is `64cbdd9f9c7d1ba76e7aab8a1155756c6030d5d232cd8e931e42f1d5f606075c`. This proves format
compatibility only, not that real history is model-ready.

The next step is to export several independent groups, validate the JSONL with `lineprior validate
--kind gate`, and archive the admitted/rejected counts and hashes before fitting. Do not reconstruct
features from post-gate outcomes.
