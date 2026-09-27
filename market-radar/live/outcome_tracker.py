#!/usr/bin/env python3
"""Leakage-safe outcome tracker for frozen Market Radar BUYABLE_NOW entries."""
from __future__ import annotations
import json
from pathlib import Path

def new_trade(ticker, entry_time_utc, entry_price, radar_id):
    return {"trade_id":f"{ticker}:{entry_time_utc}:{radar_id}","ticker":ticker,
      "entry_time_utc":entry_time_utc,"entry_price":float(entry_price),
      "plus5":False,"plus10":False,"minus5":False,"first_threshold":None,
      "resolved_time_utc":None,"max_price":float(entry_price),"min_price":float(entry_price)}

def update_trade(t, bar):
    if t.get("first_threshold") in ("PLUS5_FIRST","MINUS5_FIRST","ORDER_UNVERIFIED"):
        # Still allow +10 milestone after a +5 win, but never rewrite first threshold.
        if t["first_threshold"] != "PLUS5_FIRST": return t
    ep=float(t["entry_price"]); hi=float(bar["high"]); lo=float(bar["low"])
    t["max_price"]=max(float(t.get("max_price",ep)),hi)
    t["min_price"]=min(float(t.get("min_price",ep)),lo)
    hit5=hi >= ep*1.05; hit10=hi >= ep*1.10; hitm5=lo <= ep*0.95
    t["plus5"]=bool(t.get("plus5") or hit5); t["plus10"]=bool(t.get("plus10") or hit10)
    t["minus5"]=bool(t.get("minus5") or hitm5)
    if t.get("first_threshold") is None:
        if hit5 and hitm5: t["first_threshold"]="ORDER_UNVERIFIED"
        elif hit5: t["first_threshold"]="PLUS5_FIRST"
        elif hitm5: t["first_threshold"]="MINUS5_FIRST"
        if t["first_threshold"]: t["resolved_time_utc"]=bar["time_utc"]
    t["mfe_pct"]=round((t["max_price"]/ep-1)*100,3)
    t["mae_pct"]=round((t["min_price"]/ep-1)*100,3)
    return t

def load(path):
    p=Path(path)
    if not p.exists(): return {}
    return json.loads(p.read_text())

def save(path, trades):
    p=Path(path); p.parent.mkdir(parents=True,exist_ok=True)
    p.write_text(json.dumps(trades,indent=2,sort_keys=True)+"\n")
