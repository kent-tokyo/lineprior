# veridict prior on/off comparison recipe

This is a reproducible protocol template, not downstream evidence. Run the
same `veridict` commit, dataset split, seeds, stopping rule, hardware budget,
and candidate set in two paired arms: **off** ranks without lineprior; **on**
only reorders or filters candidates with lineprior. Keep verifier, evaluator,
and stopping rule identical.

Report paired downstream success/gate quality, coverage, fallback rate, wall
time, evaluations, and abstentions. Record `veridict_commit`,
`lineprior_version` (`0.11.1`), dataset ID, split, seeds, candidate budget,
and exact commands in a manifest. Store `off.jsonl`, `on.jsonl`, and
`manifest.json` under an artifact directory.

```bash
lineprior build history.jsonl --out prior.jsonl --config best_config.json
lineprior eval history.jsonl --config best_config.json
lineprior pack prior.jsonl --out prior.lpb
```

Fail closed on a missing/malformed prior; record unseen-state fallbacks. Use
paired bootstrap intervals and report supported-decision counts. A ranking
metric alone does not pass a veridict quality gate.
