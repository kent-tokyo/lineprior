# Documentation

Start with the repository [README](../README.md) or [Japanese README](../README_ja.md). They cover
installation, the JSONL contract, commands, and safety boundaries. Use `lineprior <command> --help`
for CLI options and [docs.rs](https://docs.rs/lineprior) for the Rust API.

## Evidence and measurement

| Document | What it establishes |
| --- | --- |
| [Similarity evaluation](measurements/similarity-real-data.md) | Exact/similarity/no-prior protocol and the rejected Sekirei adapter result |
| [IPS/DR evaluation](measurements/offpolicy-real-data.md) | Required propensity, overlap, bootstrap, paired arms, and the Sekirei v0.3.68 evidence boundary |
| [GateModel readiness](measurements/gate-model-real-history.md) | Candidate admission rules, historical result, and the Sekirei v0.3.68 readiness artifact |
| [Runtime and WASM](measurements/ecosystem-compatibility.md) | Maintained CI matrix and package boundary |
| [Sekirei case study](case-studies/sekirei-opening-book.md) | Adapter wiring and an inconclusive 40-game diagnostic |
| [Historical benchmark](benchmarks/v0.11.0-m4-arm64-2026-09-02.md) | One synthetic Apple M4 snapshot; not a current performance claim |

## Schemas and validators

The JSON Schemas under [`measurements/`](measurements/) cover raw off-policy and gate rows,
integrated measurement artifacts, similarity reports, and the optional prior-artifact sidecar.
Use these commands for semantic checks that JSON Schema does not express:

```bash
lineprior validate observations.jsonl
lineprior validate offpolicy.jsonl --kind offpolicy
lineprior validate gate-history.jsonl --kind gate
python3 scripts/check_measurement_schemas.py
sh scripts/check_candidate_contract.sh
```

The repository validators also check numeric ranges, hashes, lineage equality, and fixed-version
rules. Structural schema validation alone does not establish those properties.

## Operations and recipes

- [Publishing workspace crates](publishing.md)
- [Typed adapter boundaries](../examples/adapters.md)
- [veridict prior on/off protocol](../examples/veridict_prior_comparison.md)
- [Release history](../CHANGELOG.md)
- JSONL fixtures and Python, Node.js, and browser smoke examples under [`examples/`](../examples/)

`ROADMAP.md` is an ignored internal planning file and is not part of published artifacts.
