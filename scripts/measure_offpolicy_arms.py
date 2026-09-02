#!/usr/bin/env python3
"""Run Rust IPS/DR evaluation for paired arms and combine the audit artifact."""
import argparse, hashlib, json, pathlib, subprocess, tempfile


def sha256_file(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("off"); ap.add_argument("on"); ap.add_argument("--lineprior-bin", required=True)
    ap.add_argument("--out", required=True); ap.add_argument("--dataset-id", default="unspecified")
    ap.add_argument("--split", default="unspecified"); ap.add_argument("--policy-version")
    ap.add_argument("--bootstrap-resamples", type=int, default=2000); ap.add_argument("--bootstrap-seed", type=int, default=42)
    ap.add_argument("--confidence-level", type=float, default=.95); ap.add_argument("--max-importance-weight", type=float)
    args = ap.parse_args()
    root = pathlib.Path(__file__).resolve().parent
    compare = root / "compare_offpolicy_arms.py"
    with tempfile.TemporaryDirectory(prefix="lineprior-offpolicy-arms-") as directory:
        directory = pathlib.Path(directory)
        arm_reports = {}
        for name, source in (("off", args.off), ("on", args.on)):
            target = directory / f"{name}.json"
            command = [args.lineprior_bin, "offpolicy", source, "--out", str(target),
                       "--policy-name", name, "--doubly-robust",
                       "--bootstrap-resamples", str(args.bootstrap_resamples),
                       "--bootstrap-seed", str(args.bootstrap_seed), "--confidence-level", str(args.confidence_level)]
            if args.policy_version: command += ["--policy-version", args.policy_version]
            if args.max_importance_weight is not None: command += ["--max-importance-weight", str(args.max_importance_weight)]
            subprocess.run(command, check=True, capture_output=True, text=True)
            arm_reports[name] = json.loads(target.read_text())
        paired_path = directory / "paired.json"
        subprocess.run(["python3", str(compare), args.off, args.on, "--out", str(paired_path),
                        "--bootstrap-resamples", str(args.bootstrap_resamples),
                        "--bootstrap-seed", str(args.bootstrap_seed),
                        "--confidence-level", str(args.confidence_level)], check=True)
        report = {"protocol": "offpolicy-integrated-arms-v1",
                  "measurement": {"dataset_id": args.dataset_id, "split": args.split,
                                  "lineprior_version": args.policy_version or "0.11.1",
                                  "policy_version": args.policy_version or "unspecified",
                                  "input_sha256": {"off": sha256_file(args.off),
                                                   "on": sha256_file(args.on)}},
                  "arms": arm_reports, "paired": json.loads(paired_path.read_text())}
        pathlib.Path(args.out).write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")

if __name__ == "__main__":
    try: main()
    except (OSError, ValueError, subprocess.CalledProcessError, json.JSONDecodeError) as e:
        raise SystemExit(f"measurement error: {e}")
