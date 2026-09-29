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

def _num(v):
    if isinstance(v, (int,float)):
        return float(v)
    if not isinstance(v,str):
        return None
    x=re.sub(r"[^0-9+.\\-]","",v)
    try: return float(x)
    except ValueError: return None

def discover_candidates(fetch, limit=30):
    """Cheap broad-universe pass; expensive 5m snapshots run only on shortlist."""
    raw=json.loads(fetch(NASDAQ_URL, headers={
        "Accept":"application/json,text/plain,*/*",
        "Referer":"https://www.nasdaq.com/",
    }).decode("utf-8",errors="ignore"))
    rows=((((raw.get("data") or {}).get("rows")) or []))
    out=[]
    for r in rows:
        ticker=str(r.get("symbol") or "").upper().strip()
        if not re.fullmatch(r"[A-Z][A-Z0-9.\\-]{0,8}",ticker):
            continue
        price=_num(r.get("lastsale"))
        pct=_num(r.get("pctchange"))
        vol=_num(r.get("volume"))
        cap=_num(r.get("marketCap"))
        if price is None or pct is None or vol is None:
            continue
        # Exclude ultra-illiquid/sub-dollar noise in the broad pass.
        if price < 0.75 or vol < 100000:
            continue
        if cap is not None and cap > 0 and cap < 20_000_000:
            continue
        # Wide net: the point is not to wait for +10% before looking.
        if abs(pct) < 1.5 and vol < 2_000_000:
            continue
        rank=abs(pct)*2.0 + min(12.0, math.log10(max(vol,1))*1.5)
        out.append({"ticker":ticker,"price":price,"pctchange":pct,"volume":int(vol),
                    "market_cap":cap,"rank":round(rank,3),"source_url":SOURCE_URL})
    out.sort(key=lambda x:x["rank"],reverse=True)
    return out[:max(1,int(limit))]

def tape_signal(s):
    """Bullish early-warning score from point-in-time tape only."""
    day=float(s.get("change_pct") or 0)
    m5=float(s.get("change_5m_pct") or 0)
    m15=float(s.get("change_15m_pct") or 0)
    m30=float(s.get("change_30m_pct") or 0)
    rvol=s.get("same_time_volume_ratio")
    rvol=float(rvol) if isinstance(rvol,(int,float)) else 0.0
    sess=s.get("market_session")
    hv=s.get("holds_vwap")
    ho=s.get("holds_open")
    score=0; hits=[]
    if rvol >= 5: score+=4; hits.append("same-time-rvol>=5x")
    elif rvol >= 3: score+=3; hits.append("same-time-rvol>=3x")
    elif rvol >= 2: score+=2; hits.append("same-time-rvol>=2x")
    if m5 >= 1.5: score+=3; hits.append("5m-acceleration")
    elif m5 >= 0.8: score+=2; hits.append("5m-momentum")
    if m15 >= 2.5: score+=2; hits.append("15m-acceleration")
    if m30 >= 4.0: score+=2; hits.append("30m-acceleration")
    if 2 <= day <= 12: score+=2; hits.append("early-day-reprice")
    elif 12 < day <= 25: score+=1; hits.append("extended-day-reprice")
    elif day > 35: score-=3; hits.append("chase-risk")
    if sess=="REGULAR" and hv is True and ho is True:
        score+=2; hits.append("vwap+open-hold")
    # Premarket gets no fake VWAP credit; RVOL + acceleration must carry it.
    if sess=="PRE" and rvol >= 3 and (m5 >= 0.8 or m15 >= 2.0 or day >= 4):
        score+=2; hits.append("premarket-accumulation")
    qualifies = score >= 8 and day > 0 and rvol >= 2
    summary=(
        f"session={sess} day={day:.2f}% 5m={m5:.2f}% 15m={m15:.2f}% "
        f"30m={m30:.2f}% same-time-RVOL={rvol:.2f}x; "
        + (", ".join(hits) if hits else "no qualifying tape features")
    )
    return {"qualifies":qualifies,"score":score,"hits":hits,"summary":summary}
