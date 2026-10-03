#!/usr/bin/env python3
"""Measure exact/similarity/no-prior arms on a held-out JSONL query set.

The caller owns neighbor retrieval. This script only applies lineprior's
documented distance weighting to already supplied neighbors and never invents
an action. It is intentionally dependency-free so the same artifact can run
in a CI or data-analysis environment.
"""
import argparse, hashlib, json, math, pathlib, platform, resource, subprocess, sys, time

from version_contract import workspace_version


def sha256_file(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()

def repository_relative(path):
    root = pathlib.Path(__file__).resolve().parent.parent
    return pathlib.Path(path).resolve().relative_to(root).as_posix()

def load_book(path):
    book = {}
    fingerprint = "unspecified"
    for line_no, line in enumerate(pathlib.Path(path).read_text().splitlines(), 1):
        if not line.strip(): continue
        row = json.loads(line)
        if "build_config_fingerprint" in row:
            candidate = row["build_config_fingerprint"]
            if fingerprint != "unspecified" and fingerprint != candidate:
                raise ValueError("prior contains conflicting build_config_fingerprint headers")
            fingerprint = candidate
            continue
        book[row["state"]] = row.get("actions", [])
    return book, fingerprint

def load_queries(path):
    rows = []
    query_ids = set()
    for line_no, line in enumerate(pathlib.Path(path).read_text().splitlines(), 1):
        if not line.strip(): continue
        row = json.loads(line)
        for key in ("query_id", "state", "expected_action", "neighbors"):
            if key not in row: raise ValueError(f"queries line {line_no}: missing {key}")
        if any(not isinstance(row[key], str) or not row[key].strip()
               for key in ("query_id", "state", "expected_action")):
            raise ValueError(f"queries line {line_no}: query_id, state, and expected_action must be non-empty strings")
        if row["query_id"] in query_ids:
            raise ValueError(f"queries line {line_no}: duplicate query_id")
        query_ids.add(row["query_id"])
        if not isinstance(row["neighbors"], list):
            raise ValueError(f"queries line {line_no}: neighbors must be a list")
        for neighbor_no, neighbor in enumerate(row["neighbors"], 1):
            if not isinstance(neighbor, dict) or not isinstance(neighbor.get("state"), str) or not neighbor["state"].strip():
                raise ValueError(f"queries line {line_no}: neighbor {neighbor_no} has invalid state")
            distance = neighbor.get("distance")
            if isinstance(distance, bool) or not isinstance(distance, (int, float)) or not math.isfinite(distance) or distance < 0:
                raise ValueError(f"queries line {line_no}: neighbor {neighbor_no} has invalid distance")
            if "provenance" in neighbor and (not isinstance(neighbor["provenance"], str) or not neighbor["provenance"].strip()):
                raise ValueError(f"queries line {line_no}: neighbor {neighbor_no} has invalid provenance")
        rows.append(row)
    if not rows:
        raise ValueError("queries contain no usable rows")
    return rows

def rank_metrics(candidates, expected):
    if not candidates: return None, 0.0, None
    confidence = float(candidates[0].get("confidence", 0.0))
    rank = next((i + 1 for i, x in enumerate(candidates) if x.get("action") == expected), None)
    return rank, 1.0 if rank == 1 else 0.0, confidence

def similarity(book, neighbors, scale, max_neighbors, max_distance):
    selected = [n for n in neighbors if math.isfinite(n.get("distance", math.nan)) and n["distance"] >= 0
                and (max_distance is None or n["distance"] <= max_distance)]
    selected.sort(key=lambda n: (n["distance"], n["state"], n.get("provenance", "")))
    if max_neighbors is not None: selected = selected[:max_neighbors]
    agg = {}
    for n in selected:
        if n.get("state") not in book:
            continue
        weight = math.exp(-n["distance"] / scale)
        for action in book[n["state"]]:
            key = action["action"]
            item = agg.setdefault(key, [0.0, 0.0, 0.0])
            item[0] += weight * float(action.get("prior", 0.0))
            item[1] += weight * float(action.get("confidence", 0.0))
            item[2] += weight
    result = [{"action": key, "prior": value[0] / value[2],
               "confidence": value[1] / value[2]} for key, value in agg.items()]
    total = sum(x["prior"] for x in result)
    if total > 0: 
        for x in result: x["prior"] /= total
    result.sort(key=lambda x: (-x["prior"], x["action"]))
    return result

def percentile(values, p):
    if not values: return None
    values = sorted(values); pos = (len(values) - 1) * p
    lo, hi = math.floor(pos), math.ceil(pos)
    return values[lo] if lo == hi else values[lo] + (values[hi] - values[lo]) * (pos - lo)

def normalize_peak_rss_kb(raw_value, system):
    # getrusage reports bytes on macOS and KiB on Linux/BSD. Normalize the
    # artifact instead of silently attaching the wrong unit to macOS values.
    return raw_value / 1024.0 if system == "Darwin" else float(raw_value)

def resolve_fingerprint(header_value, override):
    if override is None:
        return header_value
    if header_value != "unspecified" and str(header_value) != str(override):
        raise ValueError("--prior-config-fingerprint conflicts with the prior header")
    return override

def git_lineage():
    root = pathlib.Path(__file__).resolve().parent.parent
    try:
        commit = subprocess.check_output(
            ["git", "rev-parse", "HEAD"],
            cwd=root,
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
        dirty = bool(
            subprocess.check_output(
                ["git", "status", "--porcelain"],
                cwd=root,
                text=True,
                stderr=subprocess.DEVNULL,
            ).strip()
        )
        return commit, dirty
    except (OSError, subprocess.CalledProcessError):
        return "unavailable", None

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("prior"); ap.add_argument("queries"); ap.add_argument("--out", required=True)
    ap.add_argument("--distance-scale", type=float, default=1.0)
    ap.add_argument("--max-neighbors", type=int); ap.add_argument("--max-distance", type=float)
    ap.add_argument("--warmup", type=int, default=1)
    ap.add_argument("--repetitions", type=int, default=5)
    ap.add_argument("--dataset-id", default="unspecified"); ap.add_argument("--split", default="unspecified")
    ap.add_argument("--feature-version", default="unspecified"); ap.add_argument("--lineprior-version", default=workspace_version())
    ap.add_argument("--prior-config-fingerprint")
    ap.add_argument("--dataset-source-commit", default="unspecified")
    ap.add_argument("--seed", default="none")
    ap.add_argument("--feature-adapter")
    args = ap.parse_args()
    if not math.isfinite(args.distance_scale) or args.distance_scale <= 0: raise SystemExit("distance-scale must be finite and > 0")
    if args.warmup < 0: raise SystemExit("warmup must be >= 0")
    if args.repetitions <= 0: raise SystemExit("repetitions must be > 0")
    book, prior_config_fingerprint = load_book(args.prior)
    prior_config_fingerprint = resolve_fingerprint(prior_config_fingerprint, args.prior_config_fingerprint)
    queries = load_queries(args.queries)
    runner_commit, runner_tree_dirty = git_lineage()
    arms = {"exact": [], "similarity": [], "no_prior": []}
    for arm in arms:
        for row in queries:
            def run_arm():
                return [] if arm == "no_prior" else (book.get(row["state"], []) if arm == "exact" else similarity(book, row["neighbors"], args.distance_scale, args.max_neighbors, args.max_distance))
            for _ in range(args.warmup):
                run_arm()
            candidates = None
            latencies = []
            for _ in range(args.repetitions):
                start = time.perf_counter_ns()
                current = run_arm()
                latencies.append((time.perf_counter_ns() - start) / 1000.0)
                if candidates is None:
                    candidates = current
                elif current != candidates:
                    raise ValueError(f"{arm} produced non-deterministic candidates")
            rank, hit, confidence = rank_metrics(candidates, row["expected_action"])
            arms[arm].append({"rank": rank, "hit": hit, "confidence": confidence,
                              "latency_us": latencies})
    report = {"protocol": "similarity-real-data-v1", "num_queries": len(queries),
              "measurement": {"dataset_id": args.dataset_id, "split": args.split,
                               "feature_version": args.feature_version,
                               "lineprior_version": args.lineprior_version,
                               "runner_commit": runner_commit,
                               "runner_tree_dirty": runner_tree_dirty,
                               "runner_path": repository_relative(__file__),
                               "runner_sha256": sha256_file(__file__),
                               "feature_adapter_path": repository_relative(args.feature_adapter) if args.feature_adapter else None,
                               "feature_adapter_sha256": sha256_file(args.feature_adapter) if args.feature_adapter else None,
                               "dataset_source_commit": args.dataset_source_commit,
                               "seed": args.seed,
                               "environment": {"python": sys.version.split()[0],
                                               "platform": platform.platform(),
                                               "machine": platform.machine()},
                               "similarity_config": {"distance_scale": args.distance_scale,
                                                     "max_neighbors": args.max_neighbors,
                                                     "max_distance": args.max_distance},
                               "timing": {"warmup": args.warmup,
                                          "repetitions": args.repetitions},
                               "input_sha256": {"prior": sha256_file(args.prior),
                                                "queries": sha256_file(args.queries)},
                               "prior_config_fingerprint": prior_config_fingerprint}, "arms": {}}
    for name, rows in arms.items():
        evaluated = [r for r in rows if r["rank"] is not None]
        hits = sum(r["hit"] for r in rows)
        brier = [((r["confidence"] - r["hit"]) ** 2) for r in evaluated]
        report["arms"][name] = {"coverage": len(evaluated) / len(rows) if rows else None,
            "abstention_rate": 1.0 - (len(evaluated) / len(rows)) if rows else None,
            "evaluated_queries": len(evaluated),
            "top1_hits": int(hits),
            "top1_hit_rate": hits / len(rows) if rows else None,
            "top1_hit_rate_covered": hits / len(evaluated) if evaluated else None,
            "false_recommendation_rate_covered": 1.0 - (hits / len(evaluated)) if evaluated else None,
            "mrr": sum(1.0 / r["rank"] if r["rank"] else 0.0 for r in rows) / len(rows) if rows else None,
            "calibration_brier": sum(brier) / len(brier) if brier else None,
            "latency_us_p50": percentile([value for r in rows for value in r["latency_us"]], .50),
            "latency_us_p95": percentile([value for r in rows for value in r["latency_us"]], .95),
            "peak_rss_kb": normalize_peak_rss_kb(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss, platform.system())}
    pathlib.Path(args.out).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

if __name__ == "__main__":
    try: main()
    except (OSError, ValueError, json.JSONDecodeError) as e: raise SystemExit(f"measurement error: {e}")
