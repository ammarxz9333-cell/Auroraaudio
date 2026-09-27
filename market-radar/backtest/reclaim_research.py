#!/usr/bin/env python3
"""Research-only second-acceptance detector. Never changes live Gate v1."""
from __future__ import annotations

def detect_reclaim(rows, first_entry_time, first_entry_price):
    """Find first post-stop reclaim with VWAP+open acceptance and renewed participation.

    Requires the initial entry to have experienced -5% first. A reclaim candidate
    must occur later, hold open and VWAP, be <=8% above open, <=6% above VWAP,
    and have RVOL >=2. Returns research metadata only.
    """
    seen_entry=False; stopped=False
    ep=float(first_entry_price)
    for r in rows:
        if r["time_utc"]==first_entry_time: seen_entry=True
        if not seen_entry: continue
        lo=r.get("low")
        if lo is not None and float(lo)<=ep*.95: stopped=True
        if not stopped or r["time_utc"]<=first_entry_time: continue
        if (r.get("holds_open") and r.get("holds_vwap") and
            float(r.get("rvol") or 0)>=2 and
            float(r.get("above_open_pct") or 999)<=8 and
            float(r.get("above_vwap_pct") or 999)<=6):
            return {"state":"RECLAIM_CANDIDATE","time_utc":r["time_utc"],"price":float(r["price"]),
                    "rvol":r.get("rvol"),"above_open_pct":r.get("above_open_pct"),
                    "above_vwap_pct":r.get("above_vwap_pct")}
    return {"state":"NO_RECLAIM"}
