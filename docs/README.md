# Documentation

Start with the repository [README](../README.md) or [Japanese README](../README_ja.md). They cover
installation, the JSONL contract, commands, and safety boundaries. Use `lineprior <command> --help`
for CLI options and [docs.rs](https://docs.rs/lineprior) for the Rust API.

## Evidence and measurement

| Document | What it establishes |
| --- | --- |
| [Similarity evaluation](measurements/similarity-real-data.md) | Exact/similarity/no-prior protocol and the decision rule for retaining abstention |
| [IPS/DR evaluation](measurements/offpolicy-real-data.md) | Required propensity, overlap, bootstrap, and paired arms |
| [GateModel readiness](measurements/gate-model-real-history.md) | Candidate admission rules and the requirements for real-history validation |
| [Runtime and WASM](measurements/ecosystem-compatibility.md) | Maintained CI matrix and package boundary |
| [Candidate-ordering integration](case-studies/candidate-ordering-integration.md) | Coverage preflight, paired evaluation artifact, and adoption criteria |
| [Historical benchmark](benchmarks/v0.11.0-m4-arm64-2026-09-02.md) | One synthetic Apple M4 snapshot; not a current performance claim |

## Schemas and validators

The JSON Schemas under [`measurements/`](measurements/) cover candidate-ordering evaluations, raw
off-policy and gate rows, integrated measurement artifacts, similarity reports, and the optional
prior-artifact sidecar.
Use these commands for semantic checks that JSON Schema does not express:

```bash
lineprior validate observations.jsonl
lineprior validate offpolicy.jsonl --kind offpolicy
lineprior validate gate-history.jsonl --kind gate
lineprior verify-artifact --manifest prior.manifest.json --input observations.jsonl --book prior.jsonl
python3 scripts/check_measurement_schemas.py
python3 scripts/validate_measurement_artifact.py candidate-ordering report.json --require-explicit-lineage
sh scripts/check_candidate_contract.sh
```

`verify-artifact` streams both files through SHA-256 and checks the embedded `BuildConfig`, its
fingerprint, and schema-v1 book header. Legacy/headerless books remain readable but return a
provenance warning. Repository validators also check numeric ranges, lineage equality, and
fixed-version rules. Structural schema validation alone does not establish those properties.

## Operations and recipes

- [Deterministic batch-query telemetry](batch-query.md)
- [Publishing workspace crates](publishing.md)
- [Typed adapter boundaries](../examples/adapters.md)
- [veridict prior on/off protocol](../examples/veridict_prior_comparison.md)
- [Release history](../CHANGELOG.md)
- JSONL fixtures and Python, Node.js, and browser smoke examples under [`examples/`](../examples/)

`ROADMAP.md` is an ignored internal planning file and is not part of published artifacts.
