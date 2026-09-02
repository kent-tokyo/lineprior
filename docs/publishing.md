# Publishing workspace crates

The workspace uses crates.io Trusted Publishing (GitHub Actions OIDC). The workflow is
`.github/workflows/publish.yml` and always checks out an exact release tag.

## One-time bootstrap for a new crate

Trusted Publishing cannot create a crate that does not exist yet. For each new workspace crate,
the owner must publish the exact tagged package once with a crates.io API token, then configure its
Trusted Publisher to match this repository, workflow, and `crates-io` environment. Do this only for
the crates listed as pending in `CHANGELOG.md`.

```bash
cargo login
cargo publish -p lineprior-adapters --locked
cargo publish -p lineprior-similarity --locked
cargo publish -p lineprior-wasm --locked
```

The token is read by Cargo locally and must not be committed, placed in workflow YAML, or pasted
into an issue. The commands must be run from the release tag and in dependency order; first verify
that `cargo package --workspace --locked` and the release checks pass.

## Subsequent releases

After the first package exists and its Trusted Publisher is configured, dispatch the workflow once
per crate, with `release_tag=vX.Y.Z`, `dry_run=false`, and the crate name. Publish `lineprior` before
workspace crates that depend on it. The workflow performs package and publish dry-runs before the
OIDC token is requested.

## Evidence boundary

A successful package or dry-run is not a publication. A successful workflow is not a browser runtime
test. Record the workflow URL and crates.io version separately in `CHANGELOG.md`.
