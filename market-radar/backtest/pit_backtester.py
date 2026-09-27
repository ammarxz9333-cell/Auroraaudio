#!/usr/bin/env python3
"""Point-in-time backtest harness for Market Radar.

Two-phase design:
  freeze  -> validate a snapshot, produce a decision record + SHA256 lock.
  reveal  -> attach future outcome to an already frozen decision and score it.

The freeze command refuses fields whose names suggest future/outcome leakage.
Stdlib only.
"""
from __future__ import annotations
import argparse, datetime as dt, hashlib, json
from pathlib import Path

ALLOWED_DECISIONS={"EARLY","WATCH","WATCH_LOW_CONFIDENCE","WATCH_FLOW","WATCH_FINANCING_RISK","GAP_HOLD","LATE","LATE_ARBITRAGE","BEARISH_AVOID","NO_TRADE"}
FORBIDDEN_TOKENS=("outcome","future","forward","next_day","next_session","after_cutoff","return_1","return_3","return_5","max_future","min_future")

def load(p): return json.loads(Path(p).read_text(encoding="utf-8"))
def dump_canon(x): return json.dumps(x,sort_keys=True,separators=(",",":"),ensure_ascii=False)
def parse_ts(s):
    x=dt.datetime.fromisoformat(s.replace("Z","+00:00"))
    if x.tzinfo is None: raise ValueError("timestamps must include timezone")
    return x

def walk(obj,path=""):
    if isinstance(obj,dict):
        for k,v in obj.items():
            low=k.lower()
            if any(t in low for t in FORBIDDEN_TOKENS):
                raise ValueError(f"future-looking field forbidden at freeze: {path+k}")
            yield from walk(v,path+k+".")
    elif isinstance(obj,list):
        for i,v in enumerate(obj): yield from walk(v,path+str(i)+".")

def freeze(snapshot,decision,reason):
    cutoff=parse_ts(snapshot["cutoff"])
    for e in snapshot.get("evidence",[]):
        if parse_ts(e["available_at"])>cutoff:
            raise ValueError(f"evidence after cutoff: {e.get('id','?')}")
    market_time=snapshot.get("market",{}).get("bar_time_utc")
    if market_time and parse_ts(market_time)>cutoff:
        raise ValueError("market bar after cutoff")
    list(walk(snapshot))
    if decision not in ALLOWED_DECISIONS: raise ValueError("unknown decision")
    core={"schema_version":1,"ticker":snapshot["ticker"],"cutoff":snapshot["cutoff"],"snapshot":snapshot,"decision":decision,"reason":reason}
    lock=hashlib.sha256(dump_canon(core).encode()).hexdigest()
    return {**core,"decision_lock_sha256":lock}

def verify(record):
    core={k:record[k] for k in ("schema_version","ticker","cutoff","snapshot","decision","reason")}
    return hashlib.sha256(dump_canon(core).encode()).hexdigest()==record["decision_lock_sha256"]

def classify_outcome(decision,ret):
    bullish=decision in {"EARLY","WATCH","WATCH_LOW_CONFIDENCE","WATCH_FLOW","GAP_HOLD"}
    bearish=decision=="BEARISH_AVOID"
    neutral=decision in {"NO_TRADE","LATE","LATE_ARBITRAGE","WATCH_FINANCING_RISK"}
    if bullish: return "correct" if ret>0 else "wrong"
    if bearish: return "correct" if ret<0 else "wrong"
    return "neutral"

def reveal(record,outcome):
    if not verify(record): raise ValueError("decision lock mismatch; frozen record was modified")
    if parse_ts(outcome["measured_at"])<=parse_ts(record["cutoff"]):
        raise ValueError("outcome must be measured after cutoff")
    ret=float(outcome["return_pct"])
    return {**record,"outcome":outcome,"grade":classify_outcome(record["decision"],ret)}

def metrics(records):
    graded=[r for r in records if r.get("grade") in ("correct","wrong")]
    c=sum(r["grade"]=="correct" for r in graded); w=len(graded)-c
    return {"graded_directional":len(graded),"correct":c,"wrong":w,"directional_accuracy_pct":round(100*c/len(graded),2) if graded else None,
            "neutral_or_ungraded":sum(r.get("grade")=="neutral" for r in records)}

def main():
    ap=argparse.ArgumentParser(); sub=ap.add_subparsers(dest="cmd",required=True)
    a=sub.add_parser("freeze"); a.add_argument("snapshot"); a.add_argument("--decision",required=True); a.add_argument("--reason",required=True); a.add_argument("--out",required=True)
    b=sub.add_parser("reveal"); b.add_argument("record"); b.add_argument("outcome"); b.add_argument("--out",required=True)
    c=sub.add_parser("metrics"); c.add_argument("records",nargs="+")
    args=ap.parse_args()
    if args.cmd=="freeze":
        r=freeze(load(args.snapshot),args.decision,args.reason); Path(args.out).write_text(json.dumps(r,indent=2,ensure_ascii=False)+"\n",encoding="utf-8"); print(r["decision_lock_sha256"])
    elif args.cmd=="reveal":
        r=reveal(load(args.record),load(args.outcome)); Path(args.out).write_text(json.dumps(r,indent=2,ensure_ascii=False)+"\n",encoding="utf-8"); print(r["grade"])
    else:
        rs=[load(p) for p in args.records]
        for r in rs:
            if not verify(r): raise ValueError(f"bad lock: {r.get('ticker')}")
        print(json.dumps(metrics(rs),indent=2))
if __name__=="__main__": main()
