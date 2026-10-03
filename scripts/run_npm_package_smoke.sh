#!/usr/bin/env sh
# Builds, packs, installs, and exercises the browser-facing npm/WASM package.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
wasm_pack_bin=${LINEPRIOR_WASM_PACK_BIN:-wasm-pack}
report=""
if [ "$#" -eq 2 ] && [ "$1" = "--out" ]; then
  report=$2
elif [ "$#" -ne 0 ]; then
  echo "usage: $0 [--out npm-package-runtime.json]" >&2
  exit 3
fi
cd "$root"

directory=$(mktemp -d "${TMPDIR:-/tmp}/lineprior-npm-package-XXXXXX")
trap 'rm -rf "$directory"' EXIT
package_dir="$directory/package"
archive_dir="$directory/archive"
consumer_dir="$directory/consumer"
mkdir -p "$archive_dir" "$consumer_dir"

"$wasm_pack_bin" build "$root/crates/lineprior-wasm" \
  --target web \
  --out-dir "$package_dir" \
  --release

# wasm-pack only searches the crate directory for license texts. The workspace
# keeps the canonical copies at its root, so add them to the generated npm
# package before packing instead of duplicating source-controlled license files.
cp "$root/LICENSE-MIT" "$package_dir/LICENSE-MIT"
cp "$root/LICENSE-APACHE" "$package_dir/LICENSE-APACHE"
python3 -c 'import json, pathlib, sys; path=pathlib.Path(sys.argv[1])/"package.json"; package=json.loads(path.read_text()); package["files"] += ["LICENSE-MIT", "LICENSE-APACHE"]; path.write_text(json.dumps(package, indent=2)+"\n")' "$package_dir"

project_version=$(python3 "$root/scripts/version_contract.py" --print)
python3 -c 'import json, pathlib, sys; p=json.loads((pathlib.Path(sys.argv[1])/"package.json").read_text()); assert p["name"] == "lineprior-wasm"; assert p["version"] == sys.argv[2]; assert p["type"] == "module"; assert p["license"] == "MIT OR Apache-2.0"; assert p["files"][-2:] == ["LICENSE-MIT", "LICENSE-APACHE"]' "$package_dir" "$project_version"
test -s "$package_dir/LICENSE-MIT"
test -s "$package_dir/LICENSE-APACHE"

(cd "$package_dir" && npm pack --pack-destination "$archive_dir" --silent >/dev/null)
archive="$archive_dir/lineprior-wasm-$project_version.tgz"
test -s "$archive"
tar -tzf "$archive" | grep -qx 'package/LICENSE-MIT'
tar -tzf "$archive" | grep -qx 'package/LICENSE-APACHE'
npm install --prefix "$consumer_dir" "$archive" --ignore-scripts --no-audit --no-fund >/dev/null
node "$root/examples/wasm/browser-smoke.mjs" "$consumer_dir/node_modules/lineprior-wasm"

if [ -n "$report" ]; then
  python3 -c 'import hashlib, json, pathlib, subprocess, sys; archive=pathlib.Path(sys.argv[1]); dirty=bool(subprocess.check_output(["git","status","--porcelain"], text=True)); data={"protocol":"npm-wasm-package-smoke-v1","package_name":"lineprior-wasm","package_version":sys.argv[2],"git_commit":subprocess.check_output(["git","rev-parse","HEAD"], text=True).strip(),"git_dirty":dirty,"target":"web","node_version":subprocess.check_output(["node","--version"], text=True).strip(),"npm_version":subprocess.check_output(["npm","--version"], text=True).strip(),"wasm_pack_version":subprocess.check_output([sys.argv[4],"--version"], text=True).strip(),"package_sha256":hashlib.sha256(archive.read_bytes()).hexdigest(),"checks":["wasm-pack-build-web","license-files","npm-pack","clean-install","browser-build-query-roundtrip","deterministic-output","error-shape"]}; pathlib.Path(sys.argv[3]).write_text(json.dumps(data, indent=2, sort_keys=True)+"\n")' "$archive" "$project_version" "$report" "$wasm_pack_bin"
  python3 "$root/scripts/validate_npm_package_runtime.py" "$report"
fi

echo "npm/WASM package smoke: ok (lineprior-wasm@$project_version)"
