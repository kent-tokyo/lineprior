# lineprior-wasm

Browser-facing WebAssembly bindings for `lineprior`.

The package exposes two synchronous JSON boundaries after asynchronous WASM
initialization:

- `build_json(observationsJsonl, buildConfigJson)`
- `query_json(priorJsonl, state, topK?)`

Build the installable web package from the workspace root:

```sh
wasm-pack build crates/lineprior-wasm --target web --release
```

Repository release checks use `scripts/run_npm_package_smoke.sh`; it also adds
the workspace's canonical MIT and Apache-2.0 license texts before `npm pack`.

The Rust crate is the source of truth for scoring and validation. This wrapper
does not provide file I/O, embeddings, domain parsers, or a Python binding.
