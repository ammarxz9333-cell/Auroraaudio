#!/usr/bin/env python3
"""Build leakage-safe PIT cases from externally collected candidate events.

Input JSONL rows contain only event metadata known at cutoff:
ticker, cutoff, evidence[], market{}, features{}.
Outcome fields are forbidden and must live in a separate JSONL file.

This builder intentionally does NOT infer decisions from future returns.
"""
from __future__ import annotations
import argparse,json
from pathlib import Path

FORBIDDEN=("outcome","return_pct","day_1","day_5","future","follow_through","max_after","min_after")

def bad_key(o):
    if isinstance(o,dict):
        for k,v in o.items():
            if any(x in k.lower() for x in FORBIDDEN): return k
            z=bad_key(v)
            if z:return z
    if isinstance(o,list):
        for v in o:
            z=bad_key(v)
            if z:return z
    return None

def main():
    ap=argparse.ArgumentParser();ap.add_argument("input");ap.add_argument("output");a=ap.parse_args()
    rows=[]
    for n,line in enumerate(Path(a.input).read_text().splitlines(),1):
        if not line.strip():continue
        r=json.loads(line); b=bad_key(r)
        if b: raise SystemExit(f"line {n}: future field forbidden: {b}")
        for req in ("ticker","cutoff","evidence","market","features"):
            if req not in r: raise SystemExit(f"line {n}: missing {req}")
        rows.append(r)
    Path(a.output).write_text(json.dumps({"schema_version":1,"count":len(rows),"snapshots":rows},indent=2)+"\n")
    print(len(rows))
if __name__=="__main__":main()
