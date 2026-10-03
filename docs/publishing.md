# Publishing workspace crates

The workspace publishes five version-locked crates through
[`publish.yml`](../.github/workflows/publish.yml). GitHub Actions OIDC is the normal crates.io
authentication path. Version `0.12.0` completed the one-time bootstrap for every current crate.
Version `0.12.1` is the current workspace release.

## Release checklist

1. Confirm all manifests and internal dependency requirements use the intended version.
2. Run the workspace quality gate and `cargo package -p lineprior`. Before the new core version is
   visible on crates.io, Cargo cannot fully package the four crates that depend on it.
3. Commit and push a clean tree. Wait for CI and `WASM browser smoke` on that commit.
4. Create and push the exact `vX.Y.Z` tag, then create the GitHub Release.
5. Dispatch `publish.yml` once per crate with that tag and `dry_run=false`.
6. Publish `lineprior` first. After crates.io exposes the new core version, run the workflow's full
   package and dry-run checks while publishing each of the four dependent crates.
7. Verify each workflow conclusion, each crates.io version, the GitHub Release, and the tag commit
   separately. Record the release in `CHANGELOG.md`.

The workflow validates the tag shape, crate allowlist, exact tagged checkout, clean tree, and
manifest/tag version match. It runs `cargo package` and `cargo publish --dry-run` before requesting
credentials.

## New crates require one bootstrap publish

Trusted Publishing cannot create a crate name. If a future workspace crate is added, publish that
crate once with a tightly scoped crates.io token, configure its Trusted Publisher, then remove or
rotate the token. Never commit, print, or paste a token into logs or issues.

```bash
cargo login
cargo publish -p new-crate
```

Run the bootstrap from the reviewed release tag and only after its dependencies are available on
crates.io. Existing crates should use the OIDC workflow.

## Evidence boundary

A successful package or dry-run is not a publication. A successful publish workflow does not prove
registry visibility, browser behavior, or downstream quality. Keep these records separate:

- tag and commit;
- CI and WASM browser-smoke URLs;
- one publish workflow per crate;
- crates.io version pages;
- GitHub Release URL.
