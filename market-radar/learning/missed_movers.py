#!/usr/bin/env python3
"""Point-in-time missed-mover learning ledger.

This module never manufactures historical features. It records only observations
captured by the live scanner, then labels a miss later when subsequent captured
bars prove a material move occurred without a prior qualifying tape alert.
"""
from __future__ import annotations
import datetime as dt
import json
from pathlib import Path

SCHEMA_VERSION = 2
MOVE_THRESHOLD_PCT = 5.0
EARLY_WINDOW_MINUTES = 90

def load(path: Path):
    try:
        data=json.loads(path.read_text(encoding="utf-8"))
        if isinstance(data,dict): return data
    except Exception:
        pass
    return {"schema_version":SCHEMA_VERSION,"updated_utc":None,"cases":[],"metrics":{}}

def save(path: Path,data: dict):
    path.parent.mkdir(parents=True,exist_ok=True)
    data["schema_version"]=SCHEMA_VERSION
    data["updated_utc"]=dt.datetime.now(dt.timezone.utc).isoformat()
    cases=data.get("cases",[])
    eligible=[c for c in cases if c.get("outcome_known")]
    caught=[c for c in eligible if c.get("caught_early")]
    missed=[c for c in eligible if c.get("missed")]
    data["metrics"]={
        "eligible_cases":len(eligible),
        "caught_early":len(caught),
        "missed":len(missed),
        "recall_pct":round(100*len(caught)/len(eligible),2) if eligible else None,
        "warning":"Recall uses only timestamped live observations; no future data may populate pre-alert features."
    }
    path.write_text(json.dumps(data,indent=2,sort_keys=True)+"\n",encoding="utf-8")

def update_case(data: dict,snapshot: dict,tape_qualified: bool,captured_utc: str):
    ticker=str(snapshot.get("ticker") or "").upper()
    bar_time=snapshot.get("bar_time_utc")
    price=snapshot.get("price")
    if not ticker or not bar_time or not isinstance(price,(int,float)): return
    day=bar_time[:10]
    key=f"{ticker}:{day}"
    cases=data.setdefault("cases",[])
    case=next((x for x in cases if x.get("key")==key),None)
    obs={
      "captured_utc":captured_utc,"bar_time_utc":bar_time,"price":price,
      "change_pct":snapshot.get("change_pct"),"change_5m_pct":snapshot.get("change_5m_pct"),
      "change_15m_pct":snapshot.get("change_15m_pct"),"change_30m_pct":snapshot.get("change_30m_pct"),
      "same_time_volume_ratio":snapshot.get("same_time_volume_ratio"),
      "market_session":snapshot.get("market_session"),"holds_vwap":snapshot.get("holds_vwap"),
      "qualified":bool(tape_qualified)
    }
    if case is None:
        case={"key":key,"ticker":ticker,"date":day,"first_observation":obs,
              "observations":[obs],"caught_early":bool(tape_qualified),
              "outcome_known":False,"missed":False}
        cases.append(case)
    else:
        case.setdefault("observations",[]).append(obs)
        if tape_qualified: case["caught_early"]=True
    first=float(case["first_observation"]["price"])
    move=((float(price)/first)-1)*100 if first else 0
    case["max_move_from_first_pct"]=max(float(case.get("max_move_from_first_pct") or 0),move)
    # A miss is learned only from later captured observations, never reconstructed.
    if move >= MOVE_THRESHOLD_PCT:
        case["outcome_known"]=True
        case["missed"]=not bool(case.get("caught_early"))
        if case["missed"]:
            f=case["first_observation"]
            case["miss_reason"]={
              "first_change_pct":f.get("change_pct"),"first_5m_pct":f.get("change_5m_pct"),
              "first_15m_pct":f.get("change_15m_pct"),"first_30m_pct":f.get("change_30m_pct"),
              "first_same_time_rvol":f.get("same_time_volume_ratio"),
              "note":"Captured live before later >=5% move but did not qualify."
            }
    # Bound repository growth.
    case["observations"]=case["observations"][-40:]
    data["cases"]=cases[-500:]
