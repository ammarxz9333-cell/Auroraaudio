#!/usr/bin/env python3
"""Persistent learning memory for the 12-seat Market Radar council.

Stdlib-only by design so it can run in GitHub Actions. It stores immutable
council predictions, settles matured horizons, computes proper scoring rules,
and derives conservative reliability weights. LLM reasoning never mutates its
own score directly; only realized market outcomes do.
"""
from __future__ import annotations
import argparse, datetime as dt, json, math, statistics, uuid
from pathlib import Path

ROOT=Path(__file__).resolve().parent
LEARN=ROOT/"learning"
LEDGER=LEARN/"council-predictions.json"
STATE=LEARN/"council-state.json"
HORIZONS={"1w":7,"3m":91,"1y":365}
AGENTS=("price_chart","financials","analyst_targets","earnings_estimates","insider_trades",
"congress_trades","news","sec_filings","institutional_holdings","options_market","economy","related_markets")

def _load(p, fallback):
    try:return json.loads(p.read_text(encoding="utf-8"))
    except Exception:return fallback
def _save(p,v):
    p.parent.mkdir(parents=True,exist_ok=True); p.write_text(json.dumps(v,indent=2,sort_keys=True)+"\n",encoding="utf-8")
def _clip(p): return min(.999,max(.001,float(p)))
def brier(p,y): return (_clip(p)-float(y))**2
def logloss(p,y):
    p=_clip(p); return -(y*math.log(p)+(1-y)*math.log(1-p))
def iso(v):
    x=dt.datetime.fromisoformat(str(v).replace("Z","+00:00"))
    if x.tzinfo is None:x=x.replace(tzinfo=dt.timezone.utc)
    return x.astimezone(dt.timezone.utc)

def validate_prediction(x):
    if not x.get("ticker") or float(x.get("price",0))<=0: raise ValueError("ticker and positive price required")
    seats=x.get("agents") or {}
    missing=[a for a in AGENTS if a not in seats]
    if missing: raise ValueError("missing agents: "+",".join(missing))
    for a in AGENTS:
        for h in HORIZONS:
            p=(seats[a].get("p_up") or {}).get(h)
            if p is not None and not 0<=float(p)<=1: raise ValueError(f"{a}.{h} probability outside [0,1]")
    return x

def record(x):
    validate_prediction(x); rows=_load(LEDGER,[])
    row=dict(x); row.setdefault("id",str(uuid.uuid4())); row.setdefault("created_utc",dt.datetime.now(dt.timezone.utc).isoformat())
    row.setdefault("outcomes",{}); rows.append(row); _save(LEDGER,rows); return row

def yahoo_daily_close(ticker, target_date):
    """Return first available Yahoo daily close on/after target_date."""
    import urllib.parse, urllib.request
    start=dt.datetime.combine(target_date,dt.time.min,tzinfo=dt.timezone.utc)
    end=start+dt.timedelta(days=8)
    url=("https://query1.finance.yahoo.com/v8/finance/chart/"+urllib.parse.quote(ticker)+
         "?interval=1d&period1="+str(int(start.timestamp()))+"&period2="+str(int(end.timestamp()))+
         "&events=div%2Csplits")
    req=urllib.request.Request(url,headers={"User-Agent":"Aurora-Council-Learner/1.0","Accept":"application/json"})
    with urllib.request.urlopen(req,timeout=15) as resp: data=json.loads(resp.read().decode())
    r=((data.get("chart") or {}).get("result") or [None])[0]
    if not r:return None
    stamps=r.get("timestamp") or []; closes=(((r.get("indicators") or {}).get("quote") or [{}])[0].get("close") or [])
    for ts,close in zip(stamps,closes):
        if close is not None and dt.datetime.fromtimestamp(ts,dt.timezone.utc).date()>=target_date:return float(close)
    return None

def auto_settle(now=None):
    """Settle every matured unsolved horizon from point-in-time daily closes."""
    now=now or dt.datetime.now(dt.timezone.utc); rows=_load(LEDGER,[]); changed=0
    for r in rows:
        created=iso(r["created_utc"]); ticker=r["ticker"]
        for h,days in HORIZONS.items():
            if h in (r.get("outcomes") or {}):continue
            target=(created+dt.timedelta(days=days)).date()
            if now.date()<target:continue
            try: px=yahoo_daily_close(ticker,target)
            except Exception: px=None
            if px is None:continue
            entry=float(r["price"]); ret=px/entry-1; y=1 if ret>0 else 0
            r.setdefault("outcomes",{})[h]={"exit_price":px,"return":ret,"up":y,
                "target_date":target.isoformat(),"settled_utc":now.isoformat(),"source":"Yahoo Finance daily close"}
            changed+=1
    if changed:_save(LEDGER,rows)
    rebuild_state(rows); return {"settled":changed,"predictions":len(rows)}

def settle(prediction_id,horizon,exit_price,settled_utc=None):
    if horizon not in HORIZONS: raise ValueError("unknown horizon")
    rows=_load(LEDGER,[]); found=None
    for r in rows:
        if r.get("id")==prediction_id:
            entry=float(r["price"]); ret=(float(exit_price)/entry)-1; y=1 if ret>0 else 0
            r.setdefault("outcomes",{})[horizon]={"exit_price":float(exit_price),"return":ret,"up":y,
              "settled_utc":settled_utc or dt.datetime.now(dt.timezone.utc).isoformat()}
            found=r; break
    if not found: raise KeyError(prediction_id)
    _save(LEDGER,rows); rebuild_state(rows); return found

def rebuild_state(rows=None,min_samples=8,shrinkage=20.0):
    rows=rows if rows is not None else _load(LEDGER,[])
    state={"generated_utc":dt.datetime.now(dt.timezone.utc).isoformat(),"min_samples_for_weighting":min_samples,
           "agents":{},"calibration":{},"lessons":[]}
    all_briers={h:[] for h in HORIZONS}
    samples={(a,h):[] for a in AGENTS for h in HORIZONS}
    buckets={h:{str(i):[] for i in range(10)} for h in HORIZONS}
    for r in rows:
        for h,out in (r.get("outcomes") or {}).items():
            if h not in HORIZONS: continue
            y=int(out["up"])
            final=((r.get("final") or {}).get("p_up") or {}).get(h)
            if final is not None:
                all_briers[h].append(brier(final,y)); buckets[h][str(min(9,int(_clip(final)*10)))].append(y)
            for a in AGENTS:
                p=((((r.get("agents") or {}).get(a) or {}).get("p_up") or {}).get(h))
                if p is not None:samples[(a,h)].append((float(p),y))
    baseline={h:(statistics.mean(v) if v else .25) for h,v in all_briers.items()}
    for a in AGENTS:
        state["agents"][a]={}
        for h in HORIZONS:
            vals=samples[(a,h)]; n=len(vals)
            bs=statistics.mean([brier(p,y) for p,y in vals]) if vals else None
            ll=statistics.mean([logloss(p,y) for p,y in vals]) if vals else None
            acc=statistics.mean([(p>=.5)==bool(y) for p,y in vals]) if vals else None
            # Conservative skill ratio shrunk toward 1.0; clamp prevents runaway self-reinforcement.
            raw=(baseline[h]/bs) if bs and bs>0 else 1.0
            trust=n/(n+shrinkage)
            weight=1.0 if n<min_samples else 1.0+trust*(raw-1.0)
            weight=max(.5,min(1.5,weight))
            state["agents"][a][h]={"n":n,"brier":bs,"log_loss":ll,"direction_accuracy":acc,"weight":weight}
    for h,bs in buckets.items():
        state["calibration"][h]={}
        for k,ys in bs.items():
            if ys: state["calibration"][h][k]={"n":len(ys),"observed_up_rate":statistics.mean(ys)}
    # Machine-readable lessons only after repeated evidence.
    for a in AGENTS:
        for h in HORIZONS:
            m=state["agents"][a][h]
            if m["n"]>=min_samples and m["weight"]<=.8:
                state["lessons"].append({"agent":a,"horizon":h,"kind":"downweight","n":m["n"],"weight":m["weight"],
                  "reason":"realized proper-score performance has been persistently weak"})
            elif m["n"]>=min_samples and m["weight"]>=1.2:
                state["lessons"].append({"agent":a,"horizon":h,"kind":"upweight","n":m["n"],"weight":m["weight"],
                  "reason":"realized proper-score performance has been persistently strong"})
    _save(STATE,state); return state

def context():
    s=rebuild_state(); return {"agent_weights":{a:{h:v["weight"] for h,v in hs.items()} for a,hs in s["agents"].items()},
      "calibration":s["calibration"],"lessons":s["lessons"]}

def main():
    ap=argparse.ArgumentParser(); sub=ap.add_subparsers(dest="cmd",required=True)
    p=sub.add_parser("record"); p.add_argument("json_file")
    q=sub.add_parser("settle"); q.add_argument("id"); q.add_argument("horizon",choices=HORIZONS); q.add_argument("exit_price",type=float)
    sub.add_parser("rebuild"); sub.add_parser("context"); sub.add_parser("auto-settle")
    a=ap.parse_args()
    if a.cmd=="record": print(json.dumps(record(json.loads(Path(a.json_file).read_text())),indent=2))
    elif a.cmd=="settle": print(json.dumps(settle(a.id,a.horizon,a.exit_price),indent=2))
    elif a.cmd=="rebuild": print(json.dumps(rebuild_state(),indent=2))
    elif a.cmd=="auto-settle": print(json.dumps(auto_settle(),indent=2))
    else: print(json.dumps(context(),indent=2))
if __name__=="__main__": main()
