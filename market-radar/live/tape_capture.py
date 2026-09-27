#!/usr/bin/env python3
"""Persist live Market Radar tape so future entry-gate validation is timestamp-exact."""
import json, os
from datetime import datetime, timezone

FIELDS=("ticker","ts","price","volume","vwap","rvol","open_price","session_high","session_low","spread_pct")

def append_bar(path, bar):
    missing=[k for k in FIELDS if k not in bar]
    if missing: raise ValueError("missing fields: "+",".join(missing))
    os.makedirs(os.path.dirname(path),exist_ok=True)
    with open(path,"a",encoding="utf-8") as f:
        f.write(json.dumps({k:bar[k] for k in FIELDS},separators=(",",":"))+"\n")

def gate_snapshot(bar):
    return {
      "ticker":bar["ticker"],"ts":bar["ts"],"price":bar["price"],
      "holds_vwap":bar["price"]>=bar["vwap"],
      "holds_open":bar["price"]>=bar["open_price"],
      "rvol":bar["rvol"],"spread_pct":bar["spread_pct"],
      "session_high":bar["session_high"],"session_low":bar["session_low"]
    }
