#!/usr/bin/env python3
"""Tape-first discovery for Market Radar.

Purpose: surface abnormal US-equity price/volume behavior before a fresh
catalyst is broadly indexed. A tape signal creates an investigation/watch,
never a blind BUYABLE_NOW decision.
"""
from __future__ import annotations
import json, math, re

NASDAQ_URL = (
    "https://api.nasdaq.com/api/screener/stocks"
    "?tableonly=true&limit=1000&download=true"
)
SOURCE_URL = "https://www.nasdaq.com/market-activity/stocks/screener"
YAHOO_SCREENS = (
    "https://query1.finance.yahoo.com/v1/finance/screener/predefined/saved?count=100&scrIds=day_gainers",
    "https://query1.finance.yahoo.com/v1/finance/screener/predefined/saved?count=100&scrIds=most_actives",
)

def _rank_rows(rows):
    out=[]
    for r in rows:
        ticker=str(r.get("symbol") or "").upper().strip()
        if not re.fullmatch(r"[A-Z][A-Z0-9.\\-]{0,8}",ticker): continue
        price=_num(r.get("lastsale", r.get("regularMarketPrice")))
        pct=_num(r.get("pctchange", r.get("regularMarketChangePercent")))
        vol=_num(r.get("volume", r.get("regularMarketVolume")))
        cap=_num(r.get("marketCap"))
        if price is None or pct is None or vol is None: continue
        if price < 0.75 or vol < 100000: continue
        if cap is not None and cap > 0 and cap < 20_000_000: continue
        if abs(pct) < 1.5 and vol < 2_000_000: continue
        rank=abs(pct)*2.0 + min(12.0, math.log10(max(vol,1))*1.5)
        out.append({"ticker":ticker,"price":price,"pctchange":pct,"volume":int(vol),
                    "market_cap":cap,"rank":round(rank,3),"source_url":SOURCE_URL})
    out.sort(key=lambda x:x["rank"],reverse=True)
    return out

def _yahoo_candidates(fetch):
    rows=[]
    for url in YAHOO_SCREENS:
        raw=json.loads(fetch(url, headers={"Accept":"application/json"}).decode("utf-8",errors="ignore"))
        result=(((raw.get("finance") or {}).get("result")) or [])
        if result: rows.extend(result[0].get("quotes") or [])
    return _rank_rows(rows)

def _num(v):
    if isinstance(v, (int,float)):
        return float(v)
    if not isinstance(v,str):
        return None
    x=re.sub(r"[^0-9+.\\-]","",v)
    try: return float(x)
    except ValueError: return None


def tape_signal(snapshot):
    """Classify abnormal tape action conservatively.

    Requires abnormal relative volume plus fresh momentum. Large day moves
    without continuing short-window momentum are treated as already repriced,
    not early opportunities.
    """
    s=snapshot or {}
    rvol=_num(s.get("same_time_volume_ratio"))
    day=_num(s.get("change_pct")) or 0.0
    m5=_num(s.get("change_5m_pct")) or 0.0
    m15=_num(s.get("change_15m_pct")) or 0.0
    m30=_num(s.get("change_30m_pct")) or 0.0
    session=str(s.get("market_session") or "").upper()
    hits=[]
    if rvol is None or rvol < 2.0:
        return {"qualifies":False,"hits":[],"score":0.0,"reason":"insufficient-relative-volume"}
    # Reject stale/parabolic day moves whose immediate tape has gone flat.
    if abs(day) >= 25 and max(abs(m5),abs(m15),abs(m30)) < 1.5:
        return {"qualifies":False,"hits":["parabolic-chase"],"score":0.0,"reason":"already-repriced"}
    if session in {"PRE","PREMARKET"} and rvol >= 3 and abs(day) >= 3 and (abs(m15)>=1.5 or abs(m30)>=2.0):
        hits.append("premarket-accumulation")
    if session=="REGULAR" and rvol >= 2.5 and (abs(m5)>=0.8 or abs(m15)>=1.5 or abs(m30)>=2.5):
        if s.get("holds_vwap") is not False:
            hits.append("regular-volume-momentum")
    score=min(10.0,(rvol or 0)*0.8+abs(m5)*0.7+abs(m15)*0.5+abs(m30)*0.3)
    return {"qualifies":bool(hits),"hits":hits,"score":round(score,3),
            "reason":"fresh-abnormal-tape" if hits else "no-fresh-momentum"}

def discover_candidates(fetch, limit=30):
    """Broad-universe pass with Yahoo fallback when Nasdaq is unavailable."""
    errors=[]
    rows=[]
    try:
        raw=json.loads(fetch(NASDAQ_URL, headers={
            "Accept":"application/json,text/plain,*/*",
            "Referer":"https://www.nasdaq.com/",
        }).decode("utf-8",errors="ignore"))
        rows=((((raw.get("data") or {}).get("rows")) or []))
        ranked=_rank_rows(rows)
    except Exception as exc:
        errors.append(exc)
        ranked=[]
    if not ranked:
        try:
            ranked=_yahoo_candidates(fetch)
        except Exception as exc:
            errors.append(exc)
    if not ranked and errors:
        raise errors[-1]
    # Deduplicate symbols when Yahoo gainers/actives overlap.
    unique={}
    for row in ranked:
        unique.setdefault(row["ticker"],row)
    ranked=sorted(unique.values(),key=lambda x:x["rank"],reverse=True)
    return ranked[:max(1,int(limit))]

