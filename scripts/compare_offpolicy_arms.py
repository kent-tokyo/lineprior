#!/usr/bin/env python3
"""Pair on/off downstream logs and audit propensity support.

Run the Rust `lineprior offpolicy` command separately on each arm for IPS/DR
and bootstrap estimates. This helper checks the pairing contract and reports
the observed paired reward delta plus deterministic uncertainty over that
paired delta; it never fabricates counterfactual rewards.
"""
import argparse, json, math, pathlib

def load(path):
    rows = {}
    for no, line in enumerate(pathlib.Path(path).read_text().splitlines(), 1):
        if not line.strip(): continue
        row = json.loads(line)
        key = row.get("row_id", row.get("metadata", {}).get("row_id"))
        if not key: raise ValueError(f"{path}:{no}: missing row_id")
        if key in rows: raise ValueError(f"{path}:{no}: duplicate row_id {key}")
        reward = row.get("reward")
        if not isinstance(reward, (int, float)) or not math.isfinite(reward):
            raise ValueError(f"{path}:{no}: reward must be finite")
        rows[key] = row
    return rows

def propensity_audit(rows):
    supported = 0; failures = 0; weights = []
    for row in rows.values():
        lp, ep = row.get("logging_propensity"), row.get("evaluation_probability")
        valid = isinstance(lp, (int, float)) and math.isfinite(lp) and lp > 0
        valid = valid and isinstance(ep, (int, float)) and math.isfinite(ep) and 0 <= ep <= 1
        if not valid or ep == 0:
            failures += 1; continue
        supported += 1; weights.append(ep / lp)
    ess = (sum(weights) ** 2 / sum(w * w for w in weights)) if weights and sum(w * w for w in weights) else None
    return {"rows": len(rows), "supported_rows": supported, "overlap_failures": failures,
            "support_fraction": supported / len(rows) if rows else None,
            "effective_sample_size": ess, "max_importance_weight": max(weights) if weights else None}

def rng(seed):
    while True:
        seed = (6364136223846793005 * seed + 1442695040888963407) & ((1 << 64) - 1)
        yield seed

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("off"); ap.add_argument("on"); ap.add_argument("--out", required=True)
    ap.add_argument("--bootstrap-resamples", type=int, default=2000); ap.add_argument("--bootstrap-seed", type=int, default=42)
    ap.add_argument("--confidence-level", type=float, default=.95)
    ap.add_argument("--dataset-id", default="unspecified"); ap.add_argument("--split", default="unspecified")
    ap.add_argument("--lineprior-version", default="0.11.1")
    args = ap.parse_args()
    if args.bootstrap_resamples <= 0 or not 0 < args.confidence_level < 1: raise SystemExit("invalid bootstrap controls")
    off, on = load(args.off), load(args.on)
    if set(off) != set(on): raise SystemExit("off/on row_id sets differ; paired comparison aborted")
    ids = sorted(off); deltas = [float(on[k]["reward"]) - float(off[k]["reward"]) for k in ids]
    gen = rng(args.bootstrap_seed); samples = []
    for _ in range(args.bootstrap_resamples):
        sample = [deltas[next(gen) % len(deltas)] for _ in deltas] if deltas else []
        if sample: samples.append(sum(sample) / len(sample))
    samples.sort()
    alpha = (1 - args.confidence_level) / 2
    report = {"protocol": "offpolicy-paired-arms-v1", "paired_rows": len(ids),
      "measurement": {"dataset_id": args.dataset_id, "split": args.split,
                       "lineprior_version": args.lineprior_version},
      "off": propensity_audit(off), "on": propensity_audit(on),
      "observed_reward_mean": {"off": sum(float(off[k]["reward"]) for k in ids) / len(ids) if ids else None,
                                "on": sum(float(on[k]["reward"]) for k in ids) / len(ids) if ids else None},
      "paired_reward_delta_on_minus_off": sum(deltas) / len(deltas) if deltas else None,
      "bootstrap": {"seed": args.bootstrap_seed, "resamples": args.bootstrap_resamples,
        "confidence_level": args.confidence_level,
        "lower": samples[math.floor(alpha * (len(samples)-1))] if samples else None,
        "upper": samples[math.ceil((1-alpha) * (len(samples)-1))] if samples else None}}
    pathlib.Path(args.out).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

if __name__ == "__main__":
    try: main()
    except (OSError, ValueError, json.JSONDecodeError) as e: raise SystemExit(f"measurement error: {e}")
