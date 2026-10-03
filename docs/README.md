# Documentation index

Start with the repository [README](../README.md) or [Japanese README](../README_ja.md). They cover
installation, the JSONL contract, core commands, and feature boundaries. Use `lineprior <command>
--help` for the complete CLI option reference and [docs.rs](https://docs.rs/lineprior) for the Rust
API.

## Evidence and measurement

- [Similarity real-data protocol](measurements/similarity-real-data.md): exact, similarity, and
  no-prior held-out comparison, including the rejected Sekirei adapter result.
- [IPS/DR real-log protocol](measurements/offpolicy-real-data.md): propensity, overlap, bootstrap,
  paired prior-on/off requirements, and the current Sekirei not-estimable audit.
- [GateModel real-history readiness](measurements/gate-model-real-history.md): candidate-level
  admission rules and the current no-feature-table, not-estimable decision.
- [Runtime and WASM evidence](measurements/ecosystem-compatibility.md): maintained CI matrix and
  the claims it does and does not support.
- [Sekirei opening-book case study](case-studies/sekirei-opening-book.md): real adapter wiring and
  an inconclusive 40-game on/off diagnostic.
- [Historical benchmark](benchmarks/v0.11.0-m4-arm64-2026-09-02.md): one synthetic Apple M4
  snapshot, retained for reproduction rather than current performance claims.

The JSON Schemas in `measurements/` validate artifact structure. The repository's semantic
validator separately checks numeric ranges, lineage equality, hashes, and fixed-version rules.

## Release operations

- [Publishing workspace crates](publishing.md): release checklist, OIDC publishing, and the
  one-time new-crate bootstrap boundary.
- [Changelog](../CHANGELOG.md): published history and the single current `Unreleased` section.

## Examples

- [Typed adapter boundaries](../examples/adapters.md)
- [veridict prior on/off protocol](../examples/veridict_prior_comparison.md)
- JSONL fixtures and Python, Node.js, and browser smoke examples under [`examples/`](../examples/)

`ROADMAP.md` is an internal planning file and is intentionally excluded from published artifacts.
