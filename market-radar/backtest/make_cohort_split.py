#!/usr/bin/env python3
"""Chronological cohorts with an embargo around model-selection boundaries."""
from __future__ import annotations
import argparse, datetime as dt, json
from pathlib import Path

def chronological_split(rows, train_pct=60, validation_pct=20, embargo_days=7):
    if not 0 < train_pct < 100 or not 0 <= validation_pct < 100 or train_pct + validation_pct >= 100:
        raise ValueError("leave positive development and final-test partitions")
    days = sorted({dt.datetime.fromisoformat(r["cutoff"].replace("Z", "+00:00")).date() for r in rows})
    if not days:
        return {k: [] for k in ("development", "validation", "final_test")}, {}
    first_boundary = days[max(0, int(len(days) * train_pct / 100) - 1)]
    second_boundary = days[max(0, int(len(days) * (train_pct + validation_pct) / 100) - 1)]
    embargo = dt.timedelta(days=embargo_days)
    parts = {k: [] for k in ("development", "validation", "final_test")}
    for row in sorted(rows, key=lambda r: (r["cutoff"], r["ticker"])):
        day = dt.datetime.fromisoformat(row["cutoff"].replace("Z", "+00:00")).date()
        if day <= first_boundary:
            part = "development"
        elif day <= first_boundary + embargo:
            continue
        elif day <= second_boundary:
            part = "validation"
        elif day <= second_boundary + embargo:
            continue
        else:
            part = "final_test"
        parts[part].append({"ticker": row["ticker"], "cutoff": row["cutoff"]})
    return parts, {"development_end": str(first_boundary), "validation_end": str(second_boundary), "embargo_calendar_days": embargo_days}

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("dataset")
    ap.add_argument("--out-dir",required=True)
    ap.add_argument("--train-pct",type=int,default=60)
    ap.add_argument("--validation-pct",type=int,default=20)
    ap.add_argument("--embargo-days",type=int,default=7)
    a=ap.parse_args()
    if a.embargo_days < 0: raise SystemExit("embargo must be nonnegative")
    d=json.loads(Path(a.dataset).read_text())
    rows=d.get("snapshots",[])
    parts,boundaries=chronological_split(rows,a.train_pct,a.validation_pct,a.embargo_days)
    out=Path(a.out_dir); out.mkdir(parents=True,exist_ok=True)
    for k,v in parts.items():
        (out/f"{k}.json").write_text(json.dumps({"schema_version":2,"partition":k,"boundaries":boundaries,"count":len(v),"cases":v},indent=2)+"\n")
    print(json.dumps({"counts":{k:len(v) for k,v in parts.items()},"boundaries":boundaries,"adequate_for_model_selection":all(parts.values())}))
if __name__=="__main__": main()
