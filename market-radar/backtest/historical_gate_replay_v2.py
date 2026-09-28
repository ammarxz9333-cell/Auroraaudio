#!/usr/bin/env python3
"""Historical 5-minute replay for frozen Market Radar catalyst cases.

Reconstructs only price/volume-derived gate fields from Yahoo chart data.
Spread is not reconstructed from OHLCV; cases requiring historical bid/ask
remain INSUFFICIENT_DATA rather than receiving an invented spread.
"""
from __future__ import annotations
import datetime as dt, json, urllib.parse, urllib.request, hashlib
from pathlib import Path
from zoneinfo import ZoneInfo
from entry_gate import GateInput, entry_gate
from entry_gate_v2 import GateV2Input, entry_gate_v2
from entry_gate_v3 import GateV3Input, entry_gate_v3
from backtest.reclaim_research import detect_reclaim

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

def forward_outcome(regular, signal_index, spread_pct, slippage_pct=0.1):
    """A bar-close signal fills no earlier than the next bar's open.

    Historical bid/ask and intrabar order are unavailable. Spread and slippage
    are explicitly assumed costs; same-bar stop/target is unresolved.
    """
    if signal_index + 1 >= len(regular):
        return None
    fill_bar = regular[signal_index + 1]
    raw_open = fill_bar.get("open")
    if raw_open is None or float(raw_open) <= 0:
        return None
    entry = float(raw_open) * (1 + spread_pct / 200 + slippage_pct / 100)
    outcome = None
    outcome_time = None
    mfe = 0.0
    mae = 0.0
    plus10 = False
    for bar in regular[signal_index + 1:]:
        hi, lo = float(bar["high"]), float(bar["low"])
        mfe = max(mfe, (hi / entry - 1) * 100)
        mae = min(mae, (lo / entry - 1) * 100)
        plus10 = plus10 or hi >= entry * 1.10
        if outcome is None:
            hit_plus = hi >= entry * 1.05
            hit_minus = lo <= entry * 0.95
            outcome = ("ORDER_UNVERIFIED" if hit_plus and hit_minus else
                       "PLUS5_FIRST" if hit_plus else "MINUS5_FIRST" if hit_minus else None)
            if outcome:
                outcome_time = bar["time"].isoformat()
    return {"entry_time": fill_bar["time"].isoformat(),
            "entry_price": round(entry, 6), "raw_next_open": float(raw_open),
            "assumed_spread_pct": spread_pct, "assumed_slippage_pct": slippage_pct,
            "outcome": outcome or "UNRESOLVED", "outcome_time": outcome_time,
            "plus10_reached": plus10, "mfe_pct": round(mfe, 3), "mae_pct": round(mae, 3)}

def replay_case(case, cache_dir=None):
    cutoff=dt.datetime.fromisoformat(case["cutoff"])
    event_ny=cutoff.astimezone(NY)
    day=event_ny.date() + (dt.timedelta(days=1) if event_ny.time() >= dt.time(16,0) else dt.timedelta(0))
    while day.weekday() >= 5: day += dt.timedelta(days=1)
    start=dt.datetime.combine(day-dt.timedelta(days=7),dt.time(0),NY)
    end=dt.datetime.combine(day+dt.timedelta(days=2),dt.time(0),NY)
    cache_path=None
    if cache_dir:
        cache_path=Path(cache_dir)/f"{case['ticker']}-{day}.json"
        if cache_path.exists():
            raw=json.loads(cache_path.read_text())
            bars=[{**b,"time":dt.datetime.fromisoformat(b["time"]),"ny":dt.datetime.fromisoformat(b["ny"])} for b in raw]
        else:
            bars=yahoo_5m(case["ticker"],start,end)
            cache_path.parent.mkdir(parents=True,exist_ok=True)
            raw=[{**b,"time":b["time"].isoformat(),"ny":b["ny"].isoformat()} for b in bars]
            cache_path.write_text(json.dumps(raw,separators=(",",":"),sort_keys=True)+"\n")
    else:
        bars=yahoo_5m(case["ticker"],start,end)
    normalized=[{**b,"time":b["time"].isoformat(),"ny":b["ny"].isoformat()} for b in bars]
    bars_sha256=hashlib.sha256(json.dumps(normalized,separators=(",",":"),sort_keys=True).encode()).hexdigest()
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
    out=[]; spread_scenarios={str(s):None for s in (0.5,1.0,2.0,3.0)}; v2_scenarios={str(s):None for s in (0.5,1.0,2.0,3.0)}; v3_scenarios={str(s):None for s in (0.5,1.0,2.0,3.0)}; v3_near_miss=[]
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
             "bar_close_location":((float(b["close"])-float(b["low"]))/(float(b["high"])-float(b["low"]))) if float(b["high"])>float(b["low"]) else 0.5,"low":float(b["low"]),"high":float(b["high"])}
        out.append(row)
        # Research-only v3 near-miss diagnostics. This does not alter gate decisions.
        if pre_reprice is not None:
            blockers=[]
            risky=case["decision"] in {"BEARISH_AVOID","WATCH_LOW_CONFIDENCE","WATCH_FINANCING"}
            if risky and float(b["close"]) < 2 and abs(row["gap_pct"]) >= 20: blockers.append("EXTREME_MICROCAP")
            if risky and mins < 10: blockers.append("TOO_EARLY")
            if risky and rvol < 2: blockers.append("RVOL_LT_2")
            if risky and not row["holds_open"]: blockers.append("BELOW_OPEN")
            if risky and not row["holds_vwap"]: blockers.append("BELOW_VWAP")
            if risky and row["above_open_pct"] > 8: blockers.append("OPEN_EXTENSION")
            if risky and row["above_vwap_pct"] is not None and row["above_vwap_pct"] > 6: blockers.append("VWAP_EXTENSION")
            if risky:
                v3_near_miss.append({"blocker_count":len(blockers),"blockers":blockers,**row,"bar_index":idx})
        for spread in (0.5,1.0,2.0,3.0):
            key=str(spread)
            if v3_scenarios[key] is None and pre_reprice is not None:
                v3g=entry_gate_v3(GateV3Input(normalized_decision(case["decision"]),row["gap_pct"],pre_reprice,rvol,row["holds_vwap"],row["holds_open"],mins,spread,above_open_pct=row["above_open_pct"],above_vwap_pct=row["above_vwap_pct"],price=float(b["close"]),gap_pct_abs=abs(row["gap_pct"]),catalyst_decision=case["decision"]))
                if v3g["state"]=="BUYABLE_NOW":
                    v3r=forward_outcome(regular,idx,spread)
                    if v3r: v3_scenarios[key]={"first_buyable_time":(b["time"]+dt.timedelta(minutes=5)).isoformat(),**v3r}
            if v2_scenarios[key] is None and pre_reprice is not None:
                vg=entry_gate_v2(GateV2Input(normalized_decision(case["decision"]),row["gap_pct"],pre_reprice,rvol,row["holds_vwap"],row["holds_open"],mins,spread,above_open_pct=row["above_open_pct"],above_vwap_pct=row["above_vwap_pct"],price=float(b["close"]),gap_pct_abs=abs(row["gap_pct"])))
                if vg["state"]=="BUYABLE_NOW":
                    result = forward_outcome(regular, idx, spread)
                    if result:
                        v2_scenarios[key] = {"first_buyable_time": (b["time"] + dt.timedelta(minutes=5)).isoformat(), **result}
            if spread_scenarios[key] is not None: continue
            if pre_reprice is None: continue
            g=entry_gate(GateInput(normalized_decision(case["decision"]),row["gap_pct"],pre_reprice,rvol,row["holds_vwap"],row["holds_open"],mins,spread))
            if g["state"]=="BUYABLE_NOW":
                result = forward_outcome(regular, idx, spread)
                if result:
                    spread_scenarios[key] = {"first_buyable_time": (b["time"] + dt.timedelta(minutes=5)).isoformat(), **result}

    reclaim=None
    base_entry=spread_scenarios.get("0.5")
    if base_entry and base_entry.get("outcome")=="MINUS5_FIRST":
        reclaim=detect_reclaim(out,base_entry["first_buyable_time"],base_entry["entry_price"],base_entry.get("outcome_time"))
        if reclaim.get("state")=="RECLAIM_CANDIDATE":
            ri=next((i for i,r in enumerate(regular) if r["time"].isoformat()==reclaim["time_utc"]),None)
            if ri is not None:
                result = forward_outcome(regular, ri, 0.5)
                reclaim["signal_known_at_utc"] = (regular[ri]["time"] + dt.timedelta(minutes=5)).isoformat()
                reclaim.update(result or {"outcome": "NO_NEXT_BAR"})
    robust=all(spread_scenarios[str(s)] is not None for s in (0.5,1.0,2.0,3.0))
    firsts={v["first_buyable_time"] for v in spread_scenarios.values() if v}
    robust=robust and len(firsts)==1
    return {**case,"replay_state":"PRICE_VOLUME_RECONSTRUCTED","session_date":str(day),"bars_evaluated":len(out),"first_bar":out[0] if out else None,
            "normalized_decision":normalized_decision(case["decision"]),"spread_scenarios":spread_scenarios,"spread_robust_buyable":robust,
            "v2_spread_scenarios":v2_scenarios,"v3_spread_scenarios":v3_scenarios,
            "v2_spread_robust_buyable": all(v2_scenarios[str(s)] is not None for s in (0.5,1.0,2.0,3.0)) and len({v["first_buyable_time"] for v in v2_scenarios.values() if v})==1,
            "first_buyable_features": next((r for r in out if any(v and v["first_buyable_time"]==(dt.datetime.fromisoformat(r["time_utc"])+dt.timedelta(minutes=5)).isoformat() for v in spread_scenarios.values())),None),
            "v3_near_miss": (lambda z: ({**z, "counterfactual_outcome": forward_outcome(regular,z["bar_index"],0.5)} if z else None))(min(v3_near_miss,key=lambda z:(z["blocker_count"],z["minutes_since_open"])) if v3_near_miss else None),"reclaim_research":reclaim,"bars_sha256":bars_sha256,"bars_cache":str(cache_path) if cache_path else None,"note":"Same-time cumulative RVOL reconstructed from up to five prior regular sessions; historical spread tested as sensitivity scenarios."}

def main():
    import argparse
    ap=argparse.ArgumentParser(); ap.add_argument("files",nargs="+"); ap.add_argument("--out",default="replay-results.json"); ap.add_argument("--cache-dir"); args=ap.parse_args()
    cases=[]
    for f in args.files: cases += json.loads(Path(f).read_text())["cases"]
    res=[replay_case(c,args.cache_dir) for c in cases]
    Path(args.out).write_text(json.dumps({"schema_version":1,"cases":res},indent=2)+"\n")
    print(json.dumps({"cases":len(res),"reconstructed":sum(x["replay_state"]=="PRICE_VOLUME_RECONSTRUCTED" for x in res),"insufficient":sum(x["replay_state"]!="PRICE_VOLUME_RECONSTRUCTED" for x in res)},indent=2))
if __name__=="__main__": main()
