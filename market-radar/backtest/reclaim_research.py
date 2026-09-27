#!/usr/bin/env python3
"""Research-only second-acceptance detector. Never changes live Gate v1."""
from __future__ import annotations

def detect_reclaim(rows, first_entry_time, first_entry_price, stop_time=None):
    """Find first post-stop reclaim with acceptance and renewed participation."""
    seen_entry=False
    stopped=False
    ep=float(first_entry_price)
    best=None
    for r in rows:
        if r["time_utc"]==first_entry_time:
            seen_entry=True
        if stop_time and r["time_utc"]>=stop_time:
            stopped=True
        if not seen_entry:
            continue
        lo=r.get("low")
        if lo is not None and float(lo)<=ep*.95:
            stopped=True
        if not stopped or r["time_utc"]<=first_entry_time or (stop_time and r["time_utc"]<=stop_time):
            continue
        ao=r.get("above_open_pct")
        av=r.get("above_vwap_pct")
        quality=sum((
            bool(r.get("holds_open")),
            bool(r.get("holds_vwap")),
            float(r.get("rvol") or 0)>=2,
            float(ao if ao is not None else 999)<=8,
            float(av if av is not None else 999)<=6,
        ))
        blockers=[]
        if not r.get("holds_open"): blockers.append("BELOW_OPEN")
        if not r.get("holds_vwap"): blockers.append("BELOW_VWAP")
        if float(r.get("rvol") or 0)<2: blockers.append("LOW_RVOL")
        if float(ao if ao is not None else 999)>8: blockers.append("OPEN_EXTENSION")
        if float(av if av is not None else 999)>6: blockers.append("VWAP_EXTENSION")
        recovery_type=("VWAP_RECLAIM_PENDING" if blockers==["BELOW_VWAP"] else
                       "EXTENSION_BREAKOUT" if blockers==["OPEN_EXTENSION"] else
                       "MIXED_RECOVERY" if blockers else "FULL_RECLAIM")
        candidate={"time_utc":r["time_utc"],"price":float(r["price"]),"conditions_met":quality,"blockers":blockers,"recovery_type":recovery_type,
                   "holds_open":bool(r.get("holds_open")),"holds_vwap":bool(r.get("holds_vwap")),
                   "rvol":r.get("rvol"),"above_open_pct":ao,"above_vwap_pct":av}
        if best is None or quality>best["conditions_met"]:
            best=candidate
        if quality==5:
            return {"state":"RECLAIM_CANDIDATE",**candidate}
    return {"state":"NO_RECLAIM","best_post_stop_candidate":best}
