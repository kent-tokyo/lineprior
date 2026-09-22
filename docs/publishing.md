# Publishing workspace crates

The workspace uses crates.io Trusted Publishing through GitHub Actions OIDC. The workflow is
`.github/workflows/publish.yml` and checks out an exact release tag.

## Current status

All five workspace crates share one workspace version. Verify the tagged version against crates.io
after each release. The one-time bootstrap for the three newer crates is complete; future releases
use the normal OIDC path.

## One-time bootstrap for a new crate

Trusted Publishing cannot create a crate that does not exist yet. For a future new workspace crate,
publish it once with a tightly scoped crates.io token, then configure its Trusted Publisher and
remove or rotate the bootstrap token. Never commit, print, or paste the token into an issue.

```bash
# Local fallback for a new crate only; normally use publish.yml.
cargo login
cargo publish -p lineprior-adapters --locked
cargo publish -p lineprior-similarity --locked
cargo publish -p lineprior-wasm --locked
```

Run the commands from the release tag and in dependency order, after `cargo package --workspace
--locked` and the release checks pass.

## Subsequent releases

Dispatch the workflow once per crate with `release_tag=vX.Y.Z`, `dry_run=false`, and the crate name.
Publish `lineprior` before workspace crates that depend on it. The workflow performs package and
publish dry-runs before requesting OIDC credentials.

## Evidence boundary

A successful package or dry-run is not a publication, and a successful workflow is not a browser
runtime test. Record the workflow URL and crates.io version separately in `CHANGELOG.md`.
