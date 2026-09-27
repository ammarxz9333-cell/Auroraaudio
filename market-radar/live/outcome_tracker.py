#!/usr/bin/env python3
"""Leakage-safe outcome tracker for frozen Market Radar BUYABLE_NOW entries."""
from __future__ import annotations
import json
from pathlib import Path

def new_trade(ticker, entry_time_utc, entry_price, radar_id):
    return {"trade_id":f"{ticker}:{entry_time_utc}:{radar_id}","ticker":ticker,
      "entry_time_utc":entry_time_utc,"entry_price":float(entry_price),
      "plus5":False,"plus10":False,"minus5":False,"first_threshold":None,
      "resolved_time_utc":None,"max_price":float(entry_price),"min_price":float(entry_price),
      "last_processed_bar_utc":None}

def update_trade(t, bar):
    if t.get("last_processed_bar_utc") == bar.get("time_utc"):
        return t
    first = t.get("first_threshold")
    if first in ("MINUS5_FIRST", "ORDER_UNVERIFIED"):
        return t
    ep = float(t["entry_price"])
    hi = float(bar["high"])
    lo = float(bar["low"])
    t["max_price"] = max(float(t.get("max_price", ep)), hi)
    t["min_price"] = min(float(t.get("min_price", ep)), lo)
    hit5 = hi >= ep * 1.05
    hit10 = hi >= ep * 1.10
    hitm5 = lo <= ep * 0.95
    t["plus5"] = bool(t.get("plus5") or hit5)
    t["plus10"] = bool(t.get("plus10") or hit10)
    if first is None:
        t["minus5"] = bool(t.get("minus5") or hitm5)
        if hit5 and hitm5:
            t["first_threshold"] = "ORDER_UNVERIFIED"
        elif hit5:
            t["first_threshold"] = "PLUS5_FIRST"
        elif hitm5:
            t["first_threshold"] = "MINUS5_FIRST"
        if t.get("first_threshold"):
            t["resolved_time_utc"] = bar["time_utc"]
    t["mfe_pct"] = round((t["max_price"] / ep - 1) * 100, 3)
    t["mae_pct"] = round((t["min_price"] / ep - 1) * 100, 3)
    t["last_processed_bar_utc"] = bar.get("time_utc")
    return t

def load(path):
    p=Path(path)
    if not p.exists(): return {}
    return json.loads(p.read_text())

def save(path, trades):
    p=Path(path); p.parent.mkdir(parents=True,exist_ok=True)
    p.write_text(json.dumps(trades,indent=2,sort_keys=True)+"\n")


def live_metrics(trades):
    vals=list(trades.values())
    resolved=[t for t in vals if t.get("first_threshold") in ("PLUS5_FIRST","MINUS5_FIRST")]
    wins=[t for t in resolved if t.get("first_threshold")=="PLUS5_FIRST"]
    losses=[t for t in resolved if t.get("first_threshold")=="MINUS5_FIRST"]
    ambiguous=[t for t in vals if t.get("first_threshold")=="ORDER_UNVERIFIED"]
    plus10=[t for t in wins if t.get("plus10")]
    return {
      "total_entries":len(vals),"resolved_entries":len(resolved),
      "plus5_first":len(wins),"minus5_first":len(losses),
      "order_unverified":len(ambiguous),"plus10_after_valid_entry":len(plus10),
      "precision_plus5_pct":round(100*len(wins)/len(resolved),2) if resolved else None,
      "false_entry_rate_pct":round(100*len(losses)/len(resolved),2) if resolved else None
    }
