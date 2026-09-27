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
from entry_gate_v2 import GateV2Input, entry_gate_v2

NY=ZoneInfo("America/New_York")
DECISION_NORMALIZATION={
    "WATCH_CONTINUATION":"WATCH","WATCH_CONTINUATION_HIGH":"WATCH",
    "WATCH_FINANCING":"WATCH_LOW_CONFIDENCE","WATCH_REGULATORY_HIGH":"WATCH",
    "WATCH_BINARY_UPCOMING":"NO_TRADE","NO_NEW_EVENT":"NO_TRADE"
}
def normalized_decision(x): return DECISION_NORMALIZATION.get(x,x)
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
    event_ny=cutoff.astimezone(NY)
    day=event_ny.date() + (dt.timedelta(days=1) if event_ny.time() >= dt.time(16,0) else dt.timedelta(0))
    while day.weekday() >= 5: day += dt.timedelta(days=1)
    start=dt.datetime.combine(day-dt.timedelta(days=7),dt.time(0),NY)
    end=dt.datetime.combine(day+dt.timedelta(days=2),dt.time(0),NY)
    bars=yahoo_5m(case["ticker"],start,end)
    today=[b for b in bars if b["ny"].date()==day]
    regular=[b for b in today if dt.time(9,30)<=b["ny"].time()<dt.time(16,0)]
    pre=[b for b in today if b["ny"].time()<dt.time(9,30)]
    prior=[b for b in bars if b["ny"].date()<day and dt.time(9,30)<=b["ny"].time()<dt.time(16,0)]
    prior_days = sorted({b["ny"].date() for b in prior})[-5:]
    if not regular or not prior:
        return {**case, "replay_state": "INSUFFICIENT_DATA", "reason": "missing bars/prior close"}
    prior_day=max(b["ny"].date() for b in prior); prev=[b for b in prior if b["ny"].date()==prior_day]
    prev_close=float(prev[-1]["close"]); reg_open=float(regular[0]["open"] or regular[0]["close"])
    pre_reprice=((float(pre[-1]["close"])/prev_close)-1)*100 if pre else None
    out=[]; spread_scenarios={str(s):None for s in (0.5,1.0,2.0,3.0)}; v2_scenarios={str(s):None for s in (0.5,1.0,2.0,3.0)}
    for idx,b in enumerate(regular):
        if event_ny.date()==day and b["time"] < cutoff.astimezone(dt.timezone.utc): continue
        vol=float(b["volume"] or 0); typical=(float(b["high"])+float(b["low"])+float(b["close"]))/3
        # VWAP must include all regular bars through this timestamp.
        hist=regular[:idx+1]; cv=sum(float(x["volume"] or 0) for x in hist)
        cpv=sum(((float(x["high"])+float(x["low"])+float(x["close"]))/3)*float(x["volume"] or 0) for x in hist)
        vwap=cpv/cv if cv else None
        mins=max(0,int((b["ny"]-dt.datetime.combine(day,dt.time(9,30),NY)).total_seconds()/60))
        prior_cums=[]
        for pd in prior_days:
            pbs=[x for x in prior if x["ny"].date()==pd and x["ny"].time()<=b["ny"].time()]
            if pbs: prior_cums.append(sum(float(x["volume"] or 0) for x in pbs))
        med=sorted(prior_cums)[len(prior_cums)//2] if prior_cums else 0
        rvol=(cv/med) if med>0 else 0.0
        row={"time_utc":b["time"].isoformat(),"price":float(b["close"]),"gap_pct":((reg_open/prev_close)-1)*100,
             "premarket_reprice_pct":pre_reprice,"holds_open":float(b["close"])>=reg_open,
             "holds_vwap":vwap is not None and float(b["close"])>=vwap,"minutes_since_open":mins,
             "cum_volume":cv,"rvol":rvol,
             "vwap":vwap,"open_price":reg_open,
             "above_vwap_pct":((float(b["close"])/vwap)-1)*100 if vwap else None,
             "above_open_pct":((float(b["close"])/reg_open)-1)*100,
             "bar_close_location":((float(b["close"])-float(b["low"]))/(float(b["high"])-float(b["low"]))) if float(b["high"])>float(b["low"]) else 0.5}
        out.append(row)
        for spread in (0.5,1.0,2.0,3.0):
            key=str(spread)
            if v2_scenarios[key] is None and pre_reprice is not None:
                vg=entry_gate_v2(GateV2Input(normalized_decision(case["decision"]),row["gap_pct"],pre_reprice,rvol,row["holds_vwap"],row["holds_open"],mins,spread,above_open_pct=row["above_open_pct"],above_vwap_pct=row["above_vwap_pct"],price=float(b["close"]),gap_pct_abs=abs(row["gap_pct"])))
                if vg["state"]=="BUYABLE_NOW":
                    entry=float(b["close"]); p5=entry*1.05; p10=entry*1.10; m5=entry*0.95
                    oc=None; p10h=False; mfe2=0.0; mae2=0.0
                    for z in regular[idx:]:
                        hi=float(z["high"]); lo=float(z["low"])
                        mfe2=max(mfe2,(hi/entry-1)*100); mae2=min(mae2,(lo/entry-1)*100); p10h=p10h or hi>=p10
                        hp=hi>=p5; hm=lo<=m5
                        if oc is None and hp and hm: oc="ORDER_UNVERIFIED"
                        elif oc is None and hp: oc="PLUS5_FIRST"
                        elif oc is None and hm: oc="MINUS5_FIRST"
                    v2_scenarios[key]={"first_buyable_time":b["time"].isoformat(),"entry_price":entry,"outcome":oc or "UNRESOLVED","plus10_reached":p10h,"mfe_pct":round(mfe2,3),"mae_pct":round(mae2,3)}
            if spread_scenarios[key] is not None: continue
            if pre_reprice is None: continue
            g=entry_gate(GateInput(normalized_decision(case["decision"]),row["gap_pct"],pre_reprice,rvol,row["holds_vwap"],row["holds_open"],mins,spread))
            if g["state"]=="BUYABLE_NOW":
                entry=float(b["close"]); plus5=entry*1.05; plus10=entry*1.10; minus5=entry*0.95
                later=regular[idx:]
                outcome=None; plus10_hit=False; mfe=0.0; mae=0.0
                for z in later:
                    hi=float(z["high"]); lo=float(z["low"])
                    mfe=max(mfe,(hi/entry-1)*100); mae=min(mae,(lo/entry-1)*100)
                    plus10_hit=plus10_hit or hi>=plus10
                    p5=hi>=plus5; m5=lo<=minus5
                    if outcome is None and p5 and m5: outcome="ORDER_UNVERIFIED"
                    elif outcome is None and p5: outcome="PLUS5_FIRST"
                    elif outcome is None and m5: outcome="MINUS5_FIRST"
                spread_scenarios[key]={"first_buyable_time":b["time"].isoformat(),"entry_price":entry,"outcome":outcome or "UNRESOLVED","plus10_reached":plus10_hit,"mfe_pct":round(mfe,3),"mae_pct":round(mae,3)}

    robust=all(spread_scenarios[str(s)] is not None for s in (0.5,1.0,2.0,3.0))
    firsts={v["first_buyable_time"] for v in spread_scenarios.values() if v}
    robust=robust and len(firsts)==1
    return {**case,"replay_state":"PRICE_VOLUME_RECONSTRUCTED","session_date":str(day),"bars_evaluated":len(out),"first_bar":out[0] if out else None,
            "normalized_decision":normalized_decision(case["decision"]),"spread_scenarios":spread_scenarios,"spread_robust_buyable":robust,
            "v2_spread_scenarios":v2_scenarios,
            "v2_spread_robust_buyable": all(v2_scenarios[str(s)] is not None for s in (0.5,1.0,2.0,3.0)) and len({v["first_buyable_time"] for v in v2_scenarios.values() if v})==1,
            "first_buyable_features": next((r for r in out if any(v and v["first_buyable_time"]==r["time_utc"] for v in spread_scenarios.values())),None),
            "note":"Same-time cumulative RVOL reconstructed from up to five prior regular sessions; historical spread tested as sensitivity scenarios."}

def main():
    import argparse
    ap=argparse.ArgumentParser(); ap.add_argument("files",nargs="+"); ap.add_argument("--out",default="replay-results.json"); args=ap.parse_args()
    cases=[]
    for f in args.files: cases += json.loads(Path(f).read_text())["cases"]
    res=[replay_case(c) for c in cases]
    Path(args.out).write_text(json.dumps({"schema_version":1,"cases":res},indent=2)+"\n")
    print(json.dumps({"cases":len(res),"reconstructed":sum(x["replay_state"]=="PRICE_VOLUME_RECONSTRUCTED" for x in res),"insufficient":sum(x["replay_state"]!="PRICE_VOLUME_RECONSTRUCTED" for x in res)},indent=2))
if __name__=="__main__": main()
