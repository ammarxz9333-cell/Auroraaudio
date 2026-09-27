#!/usr/bin/env python3
"""Create deterministic, leakage-safe cohort manifests for Market Radar research.

This tool does not label outcomes and does not tune a gate. It partitions
pre-cutoff candidate snapshots deterministically so development and validation
cannot silently exchange cases.
"""
from __future__ import annotations
import argparse, hashlib, json
from pathlib import Path

def bucket(ticker: str, cutoff: str) -> int:
    h=hashlib.sha256(f"{ticker.upper()}|{cutoff}".encode()).hexdigest()
    return int(h[:8],16)%100

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("dataset")
    ap.add_argument("--out-dir",required=True)
    ap.add_argument("--train-pct",type=int,default=60)
    ap.add_argument("--validation-pct",type=int,default=20)
    a=ap.parse_args()
    if a.train_pct+a.validation_pct>=100: raise SystemExit("leave a positive final-test partition")
    d=json.loads(Path(a.dataset).read_text())
    rows=d.get("snapshots",[])
    parts={"development":[],"validation":[],"final_test":[]}
    for r in rows:
        b=bucket(r["ticker"],r["cutoff"])
        k="development" if b<a.train_pct else "validation" if b<a.train_pct+a.validation_pct else "final_test"
        parts[k].append({"ticker":r["ticker"],"cutoff":r["cutoff"]})
    out=Path(a.out_dir); out.mkdir(parents=True,exist_ok=True)
    for k,v in parts.items():
        (out/f"{k}.json").write_text(json.dumps({"schema_version":1,"partition":k,"count":len(v),"cases":v},indent=2)+"\n")
    print(json.dumps({k:len(v) for k,v in parts.items()}))
if __name__=="__main__": main()
