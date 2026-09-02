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
integrated="$directory/integrated.json"

"$binary" build "$root/crates/lineprior-similarity/tests/fixtures/unseen_states.jsonl" --out "$prior" >/dev/null
python3 "$root/scripts/measure_similarity.py" "$prior" "$root/examples/similarity_queries.jsonl" --out "$similarity_first" --dataset-id fixture-v1 --split heldout --feature-version fixture-v1 --lineprior-version 0.11.1
python3 "$root/scripts/measure_similarity.py" "$prior" "$root/examples/similarity_queries.jsonl" --out "$similarity_second" --dataset-id fixture-v1 --split heldout --feature-version fixture-v1 --lineprior-version 0.11.1
python3 -c 'import json, pathlib, sys; a=json.loads(pathlib.Path(sys.argv[1]).read_text()); b=json.loads(pathlib.Path(sys.argv[2]).read_text()); [x.pop(k, None) for x in (a["arms"].values()) for k in ("latency_us_p50", "latency_us_p95", "peak_rss_kb")]; [x.pop(k, None) for x in (b["arms"].values()) for k in ("latency_us_p50", "latency_us_p95", "peak_rss_kb")]; assert a == b; r=a; assert r["measurement"]["prior_config_fingerprint"] != "unspecified"; assert set(r["measurement"]["input_sha256"]) == {"prior", "queries"}; assert r["arms"]["exact"]["coverage"] == 0.5; assert r["arms"]["similarity"]["coverage"] == 1.0; assert r["arms"]["no_prior"]["abstention_rate"] == 1.0' "$similarity_first" "$similarity_second"
python3 "$root/scripts/validate_measurement_artifact.py" similarity "$similarity_first" --require-explicit-lineage
bad_lineage="$directory/similarity-bad-lineage.json"
python3 -c 'import json, pathlib, sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); r["measurement"]["dataset_id"]="unspecified"; pathlib.Path(sys.argv[2]).write_text(json.dumps(r))' "$similarity_first" "$bad_lineage"
if python3 "$root/scripts/validate_measurement_artifact.py" similarity "$bad_lineage" --require-explicit-lineage >/dev/null 2>&1; then exit 1; fi
bad_queries="$directory/similarity-bad-queries.jsonl"
python3 -c 'import pathlib, sys; row=pathlib.Path(sys.argv[1]).read_text().splitlines()[0].replace("\"expected_action\":\"click:add-to-cart\",", "\"expected_action\":\"\","); pathlib.Path(sys.argv[2]).write_text(row+"\n")' "$root/examples/similarity_queries.jsonl" "$bad_queries"
if python3 "$root/scripts/measure_similarity.py" "$prior" "$bad_queries" --out "$directory/similarity-bad-queries.json" >/dev/null 2>&1; then exit 1; fi
bad_version="$directory/similarity-bad-version.json"
python3 -c 'import json, pathlib, sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); r["measurement"]["lineprior_version"] = "0.11.0"; pathlib.Path(sys.argv[2]).write_text(json.dumps(r))' "$similarity_first" "$bad_version"
if python3 "$root/scripts/validate_measurement_artifact.py" similarity "$bad_version" >/dev/null 2>&1; then exit 1; fi

python3 "$root/scripts/compare_offpolicy_arms.py" "$root/examples/offpolicy_off.jsonl" "$root/examples/offpolicy_on.jsonl" --out "$paired_first" --bootstrap-resamples 64 --bootstrap-seed 42
python3 "$root/scripts/compare_offpolicy_arms.py" "$root/examples/offpolicy_off.jsonl" "$root/examples/offpolicy_on.jsonl" --out "$paired_second" --bootstrap-resamples 64 --bootstrap-seed 42
cmp -s "$paired_first" "$paired_second"
python3 -c 'import json, pathlib, sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); assert r["paired_rows"] == 2; assert r["paired_reward_delta_on_minus_off"] == 0.5; assert r["off"]["overlap_failures"] == 0' "$paired_first"
python3 "$root/scripts/measure_offpolicy_arms.py" "$root/examples/offpolicy_off.jsonl" "$root/examples/offpolicy_on.jsonl" --lineprior-bin "$binary" --out "$integrated" --dataset-id fixture-v1 --split heldout --policy-version 0.11.1 --bootstrap-resamples 64 --bootstrap-seed 42
python3 -c 'import json, pathlib, sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); assert r["protocol"] == "offpolicy-integrated-arms-v1"; assert r["arms"]["off"]["doubly_robust"]["estimate"] is not None; assert r["measurement"]["input_sha256"] == r["paired"]["measurement"]["input_sha256"]; assert r["paired"]["paired_reward_delta_on_minus_off"] == 0.5' "$integrated"
python3 "$root/scripts/validate_measurement_artifact.py" offpolicy "$integrated" --require-explicit-lineage
bad_schema="$directory/offpolicy-missing-field.json"
python3 -c 'import json, pathlib, sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); del r["paired"]["paired_rows"]; pathlib.Path(sys.argv[2]).write_text(json.dumps(r))' "$integrated" "$bad_schema"
if python3 "$root/scripts/validate_measurement_artifact.py" offpolicy "$bad_schema" >/dev/null 2>&1; then exit 1; fi
bad_lineage="$directory/offpolicy-bad-lineage.json"
python3 -c 'import json, pathlib, sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); r["paired"]["measurement"]["input_sha256"]["on"] = "0" * 64; pathlib.Path(sys.argv[2]).write_text(json.dumps(r))' "$integrated" "$bad_lineage"
if python3 "$root/scripts/validate_measurement_artifact.py" offpolicy "$bad_lineage" >/dev/null 2>&1; then exit 1; fi
bad_metric="$directory/similarity-bad-metric.json"
python3 -c 'import json, pathlib, sys; r=json.loads(pathlib.Path(sys.argv[1]).read_text()); r["arms"]["exact"]["coverage"] = 1.5; pathlib.Path(sys.argv[2]).write_text(json.dumps(r))' "$similarity_first" "$bad_metric"
if python3 "$root/scripts/validate_measurement_artifact.py" similarity "$bad_metric" >/dev/null 2>&1; then exit 1; fi
echo "measurement smoke: ok (similarity arms + paired OPE audit)"
