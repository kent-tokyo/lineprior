#!/usr/bin/env python3
"""Measure exact/similarity/no-prior arms on a held-out JSONL query set.

The caller owns neighbor retrieval. This script only applies lineprior's
documented distance weighting to already supplied neighbors and never invents
an action. It is intentionally dependency-free so the same artifact can run
in a CI or data-analysis environment.
"""
import argparse, json, math, pathlib, time

def load_book(path):
    book = {}
    for line_no, line in enumerate(pathlib.Path(path).read_text().splitlines(), 1):
        if not line.strip(): continue
        row = json.loads(line)
        if "build_config_fingerprint" in row: continue
        book[row["state"]] = row.get("actions", [])
    return book

def load_queries(path):
    rows = []
    for line_no, line in enumerate(pathlib.Path(path).read_text().splitlines(), 1):
        if not line.strip(): continue
        row = json.loads(line)
        for key in ("query_id", "state", "expected_action", "neighbors"):
            if key not in row: raise ValueError(f"queries line {line_no}: missing {key}")
        rows.append(row)
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

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("prior"); ap.add_argument("queries"); ap.add_argument("--out", required=True)
    ap.add_argument("--distance-scale", type=float, default=1.0)
    ap.add_argument("--max-neighbors", type=int); ap.add_argument("--max-distance", type=float)
    args = ap.parse_args()
    if not math.isfinite(args.distance_scale) or args.distance_scale <= 0: raise SystemExit("distance-scale must be finite and > 0")
    book, queries = load_book(args.prior), load_queries(args.queries)
    arms = {"exact": [], "similarity": [], "no_prior": []}
    for arm in arms:
        for row in queries:
            start = time.perf_counter_ns()
            candidates = [] if arm == "no_prior" else (book.get(row["state"], []) if arm == "exact" else similarity(book, row["neighbors"], args.distance_scale, args.max_neighbors, args.max_distance))
            rank, hit, confidence = rank_metrics(candidates, row["expected_action"])
            arms[arm].append({"rank": rank, "hit": hit, "confidence": confidence,
                              "latency_us": (time.perf_counter_ns() - start) / 1000.0})
    report = {"protocol": "similarity-real-data-v1", "num_queries": len(queries), "arms": {}}
    for name, rows in arms.items():
        evaluated = [r for r in rows if r["rank"] is not None]
        hits = sum(r["hit"] for r in rows)
        brier = [((r["confidence"] - r["hit"]) ** 2) for r in evaluated]
        report["arms"][name] = {"coverage": len(evaluated) / len(rows) if rows else None,
            "abstention_rate": 1.0 - (len(evaluated) / len(rows)) if rows else None,
            "top1_hit_rate": hits / len(rows) if rows else None,
            "mrr": sum(1.0 / r["rank"] if r["rank"] else 0.0 for r in rows) / len(rows) if rows else None,
            "calibration_brier": sum(brier) / len(brier) if brier else None,
            "latency_us_p50": percentile([r["latency_us"] for r in rows], .50),
            "latency_us_p95": percentile([r["latency_us"] for r in rows], .95),
            "peak_rss_kb": __import__("resource").getrusage(__import__("resource").RUSAGE_SELF).ru_maxrss}
    pathlib.Path(args.out).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

if __name__ == "__main__":
    try: main()
    except (OSError, ValueError, json.JSONDecodeError) as e: raise SystemExit(f"measurement error: {e}")
