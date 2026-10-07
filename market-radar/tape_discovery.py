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



def tape_signal(snapshot):
    """Score first abnormal tape acceleration without waiting for full confirmation.

    Requires abnormal same-clock volume.  Designed to page EARLY while a move is
    still small, and to reject mature/parabolic moves.  It never implies a trade.
    """
    pct = float(snapshot.get("change_pct") or 0.0)
    m5 = float(snapshot.get("change_5m_pct") or 0.0)
    m15 = float(snapshot.get("change_15m_pct") or 0.0)
    m30 = float(snapshot.get("change_30m_pct") or 0.0)
    rvol = snapshot.get("same_time_volume_ratio")
    rvol = float(rvol) if rvol is not None else 0.0
    session = str(snapshot.get("market_session") or "")
    hits, score = [], 0

    # Volume is mandatory: price-only gainers are too noisy.
    if rvol < 1.8:
        return {"qualifies": False, "score": 0, "hits": ["insufficient-rvol"],
                "summary": f"RVOL {rvol:.2f}x below early-tape floor"}

    if rvol >= 2.0:
        score += 2; hits.append("abnormal-rvol")
    if rvol >= 4.0:
        score += 2; hits.append("extreme-rvol")

    # First acceleration: deliberately fires before VWAP confirmation.
    if m5 >= 0.7:
        score += 2; hits.append("5m-acceleration")
    if m15 >= 1.5:
        score += 2; hits.append("15m-acceleration")
    if m30 >= 2.2:
        score += 1; hits.append("30m-acceleration")

    # Small day move + abnormal tape is the desired early window.
    if 1.0 <= pct <= 5.0 and (m5 >= 0.7 or m15 >= 1.5):
        score += 3; hits.append("early-window")
    elif 5.0 < pct <= 10.0:
        score += 1; hits.append("developing-window")

    if session == "PRE" and pct >= 1.5 and rvol >= 2.5 and (m15 >= 1.5 or m30 >= 2.2):
        score += 2; hits.append("premarket-accumulation")

    if snapshot.get("holds_vwap") is True:
        score += 1; hits.append("vwap-hold")
    if snapshot.get("holds_open") is True:
        score += 1; hits.append("open-hold")

    # Do not page a mature move as an 'early' discovery.
    if abs(pct) >= 20:
        score -= 6; hits.append("chase-penalty")
    if abs(pct) >= 40:
        score -= 8; hits.append("parabolic-penalty")
    if abs(pct) >= 15 and abs(m15) < 1.0:
        score -= 4; hits.append("stale-move-penalty")

    qualifies = score >= 7 and abs(pct) < 20
    summary = (
        f"day {pct:+.2f}% | 5m {m5:+.2f}% | 15m {m15:+.2f}% | "
        f"30m {m30:+.2f}% | same-time RVOL {rvol:.2f}x | {session}"
    )
    return {"qualifies": qualifies, "score": score, "hits": hits, "summary": summary}
