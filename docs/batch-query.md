# Deterministic batch-query telemetry

`lineprior batch-query` applies exact-state lookups to a frozen schema-v1 prior book. It is an
opt-in telemetry boundary for repeated read-only queries, not an action generator or evaluator.

## Input

Each non-empty JSONL row must contain:

```json
{"batch_query_schema_version":1,"query_id":"case-001","state":"state_a","candidate_budget":3}
```

`query_id` and `state` must be non-empty. Query IDs must be unique within the file.
`candidate_budget`, when present, must be greater than zero and cannot exceed the command-level
`--top-k` bound. Unknown fields, malformed rows, and unsupported future versions are rejected
before the output file is created.

## Output

The first JSONL row is a report header. It records the report schema, producer version, exact query
and prior-book SHA-256 values, prior-artifact input hash and manifest schema, schema-v1 book
metadata, build-config fingerprint, query controls, and query count. Each following row corresponds
to one input query in the same order.

Result status is one of:

- `matched`: at least one observed action passed all thresholds;
- `unseen_state`: the exact opaque state key is absent from the book;
- `insufficient_support`: the state exists, but no action passed the requested thresholds.

Result rows include available, eligible, selected, and truncation counts. Candidate rows copy only
the book's ranked action, count, weighted count, prior, and confidence. One scratch index buffer is
reused across the batch.

The command requires `--artifact-manifest`. It verifies the manifest, exact book digest, and
schema-v1 book metadata before reading results or creating output. Headerless and legacy-header
books remain supported by other readers but are intentionally rejected here because they cannot
provide the complete provenance contract.

## Evidence boundary

Deterministic telemetry proves which observed candidates a fixed book returned under declared
thresholds. It does not prove action legality, future correctness, causal effect, or downstream
improvement. The caller still owns execution, verification, fallback, and paired on/off evaluation.
