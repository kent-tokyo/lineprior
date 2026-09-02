# Publishing workspace crates

The workspace uses crates.io Trusted Publishing (GitHub Actions OIDC). The workflow is
`.github/workflows/publish.yml` and always checks out an exact release tag.

## One-time bootstrap for a new crate

Trusted Publishing cannot create a crate that does not exist yet. The publish workflow now prefers
the repository's explicitly configured `CARGO_REGISTRY_TOKEN` secret when present, so it can perform
this one-time bootstrap without changing the release tag. The secret must be a crates.io API token
with publish permission; it is masked by GitHub and must never be printed. After creation, configure
each crate's Trusted Publisher to match this repository, workflow, and `crates-io` environment, then
remove or rotate the bootstrap token if it is no longer needed.

```bash
# Local fallback only; normally dispatch publish.yml with CARGO_REGISTRY_TOKEN configured.
cargo login
cargo publish -p lineprior-adapters --locked
cargo publish -p lineprior-similarity --locked
cargo publish -p lineprior-wasm --locked
```

The token must not be committed, placed in workflow YAML, or pasted into an issue. The commands must
be run from the release tag and in dependency order; first verify that `cargo package --workspace
--locked` and the release checks pass.

## Subsequent releases

After the first package exists and its Trusted Publisher is configured, dispatch the workflow once
per crate, with `release_tag=vX.Y.Z`, `dry_run=false`, and the crate name. Publish `lineprior` before
workspace crates that depend on it. The workflow performs package and publish dry-runs before the
OIDC token is requested.

## Evidence boundary

A successful package or dry-run is not a publication. A successful workflow is not a browser runtime
test. Record the workflow URL and crates.io version separately in `CHANGELOG.md`.
