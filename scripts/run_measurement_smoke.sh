#!/usr/bin/env sh
# Verifies the checked-in similarity and paired on/off measurement runners.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
binary=${LINEPRIOR_BIN:-$root/target/debug/lineprior}
if [ ! -x "$binary" ]; then
  echo "lineprior executable not found or not executable: $binary" >&2
  exit 2
fi

directory=$(mktemp -d "${TMPDIR:-/tmp}/lineprior-measurement-XXXXXX")
trap 'rm -rf "$directory"' EXIT
prior="$directory/prior.jsonl"
similarity_first="$directory/similarity-1.json"
similarity_second="$directory/similarity-2.json"
paired_first="$directory/paired-1.json"
paired_second="$directory/paired-2.json"

"$binary" build "$root/crates/lineprior-similarity/tests/fixtures/unseen_states.jsonl" --out "$prior" >/dev/null
python3 "$root/scripts/measure_similarity.py" "$prior" "$root/examples/similarity_queries.jsonl" --out "$similarity_first"
python3 "$root/scripts/measure_similarity.py" "$prior" "$root/examples/similarity_queries.jsonl" --out "$similarity_second"
python3 -c 'import json, pathlib, sys; a=json.loads(pathlib.Path(sys.argv[1]).read_text()); b=json.loads(pathlib.Path(sys.argv[2]).read_text()); [x.pop(k, None) for x in (a["arms"].values()) for k in ("latency_us_p50", "latency_us_p95", "peak_rss_kb")]; [x.pop(k, None) for x in (b["arms"].values()) for k in ("latency_us_p50", "latency_us_p95", "peak_rss_kb")]; assert a == b; r=a; assert r["arms"]["exact"]["coverage"] == 0.5; assert r["arms"]["similarity"]["coverage"] == 1.0; assert r["arms"]["no_prior"]["abstention_rate"] == 1.0' "$similarity_first" "$similarity_second"

python3 "$root/scripts/compare_offpolicy_arms.py" "$root/examples/offpolicy_off.jsonl" "$root/examples/offpolicy_on.jsonl" --out "$paired_first" --bootstrap-resamples 64 --bootstrap-seed 42
python3 "$root/scripts/compare_offpolicy_arms.py" "$root/examples/offpolicy_off.jsonl" "$root/examples/offpolicy_on.jsonl" --out "$paired_second" --bootstrap-resamples 64 --bootstrap-seed 42
cmp -s "$paired_first" "$paired_second"
python3 -c 'import json, pathlib, sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); assert r["paired_rows"] == 2; assert r["paired_reward_delta_on_minus_off"] == 0.5; assert r["off"]["overlap_failures"] == 0' "$paired_first"
echo "measurement smoke: ok (similarity arms + paired OPE audit)"
