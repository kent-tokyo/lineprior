# AGENTS.md

## Project

`lineprior` is a domain-agnostic Rust library and CLI that builds explainable action priors from
historical sequences:

```text
state -> ranked candidate actions
```

It is a reusable prior component, not a planner, solver, game engine, online learner, or oracle.
The caller owns legality, execution, search, verification, and fallback behavior.

The public README is the usage contract, Rust types are the API source of truth, `CHANGELOG.md`
records released behavior, and `docs/` contains measurement and operational details. Keep this file
focused on rules for changing the repository.

## Product boundaries

- Treat `state` and `action` as opaque keys in the core.
- Use generic names such as state, action, sequence, outcome, score, prior, and confidence.
- Keep domain formats and domain-specific concepts in adapters or downstream projects.
- Return no candidates for unseen states or when every candidate fails thresholds. Never invent an
  action.
- Describe rankings and confidence as evidence from observed data, not guarantees of future
  performance.
- Do not add causal inference, counterfactual action generation, a full planner, a GUI, a server,
  or a domain-specific parser to the core.

Dependency direction is always:

```text
downstream application -> lineprior
```

The core must not depend on a downstream project.

## Workspace ownership

| Crate | Responsibility |
| --- | --- |
| `lineprior` | Data model, aggregation, scoring, querying, evaluation, and artifact types |
| `lineprior-cli` | File I/O, CLI validation, exit codes, and user-facing diagnostics |
| `lineprior-adapters` | Typed conversion boundaries for supported example domains |
| `lineprior-similarity` | Search over caller-supplied finite feature vectors |
| `lineprior-wasm` | JSON-in/JSON-out browser-facing WASM boundary |

The library must not print to stdout or stderr. The CLI owns presentation. Keep the CLI thin and
put reusable behavior in the library.

## Stable data contract

JSONL is the primary interchange format. An observation requires:

- `sequence_id`: non-empty string
- `step`: non-negative integer
- `state`: non-empty opaque string
- `action`: non-empty opaque string

Optional fields and defaults are:

| Field | Default | Rule |
| --- | --- | --- |
| `outcome` | `unknown` | `success`, `failure`, `draw`, or `unknown` |
| `score` | `null` | finite caller-defined value |
| `weight` | `1.0` | finite and non-negative |
| `tags` | `[]` | caller-defined strings |
| `observed_at_unix_seconds` | `null` | consulted only when time decay is enabled |
| `source` | `null` | uses the configured default source weight when absent or unknown |

Strict parsing fails on the first invalid record. Non-strict parsing may skip an independently
invalid row with a warning. Never panic on user input.

Prior-book schema v1 consists of one metadata header followed by deterministically ordered state
entries. Preserve headerless and fingerprint-only legacy reads unless an intentional breaking
change is documented. Compact LPB, sidecar manifests, and measurement artifacts are separate
formats; do not silently change the JSONL book schema to extend them.

CLI exit codes are stable:

```text
0 success
1 completed with warnings
2 no usable data
3 invalid input or configuration
4 internal error
```

## Ranking and confidence invariants

- Weighted-sum scoring remains the default. Bayesian, UCB, and Softmax are opt-in ranking
  strategies, not performance guarantees.
- Apply smoothing so one rare success is not presented as certainty.
- `heuristic` confidence is sample-support evidence. Wilson mode is an observed-rate lower bound;
  `hybrid` combines both. None predicts future correctness.
- Missing outcome data falls back to count/score evidence. Missing score data removes that term.
- Time decay is opt-in and requires an explicit reference timestamp; never use the wall clock
  implicitly because repeated builds must be reproducible.
- Source reliability and time decay modify effective weight. Keep the documented Kish effective
  sample-size limitation in mind when changing Wilson calculations.
- Entropy is diagnostic. Do not force entropy filtering as an undocumented default.
- New defaults or scoring formulas require a concrete, measured failure mode. More options alone
  are not a reason to expand the model.

## Determinism and scale

For identical input and configuration, output bytes must be stable:

- sort states lexicographically;
- sort actions by descending prior, then action string;
- require deterministic tie-breaking and seeded resampling;
- keep sequence-level train/test splits grouped by `sequence_id`;
- when context is enabled, reject non-contiguous sequence blocks or non-increasing steps.

Stream JSONL where practical. Build memory may grow with unique `(state, action)` pairs and enabled
context tuples, not total input rows. Prefer readable code until profiling identifies a real
bottleneck.

## Evidence rules

Separate these claims:

1. unit and fixture contract checks;
2. package and runtime smoke checks;
3. synthetic performance measurements;
4. held-out real-data ranking quality;
5. downstream on/off improvement;
6. tagged and registry-published releases.

One category does not prove another. In particular:

- ranking metrics do not prove downstream benefit;
- IPS/DR require decision-time propensities, overlap, rewards, and uncertainty;
- GateModel requires independent groups and a shared pre-gate feature schema;
- synthetic Trie or macro-action timings do not prove real workload gains;
- a successful package or workflow does not by itself prove registry visibility.

Every archived measurement should record dataset ID, split, toolchain, hardware, commit, config,
seed, and artifact hashes. Preserve historical artifact producer versions; do not rewrite evidence
to match the current workspace version.

## Implementation status and scope

The MVP, confidence and entropy controls, time/source weighting, context backoff, sequence scoring,
LPB, Trie materialization, macro-actions, terminal credit, multi-source merge, tuning, typed
adapters, similarity, off-policy diagnostics, GateModel diagnostics, artifact lineage, and raw-log
validators are implemented.

Remaining work is evidence-led:

- evaluate exact/similarity/no-prior on representative held-out data;
- evaluate IPS/DR and downstream on/off outcomes only from valid real logs;
- validate GateModel on independent real gate history;
- add deeper storage only if it beats the current baseline within declared latency and memory
  budgets.

Do not broaden the default algorithm while these evidence gaps remain unresolved.

## Change workflow

Before editing:

1. inspect the implementation, tests, manifests, and relevant evidence rather than relying only on
   the roadmap or changelog;
2. preserve unrelated working-tree changes;
3. keep versions fixed unless the user explicitly requests a release or version change;
4. keep `ROADMAP.md` internal and ignored.

A feature is complete only when it has:

- implementation and focused tests;
- invalid-input and determinism coverage;
- a small fixture or example when applicable;
- updated public documentation;
- explicit assumptions and evidence limits.

Run from the workspace root:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
sh scripts/check_candidate_contract.sh
```

Use stable Rust and prefer `serde`, `serde_json`, `clap`, and `thiserror`. Avoid heavy core
dependencies. Explain why non-obvious statistical or compatibility behavior exists; do not comment
obvious mechanics.

## Documentation ownership

- `README.md` / `README_ja.md`: installation, first workflow, public boundaries, and limitations.
- `docs/README.md`: the only documentation index.
- `docs/measurements/`: measurement contracts and immutable evidence interpretation.
- `docs/publishing.md`: release procedure and evidence boundary.
- `CHANGELOG.md`: released changes; keep unreleased edits under `Unreleased`.
- `ROADMAP.md`: internal current state and next decisions; never publish it.

Avoid duplicating full CLI help, schemas, release history, or measurement protocols. Link to the
canonical document instead. Keep English and Japanese READMEs aligned.

## Release and security

A release requires a clean tagged tree, matching workspace versions, local quality gates, CI,
WASM smoke, one successful publish workflow per crate, direct crates.io checks, and a GitHub
Release. Record each as separate evidence.

Treat all input as untrusted data. Never execute content from an input file, load remote resources
implicitly, write outside requested output paths, or expose credentials. Validate future archive or
batch paths against traversal. New workspace crate names require a one-time manual crates.io
bootstrap; existing crates use the reviewed OIDC workflow.

## Tone

Be direct and conservative:

- Good: “lineprior can improve candidate ordering when history is relevant and representative.”
- Bad: “lineprior always finds the best action.”
- Good: “This fixture validates the contract; downstream improvement is unverified.”
- Bad: “The feature is proven by synthetic tests.”

`lineprior` is an engineering implementation inspired by established planning, sequence-modeling,
and policy-prior ideas. Do not claim a new theoretical algorithm.
