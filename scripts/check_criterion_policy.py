#!/usr/bin/env python3
"""Evaluate selected Criterion estimates against Aurora's versioned CI policy."""

from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path
import sys


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--criterion-root", type=Path, default=Path("target/criterion"))
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def load_json(path: Path) -> object:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def main() -> int:
    args = parse_args()
    policy = load_json(args.policy)
    if not isinstance(policy, dict) or policy.get("schema_version") != 1:
        raise SystemExit("unsupported Criterion policy schema; expected schema_version=1")

    sample_rate = int(policy["sample_rate_hz"])
    block_size = int(policy["block_size_frames"])
    if sample_rate <= 0 or block_size <= 0:
        raise SystemExit("policy sample rate and block size must be positive")
    block_budget_ns = block_size / sample_rate * 1_000_000_000.0

    results = []
    passed = True
    for benchmark in policy.get("benchmarks", []):
        benchmark_id = str(benchmark["id"])
        max_ns = float(benchmark["max_ns"])
        estimates_path = args.criterion_root / benchmark_id / "new" / "estimates.json"
        entry = {
            "id": benchmark_id,
            "max_ns": max_ns,
            "max_us": max_ns / 1_000.0,
            "criterion_estimates": str(estimates_path),
            "rationale": benchmark.get("rationale", ""),
        }
        try:
            estimates = load_json(estimates_path)
            measured_ns = float(estimates["mean"]["point_estimate"])
            if not math.isfinite(measured_ns) or measured_ns < 0.0:
                raise ValueError("mean point estimate is not finite and non-negative")
            entry.update(
                {
                    "measured_ns": measured_ns,
                    "measured_us": measured_ns / 1_000.0,
                    "block_budget_percent": measured_ns / block_budget_ns * 100.0,
                    "limit_percent": measured_ns / max_ns * 100.0,
                    "passed": measured_ns <= max_ns,
                }
            )
        except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError) as error:
            entry.update({"passed": False, "error": str(error)})
        passed = passed and bool(entry["passed"])
        results.append(entry)

    summary = {
        "schema_version": 1,
        "artifact": "criterion-regression-policy-summary",
        "passed": passed,
        "commit_sha": os.environ.get("GITHUB_SHA", "unknown"),
        "policy": str(args.policy),
        "metric": policy.get("metric", "mean.point_estimate_ns"),
        "sample_rate_hz": sample_rate,
        "block_size_frames": block_size,
        "block_budget_ns": block_budget_ns,
        "results": results,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    for result in results:
        if result.get("passed"):
            print(
                f"PASS {result['id']}: {result['measured_us']:.3f} us "
                f"<= {result['max_us']:.3f} us "
                f"({result['block_budget_percent']:.2f}% of block budget)"
            )
        else:
            reason = result.get("error") or (
                f"{result.get('measured_us', float('nan')):.3f} us > {result['max_us']:.3f} us"
            )
            print(f"FAIL {result['id']}: {reason}", file=sys.stderr)

    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
