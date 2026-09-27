#!/usr/bin/env python3
"""Historical 5-minute replay for frozen Market Radar catalyst cases.

Reconstructs only price/volume-derived gate fields from Yahoo chart data.
Spread is not reconstructed from OHLCV; cases requiring historical bid/ask
remain INSUFFICIENT_DATA rather than receiving an invented spread.
"""
from __future__ import annotations
import datetime as dt, json, urllib.parse, urllib.request
from pathlib import Path
from zoneinfo import ZoneInfo
from entry_gate import GateInput, entry_gate

NY=ZoneInfo("America/New_York")
UA="Ammar-Market-Radar-Replay/1.0"

def yahoo_5m(ticker, start, end):
    p1=int(start.timestamp()); p2=int(end.timestamp())
    q=urllib.parse.urlencode({"period1":p1,"period2":p2,"interval":"5m","includePrePost":"true","events":"div,splits"})
    url=f"https://query2.finance.yahoo.com/v8/finance/chart/{urllib.parse.quote(ticker)}?{q}"
    req=urllib.request.Request(url,headers={"User-Agent":UA})
    with urllib.request.urlopen(req,timeout=20) as resp: data=json.load(resp)
    result=data["chart"]["result"][0]; stamps=result.get("timestamp") or []; q0=result["indicators"]["quote"][0]
    out=[]
    for i,ts in enumerate(stamps):
        vals={k:(q0.get(k) or [])[i] if i<len(q0.get(k) or []) else None for k in ("open","high","low","close","volume")}
        if vals["close"] is None: continue
        out.append({"time":dt.datetime.fromtimestamp(ts,dt.timezone.utc),"ny":dt.datetime.fromtimestamp(ts,dt.timezone.utc).astimezone(NY),**vals})
    return out

def replay_case(case):
    cutoff=dt.datetime.fromisoformat(case["cutoff"])
    day=cutoff.astimezone(NY).date()
    start=dt.datetime.combine(day-dt.timedelta(days=7),dt.time(0),NY)
    end=dt.datetime.combine(day+dt.timedelta(days=2),dt.time(0),NY)
    bars=yahoo_5m(case["ticker"],start,end)
    today=[b for b in bars if b["ny"].date()==day]
    regular=[b for b in today if dt.time(9,30)<=b["ny"].time()<dt.time(16,0)]
    pre=[b for b in today if b["ny"].time()<dt.time(9,30)]
    prior=[b for b in bars if b["ny"].date()<day and dt.time(9,30)<=b["ny"].time()<dt.time(16,0)]
    if not regular or not prior: return {**case,"replay_state":"INSUFFICIENT_DATA","reason":"missing bars/prior close"}
    prior_day=max(b["ny"].date() for b in prior); prev=[b for b in prior if b["ny"].date()==prior_day]
    prev_close=float(prev[-1]["close"]); reg_open=float(regular[0]["open"] or regular[0]["close"])
    pre_reprice=((float(pre[-1]["close"])/prev_close)-1)*100 if pre else None
    cum_pv=0.0; cum_v=0.0; out=[]
    for idx,b in enumerate(regular):
        if b["time"] < cutoff.astimezone(dt.timezone.utc): continue
        vol=float(b["volume"] or 0); typical=(float(b["high"])+float(b["low"])+float(b["close"]))/3
        # VWAP must include all regular bars through this timestamp.
        hist=regular[:idx+1]; cv=sum(float(x["volume"] or 0) for x in hist)
        cpv=sum(((float(x["high"])+float(x["low"])+float(x["close"]))/3)*float(x["volume"] or 0) for x in hist)
        vwap=cpv/cv if cv else None
        mins=max(0,int((b["ny"]-dt.datetime.combine(day,dt.time(9,30),NY)).total_seconds()/60))
        out.append({"time_utc":b["time"].isoformat(),"price":float(b["close"]),"gap_pct":((reg_open/prev_close)-1)*100,
                    "premarket_reprice_pct":pre_reprice,"holds_open":float(b["close"])>=reg_open,
                    "holds_vwap":vwap is not None and float(b["close"])>=vwap,"minutes_since_open":mins,
                    "cum_volume":cv})
    return {**case,"replay_state":"PRICE_VOLUME_RECONSTRUCTED","bars_evaluated":len(out),"first_bar":out[0] if out else None,
            "note":"Historical bid/ask spread and same-time baseline RVOL are not inferable from this single-session OHLCV pull; no BUYABLE classification is fabricated."}

def main():
    import argparse
    ap=argparse.ArgumentParser(); ap.add_argument("files",nargs="+"); ap.add_argument("--out",default="replay-results.json"); args=ap.parse_args()
    cases=[]
    for f in args.files: cases += json.loads(Path(f).read_text())["cases"]
    res=[replay_case(c) for c in cases]
    Path(args.out).write_text(json.dumps({"schema_version":1,"cases":res},indent=2)+"\n")
    print(json.dumps({"cases":len(res),"reconstructed":sum(x["replay_state"]=="PRICE_VOLUME_RECONSTRUCTED" for x in res),"insufficient":sum(x["replay_state"]!="PRICE_VOLUME_RECONSTRUCTED" for x in res)},indent=2))
if __name__=="__main__": main()
