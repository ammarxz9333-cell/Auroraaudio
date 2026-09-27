#!/usr/bin/env python3
"""Market Radar: first-public-source monitor for GitHub Actions.

Uses only Python stdlib. Reads sources.json and state.json from this directory.
Creates GitHub issues for high-scoring new public items and optionally Telegram alerts.
"""
from __future__ import annotations

import datetime as dt
import base64
import email.utils
import hashlib
import html
import json
import os
import re
import sys
import gzip
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
import urllib.parse
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
from html.parser import HTMLParser
from pathlib import Path
from statistics import median
from zoneinfo import ZoneInfo
from entry_gate import GateInput, entry_gate
from provenance import provenance
from live.outcome_tracker import new_trade, update_trade, load as load_trades, save as save_trades, live_metrics

ROOT = Path(__file__).resolve().parent
SOURCES_FILE = ROOT / "sources.json"
STATE_FILE = ROOT / "state.json"
TAPE_DIR = ROOT / "live" / "tape"
TRADES_FILE = ROOT / "live" / "trades.json"
USER_AGENT = os.getenv("RADAR_USER_AGENT", "MarketRadar/1.0 public-source-monitor contact=github-actions")
REPO = os.getenv("GITHUB_REPOSITORY", "")
TOKEN = os.getenv("GITHUB_TOKEN", "")
OWNER = REPO.split("/", 1)[0] if "/" in REPO else ""
THRESHOLD = int(os.getenv("RADAR_SCORE_THRESHOLD", "8"))
BOOTSTRAP_ALERT_HOURS = int(os.getenv("RADAR_BOOTSTRAP_ALERT_HOURS", "8"))
MAX_SEEN = int(os.getenv("RADAR_MAX_SEEN", "6000"))
REQUEST_TIMEOUT = int(os.getenv("RADAR_REQUEST_TIMEOUT", "8"))
MAX_WORKERS = int(os.getenv("RADAR_MAX_WORKERS", "12"))
DOMAIN_LOCKS = {"www.sec.gov": threading.Lock(), "www.reddit.com": threading.Lock()}
DOMAIN_LAST = {}
DOMAIN_MIN_INTERVAL = {"www.sec.gov": 0.40, "www.reddit.com": 1.25}

CATALYSTS = {
    "definitive agreement": 5, "merger": 5, "acquisition": 5, "takeover": 5,
    "tender offer": 5, "going private": 5, "fda approves": 5, "fda approved": 5,
    "approval": 3, "complete response letter": 5, "phase iii": 5, "phase 3": 5,
    "primary endpoint": 4, "contract award": 5, "awarded contract": 5,
    "government contract": 5, "chapter 11": 5, "bankruptcy": 5,
    "strategic review": 4, "activist stake": 5, "schedule 13d": 5,
    "short report": 5, "fraud": 5, "subpoena": 5, "recall": 4,
    "raises guidance": 4, "raised guidance": 4, "lowers guidance": 4,
    "lowered guidance": 4, "guidance": 2, "registered direct": 4,
    "public offering": 4, "atm offering": 4, "convertible": 3, "warrant": 3,
    "dilution": 4, "strategic partnership": 4, "partnership": 2,
    "supply agreement": 4, "offtake": 4, "hyperscaler": 4, "data center": 3,
    "defense": 2, "department of defense": 4, "clinical trial": 3,
    "pdufa": 4, "snda": 4, "nda": 2, "bla": 3, "contract": 2,
    "exclusive": 3, "sources say": 4, "people familiar": 4, "according to people": 4,
    "scoop": 3, "leak": 3, "leaked": 3, "rumor": 2, "reportedly": 2,
    "investigation": 3, "probe": 3, "permit": 2, "interconnection": 3,
    "procurement": 3, "award notice": 4, "material agreement": 4,
    "8-k": 2, "form 8-k": 2, "13d": 4, "13g": 2, "form 4": 1,
    "initiates coverage": 3, "upgrades": 3, "raised its price target": 2,
    "certification": 3, "commercial operation": 4, "commercial production": 4,
    "index inclusion": 4, "added to the index": 4, "rebalance": 2,
}

NEGATIVE_NOISE = {
    "podcast": -1, "opinion": -1, "sponsored": -2, "advertisement": -3,
    "price target": -1, "technical analysis": -1, "watchlist": -1,
    "shares acquired by": -5, "shares purchased by": -5,
    "stock position": -4, "holdings in": -4, "quarterly 13f": -4,
}

class LinkParser(HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []
        self._href = None
        self._text = []
    def handle_starttag(self, tag, attrs):
        if tag.lower() == "a":
            self._href = dict(attrs).get("href")
            self._text = []
    def handle_data(self, data):
        if self._href is not None:
            self._text.append(data)
    def handle_endtag(self, tag):
        if tag.lower() == "a" and self._href is not None:
            text = " ".join("".join(self._text).split())
            if text and self._href:
                self.links.append((self._href, html.unescape(text)))
            self._href = None
            self._text = []

def now_utc():
    return dt.datetime.now(dt.timezone.utc)

def fetch(url: str, headers: dict | None = None) -> bytes:
    h = {
        "User-Agent": USER_AGENT,
        "Accept": "application/rss+xml, application/atom+xml, application/xml, text/xml, text/html;q=0.9,*/*;q=0.5",
    }
    host = urllib.parse.urlparse(url).netloc.lower()
    if host == "www.sec.gov":
        h["Accept-Encoding"] = "gzip, deflate"
    if headers:
        h.update(headers)
    lock = DOMAIN_LOCKS.get(host)
    if lock:
        with lock:
            min_gap = DOMAIN_MIN_INTERVAL.get(host, 0)
            elapsed = time.monotonic() - DOMAIN_LAST.get(host, 0)
            if elapsed < min_gap:
                time.sleep(min_gap - elapsed)
            req = urllib.request.Request(url, headers=h)
            with urllib.request.urlopen(req, timeout=REQUEST_TIMEOUT) as resp:
                raw = resp.read()
                DOMAIN_LAST[host] = time.monotonic()
                if resp.headers.get("Content-Encoding", "").lower() == "gzip":
                    return gzip.decompress(raw)
                return raw
    req = urllib.request.Request(url, headers=h)
    with urllib.request.urlopen(req, timeout=REQUEST_TIMEOUT) as resp:
        return resp.read()

def clean_text(s: str | None) -> str:
    if not s:
        return ""
    s = re.sub(r"<[^>]+>", " ", s)
    return " ".join(html.unescape(s).split())

def parse_date(value: str | None):
    if not value:
        return None
    value = value.strip()
    try:
        d = email.utils.parsedate_to_datetime(value)
        if d.tzinfo is None:
            d = d.replace(tzinfo=dt.timezone.utc)
        return d.astimezone(dt.timezone.utc)
    except Exception:
        pass
    try:
        d = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
        if d.tzinfo is None:
            d = d.replace(tzinfo=dt.timezone.utc)
        return d.astimezone(dt.timezone.utc)
    except Exception:
        return None

def node_text(node, tags):
    for tag in tags:
        x = node.find(tag)
        if x is not None and x.text:
            return x.text.strip()
    return ""

def parse_feed(data: bytes, source: dict):
    items = []
    try:
        root = ET.fromstring(data)
    except ET.ParseError:
        return items
    for node in root.findall(".//item"):
        title = clean_text(node_text(node, ["title"]))
        link = node_text(node, ["link"])
        desc = clean_text(node_text(node, ["description", "{http://purl.org/rss/1.0/modules/content/}encoded"]))
        published = parse_date(node_text(node, ["pubDate", "date", "{http://purl.org/dc/elements/1.1/}date"]))
        if title or link:
            items.append({"title": title, "url": link, "snippet": desc[:1200], "published": published})
    ns = {"a": "http://www.w3.org/2005/Atom"}
    for node in root.findall(".//a:entry", ns):
        title = clean_text(node.findtext("a:title", default="", namespaces=ns))
        link = ""
        for l in node.findall("a:link", ns):
            href = l.attrib.get("href", "")
            rel = l.attrib.get("rel", "alternate")
            if href and rel in ("alternate", ""):
                link = href
                break
            if href and not link:
                link = href
        desc = clean_text(node.findtext("a:summary", default="", namespaces=ns) or node.findtext("a:content", default="", namespaces=ns))
        published = parse_date(node.findtext("a:published", default="", namespaces=ns) or node.findtext("a:updated", default="", namespaces=ns))
        if title or link:
            items.append({"title": title, "url": link, "snippet": desc[:1200], "published": published})
    return items

def parse_html_links(data: bytes, source: dict):
    text = data.decode("utf-8", errors="ignore")
    p = LinkParser()
    p.feed(text)
    base = source["url"]
    allow = source.get("allow_regex")
    deny = source.get("deny_regex")
    out = []
    seen = set()
    for href, title in p.links:
        url = urllib.parse.urljoin(base, href)
        if not url.startswith("http"):
            continue
        if allow and not re.search(allow, url, re.I):
            continue
        if deny and re.search(deny, url, re.I):
            continue
        key = (url, title)
        if key in seen:
            continue
        seen.add(key)
        if len(title) < 8:
            continue
        out.append({"title": title[:400], "url": url, "snippet": "", "published": None})
    return out[:250]

def google_news_url(query: str) -> str:
    return "https://news.google.com/rss/search?" + urllib.parse.urlencode({"q": query, "hl": "en-US", "gl": "US", "ceid": "US:en"})


NY = ZoneInfo("America/New_York")

def yahoo_market_snapshot(ticker: str, event_time=None):
    """Free/no-key intraday snapshot from Yahoo Finance chart endpoint.
    Returns None if unavailable. Uses 5m bars over 5d including pre/post market.
    """
    url = (
        "https://query1.finance.yahoo.com/v8/finance/chart/"
        + urllib.parse.quote(ticker)
        + "?interval=5m&range=5d&includePrePost=true&events=div%2Csplits"
    )
    try:
        data = json.loads(fetch(url, headers={"Accept":"application/json"}).decode("utf-8", errors="ignore"))
        result = ((data.get("chart") or {}).get("result") or [None])[0]
        if not result:
            return None
        meta = result.get("meta") or {}
        stamps = result.get("timestamp") or []
        quote = (((result.get("indicators") or {}).get("quote") or [{}])[0]) or {}
        opens = quote.get("open") or []
        highs = quote.get("high") or []
        lows = quote.get("low") or []
        closes = quote.get("close") or []
        volumes = quote.get("volume") or []
        bars = []
        for i, ts in enumerate(stamps):
            opn = opens[i] if i < len(opens) else None
            high = highs[i] if i < len(highs) else None
            low = lows[i] if i < len(lows) else None
            close = closes[i] if i < len(closes) else None
            vol = volumes[i] if i < len(volumes) else None
            if close is None:
                continue
            when_utc = dt.datetime.fromtimestamp(ts, tz=dt.timezone.utc)
            when_ny = when_utc.astimezone(NY)
            bars.append({"ts":ts,"utc":when_utc,"ny":when_ny,"open":float(opn) if opn is not None else None,"high":float(high) if high is not None else float(close),"low":float(low) if low is not None else float(close),"close":float(close),"volume":int(vol or 0)})
        if not bars:
            return None

        latest = bars[-1]

        last_two = bars[-2:] if len(bars) >= 2 else bars
        change_5m = None
        if len(last_two) == 2 and last_two[0]["close"]:
            change_5m = ((last_two[1]["close"] / last_two[0]["close"]) - 1.0) * 100.0

        event_price = None
        pre30_move = None
        post30_move = None
        since_event_move = None
        if isinstance(event_time, dt.datetime):
            et = event_time.astimezone(dt.timezone.utc)
            before = [b for b in bars if b["utc"] <= et]
            if before:
                event_price = before[-1]["close"]
                pre30 = [b for b in bars if b["utc"] <= et - dt.timedelta(minutes=30)]
                if pre30 and pre30[-1]["close"]:
                    pre30_move = ((event_price / pre30[-1]["close"]) - 1.0) * 100.0
                post30 = [b for b in bars if et <= b["utc"] <= et + dt.timedelta(minutes=30)]
                if post30 and event_price:
                    post30_move = ((post30[-1]["close"] / event_price) - 1.0) * 100.0
                if event_price:
                    since_event_move = ((latest["close"] / event_price) - 1.0) * 100.0

        ny_now = latest["ny"]
        mins_now = ny_now.hour * 60 + ny_now.minute
        reg_start, reg_end = 9*60+30, 16*60
        if mins_now < reg_start:
            market_session = "PRE"
            cutoff = reg_start
        elif mins_now < reg_end:
            market_session = "REGULAR"
            cutoff = mins_now
        else:
            market_session = "AFTER/CLOSED"
            cutoff = reg_end - 1

        by_day = {}
        for b in bars:
            m = b["ny"].hour * 60 + b["ny"].minute
            if reg_start <= m < reg_end:
                by_day.setdefault(b["ny"].date().isoformat(), []).append(b)

        today_key = ny_now.date().isoformat()
        day_keys = sorted(by_day.keys())
        prev_close = None
        if today_key in by_day:
            prior_days = [d for d in day_keys if d < today_key]
            if prior_days:
                prev_close = by_day[prior_days[-1]][-1]["close"]
        elif day_keys:
            prev_close = by_day[day_keys[-1]][-1]["close"]
        if prev_close is None:
            prev_close = meta.get("regularMarketPreviousClose") or meta.get("previousClose") or meta.get("chartPreviousClose")
        if prev_close is None:
            prev_close = bars[0]["close"]
        prev_close = float(prev_close)
        change_pct = ((latest["close"] / prev_close) - 1.0) * 100.0 if prev_close else None

        # Session-level premarket repricing is independent of the catalyst timestamp.
        # This prevents missing event-relative data from being interpreted as a 0% premarket move.
        today_premarket = [b for b in bars if b["ny"].date().isoformat() == today_key and (b["ny"].hour * 60 + b["ny"].minute) < reg_start]
        session_premarket_reprice = None
        if today_premarket and prev_close:
            session_premarket_reprice = ((today_premarket[-1]["close"] / prev_close) - 1.0) * 100.0

        today_bars = [b for b in by_day.get(today_key, []) if (b["ny"].hour*60+b["ny"].minute) <= cutoff]
        today_cum = sum(b["volume"] for b in today_bars)
        regular_open = today_bars[0]["open"] if today_bars else None
        session_high = max((b["high"] for b in today_bars), default=None)
        session_low = min((b["low"] for b in today_bars), default=None)
        vwap_num = sum((((b["high"] + b["low"] + b["close"]) / 3.0) * b["volume"]) for b in today_bars if b["volume"] > 0)
        vwap_den = sum(b["volume"] for b in today_bars if b["volume"] > 0)
        session_vwap = (vwap_num / vwap_den) if vwap_den else None
        hist_cums = []
        for d, dbars in by_day.items():
            if d == today_key:
                continue
            cum = sum(b["volume"] for b in dbars if (b["ny"].hour*60+b["ny"].minute) <= cutoff)
            if cum > 0:
                hist_cums.append(cum)
        vol_ratio = (today_cum / median(hist_cums)) if today_cum > 0 and hist_cums else None

        # Research features: record them point-in-time; do not alter the live gate yet.
        recent = today_bars[-3:]
        prior_recent = today_bars[-6:-3] if len(today_bars) >= 6 else []
        recent_vol = sum(b["volume"] for b in recent)
        prior_recent_vol = sum(b["volume"] for b in prior_recent)
        volume_acceleration = (recent_vol / prior_recent_vol) if prior_recent_vol > 0 else None
        opening_range = today_bars[:3]
        opening_range_high = max((b["high"] for b in opening_range), default=None)
        opening_range_low = min((b["low"] for b in opening_range), default=None)
        above_open_pct = ((latest["close"]/regular_open)-1)*100 if regular_open else None
        above_vwap_pct = ((latest["close"]/session_vwap)-1)*100 if session_vwap else None
        from_session_high_pct = ((latest["close"]/session_high)-1)*100 if session_high else None
        holds_opening_range_high = (latest["close"] >= opening_range_high) if opening_range_high is not None else None
        ret_15m = ((today_bars[-1]["close"]/today_bars[-4]["close"])-1)*100 if len(today_bars) >= 4 and today_bars[-4]["close"] else None
        ret_30m = ((today_bars[-1]["close"]/today_bars[-7]["close"])-1)*100 if len(today_bars) >= 7 and today_bars[-7]["close"] else None

        abs_change = abs(change_pct or 0)
        if abs_change >= 10 or (vol_ratio is not None and vol_ratio >= 4):
            reaction = "major-reprice"
        elif abs_change >= 3 or (vol_ratio is not None and vol_ratio >= 2):
            reaction = "reacting"
        elif abs_change < 2 and (vol_ratio is None or vol_ratio < 1.25):
            reaction = "not-yet-reacted"
        else:
            reaction = "mixed/early"

        return {
            "ticker": ticker,
            "price": round(latest["close"], 4),
            "previous_close": round(prev_close, 4),
            "change_pct": round(change_pct, 3) if change_pct is not None else None,
            "change_5m_pct": round(change_5m, 3) if change_5m is not None else None,
            "cum_volume": today_cum,
            "regular_open": round(regular_open, 4) if regular_open is not None else None,
            "session_high": round(session_high, 4) if session_high is not None else None,
            "session_low": round(session_low, 4) if session_low is not None else None,
            "vwap": round(session_vwap, 4) if session_vwap is not None else None,
            "holds_vwap": (latest["close"] >= session_vwap) if session_vwap is not None else None,
            "holds_open": (latest["close"] >= regular_open) if regular_open is not None else None,
            "same_time_volume_ratio": round(vol_ratio, 2) if vol_ratio is not None else None,
            "volume_acceleration_15m": round(volume_acceleration, 3) if volume_acceleration is not None else None,
            "opening_range_high": round(opening_range_high, 4) if opening_range_high is not None else None,
            "opening_range_low": round(opening_range_low, 4) if opening_range_low is not None else None,
            "holds_opening_range_high": holds_opening_range_high,
            "above_open_pct": round(above_open_pct, 3) if above_open_pct is not None else None,
            "above_vwap_pct": round(above_vwap_pct, 3) if above_vwap_pct is not None else None,
            "from_session_high_pct": round(from_session_high_pct, 3) if from_session_high_pct is not None else None,
            "momentum_15m_pct": round(ret_15m, 3) if ret_15m is not None else None,
            "momentum_30m_pct": round(ret_30m, 3) if ret_30m is not None else None,
            "market_session": market_session,
            "reaction": reaction,
            "event_price": round(event_price, 4) if event_price is not None else None,
            "pre30m_move_pct": round(pre30_move, 3) if pre30_move is not None else None,
            "premarket_reprice_pct": round(session_premarket_reprice, 3) if session_premarket_reprice is not None else None,
            "post30m_move_pct": round(post30_move, 3) if post30_move is not None else None,
            "since_event_move_pct": round(since_event_move, 3) if since_event_move is not None else None,
            "bar_time_utc": latest["utc"].isoformat(),
            "bar_high": round(latest["high"], 4),
            "bar_low": round(latest["low"], 4),
        }
    except Exception as e:
        return {"ticker":ticker,"error":f"{type(e).__name__}: {e}"}

def finra_short_sale_volume(ticker: str):
    """Official FINRA Reg SHO daily short-sale volume; NOT short interest."""
    try:
        payload=json.dumps({"limit":20,"fields":["tradeReportDate","securitiesInformationProcessorSymbolIdentifier","shortParQuantity","shortExemptParQuantity","totalParQuantity"],"compareFilters":[{"compareType":"equal","fieldName":"securitiesInformationProcessorSymbolIdentifier","fieldValue":ticker.upper()}]}).encode("utf-8")
        req=urllib.request.Request("https://api.finra.org/data/group/otcMarket/name/regShoDaily",data=payload,method="POST",headers={"User-Agent":USER_AGENT,"Accept":"application/json","Content-Type":"application/json"})
        rows=json.loads(urllib.request.urlopen(req,timeout=REQUEST_TIMEOUT).read().decode("utf-8"))
        if not rows: return None
        bydate={}
        for r in rows:
            d=r.get("tradeReportDate"); total=float(r.get("totalParQuantity") or 0); short=float(r.get("shortParQuantity") or 0); exempt=float(r.get("shortExemptParQuantity") or 0)
            x=bydate.setdefault(d,{"total":0.0,"short":0.0,"exempt":0.0}); x["total"]+=total; x["short"]+=short; x["exempt"]+=exempt
        d=sorted(bydate)[-1]; x=bydate[d]
        return {"trade_date":d,"short_sale_volume":int(x["short"]),"short_exempt_volume":int(x["exempt"]),"finra_reported_volume":int(x["total"]),"short_sale_volume_pct":round(100*x["short"]/x["total"],3) if x["total"] else None,"is_short_interest":False,"source":"FINRA_REG_SHO_DAILY"}
    except Exception as e:
        return {"error":f"{type(e).__name__}: {e}","source":"FINRA_REG_SHO_DAILY"}

def yahoo_quote_bid_ask(ticker: str):
    """Best-effort Yahoo quote bid/ask. Endpoint may require cookie/crumb.
    Failure is non-fatal and must never be replaced with an inferred spread.
    """
    try:
        # Bootstrap Yahoo cookie, then obtain crumb for v7 quote.
        opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor())
        opener.addheaders = [("User-Agent", USER_AGENT), ("Accept", "application/json,text/plain,*/*")]
        try:
            opener.open("https://fc.yahoo.com", timeout=REQUEST_TIMEOUT).read(1)
        except Exception:
            pass
        crumb = opener.open("https://query1.finance.yahoo.com/v1/test/getcrumb", timeout=REQUEST_TIMEOUT).read().decode("utf-8").strip()
        url = "https://query1.finance.yahoo.com/v7/finance/quote?" + urllib.parse.urlencode({"symbols":ticker,"crumb":crumb})
        data = json.loads(opener.open(url, timeout=REQUEST_TIMEOUT).read().decode("utf-8"))
        rows = ((data.get("quoteResponse") or {}).get("result") or [])
        if not rows:
            return None
        q = rows[0]
        bid, ask = q.get("bid"), q.get("ask")
        if not isinstance(bid,(int,float)) or not isinstance(ask,(int,float)) or bid <= 0 or ask <= 0 or ask < bid:
            return None
        mid = (bid + ask) / 2.0
        return {"bid":float(bid),"ask":float(ask),"spread_pct":((ask-bid)/mid)*100.0 if mid else None,"quote_time":q.get("regularMarketTime")}
    except Exception:
        return None

def evaluate_entry_gate(snapshot: dict, score: int, threshold: int):
    required = ("price","previous_close","regular_open","vwap","same_time_volume_ratio","holds_vwap","holds_open","bar_time_utc")
    session = snapshot.get("market_session")
    if session != "REGULAR":
        return {"state":"WAIT","reason":f"entry disabled outside regular session ({session or 'unknown'})"}
    try:
        bar_time = dt.datetime.fromisoformat(snapshot["bar_time_utc"])
        age = now_utc() - bar_time.astimezone(dt.timezone.utc)
    except (KeyError, TypeError, ValueError):
        return {"state":"INSUFFICIENT_DATA","reason":"missing or invalid bar timestamp"}
    if not dt.timedelta(0) <= age <= dt.timedelta(minutes=15):
        return {"state":"WAIT","reason":"market bar is stale or from the future"}
    if any(snapshot.get(k) is None for k in required):
        return {"state":"INSUFFICIENT_DATA","reason":"missing required point-in-time tape field"}
    quote = yahoo_quote_bid_ask(snapshot["ticker"])
    if not quote or quote.get("spread_pct") is None:
        return {"state":"INSUFFICIENT_DATA","reason":"point-in-time bid/ask unavailable"}
    reaction = snapshot.get("reaction")
    decision = "EARLY" if reaction == "not-yet-reacted" and score >= threshold else "WATCH"
    price=float(snapshot["price"]); prev=float(snapshot["previous_close"]); opn=float(snapshot["regular_open"])
    gap=((opn/prev)-1)*100 if prev else 0.0
    bt=dt.datetime.fromisoformat(snapshot["bar_time_utc"]).astimezone(NY)
    mins=max(0,(bt.hour*60+bt.minute)-(9*60+30))
    pre=snapshot.get("premarket_reprice_pct")
    if pre is None:
        pre=snapshot.get("pre30m_move_pct")
    if pre is None:
        return {"state":"INSUFFICIENT_DATA","reason":"premarket repricing unavailable"}
    gi=GateInput(decision=decision,gap_pct=gap,premarket_reprice_pct=float(pre or 0.0),rvol=float(snapshot["same_time_volume_ratio"]),holds_vwap=bool(snapshot["holds_vwap"]),holds_open=bool(snapshot["holds_open"]),minutes_since_open=mins,spread_pct=float(quote["spread_pct"]))
    out=entry_gate(gi)
    out.update({"decision":decision,"entry_price":price if out["state"]=="BUYABLE_NOW" else None,"entry_time_utc":snapshot["bar_time_utc"] if out["state"]=="BUYABLE_NOW" else None,"inputs":{"gap_pct":round(gap,3),"premarket_reprice_pct":float(pre or 0.0),"rvol":gi.rvol,"holds_vwap":gi.holds_vwap,"holds_open":gi.holds_open,"minutes_since_open":mins,"spread_pct":round(gi.spread_pct,4)}})
    return out

def persist_market_snapshot(snapshot: dict, rid: str, source_name: str, score: int, threshold: int, research_context=None):
    """Append the exact point-in-time market snapshot used by the radar.
    Missing fields stay null; never backfill them from later bars.
    """
    if not snapshot or snapshot.get("error") or not snapshot.get("ticker"):
        return
    TAPE_DIR.mkdir(parents=True, exist_ok=True)
    ticker = re.sub(r"[^A-Z0-9._-]", "_", snapshot["ticker"].upper())
    gate = evaluate_entry_gate(snapshot, score, threshold)
    catalyst_qualified = score >= threshold
    if not catalyst_qualified:
        gate = {**gate, "state": "NO_ENTRY", "reason": "catalyst score below effective threshold"}
    row = {
        "captured_utc": now_utc().isoformat(),
        "radar_id": rid,
        "source": source_name,
        "score_at_capture": score,
        "ticker": ticker,
        "bar_time_utc": snapshot.get("bar_time_utc"),
        "price": snapshot.get("price"),
        "bar_high": snapshot.get("bar_high"),
        "bar_low": snapshot.get("bar_low"),
        "previous_close": snapshot.get("previous_close"),
        "change_pct": snapshot.get("change_pct"),
        "change_5m_pct": snapshot.get("change_5m_pct"),
        "cum_volume": snapshot.get("cum_volume"),
        "same_time_volume_ratio": snapshot.get("same_time_volume_ratio"),
        "volume_acceleration_15m": snapshot.get("volume_acceleration_15m"),
        "opening_range_high": snapshot.get("opening_range_high"),
        "opening_range_low": snapshot.get("opening_range_low"),
        "holds_opening_range_high": snapshot.get("holds_opening_range_high"),
        "above_open_pct": snapshot.get("above_open_pct"),
        "above_vwap_pct": snapshot.get("above_vwap_pct"),
        "from_session_high_pct": snapshot.get("from_session_high_pct"),
        "momentum_15m_pct": snapshot.get("momentum_15m_pct"),
        "momentum_30m_pct": snapshot.get("momentum_30m_pct"),
        "market_session": snapshot.get("market_session"),
        "reaction": snapshot.get("reaction"),
        "event_price": snapshot.get("event_price"),
        "pre30m_move_pct": snapshot.get("pre30m_move_pct"),
        "post30m_move_pct": snapshot.get("post30m_move_pct"),
        "since_event_move_pct": snapshot.get("since_event_move_pct"),
        "vwap": snapshot.get("vwap"),
        "spread_pct": (gate.get("inputs") or {}).get("spread_pct"),
        "gate_data_complete": gate.get("state") != "INSUFFICIENT_DATA",
        "gate_state": gate.get("state"),
        "gate_reason": gate.get("reason"),
        "catalyst_qualified": catalyst_qualified,
        "gate": gate
    }
    if research_context is not None:
        row["research_context"] = research_context
    track_live_outcome(snapshot, gate, rid)
    with (TAPE_DIR / f"{ticker}.jsonl").open("a", encoding="utf-8") as fh:
        fh.write(json.dumps(row, separators=(",", ":")) + "\n")
    return gate

def track_live_outcome(snapshot: dict, gate: dict, rid: str):
    if not snapshot or not snapshot.get("ticker") or not snapshot.get("bar_time_utc"):
        return
    trades=load_trades(TRADES_FILE)
    ticker=snapshot["ticker"].upper()
    if gate.get("state")=="BUYABLE_NOW" and gate.get("entry_price") and gate.get("entry_time_utc"):
        tid=f"{ticker}:{gate['entry_time_utc']}:{rid}"
        if tid not in trades:
            trades[tid]=new_trade(ticker,gate["entry_time_utc"],gate["entry_price"],rid)
    for tid,t in list(trades.items()):
        if t.get("ticker") != ticker: continue
        if snapshot["bar_time_utc"] <= t["entry_time_utc"]: continue
        if snapshot.get("bar_high") is None or snapshot.get("bar_low") is None: continue
        update_trade(t,{"high":snapshot["bar_high"],"low":snapshot["bar_low"],"time_utc":snapshot["bar_time_utc"]})
    save_trades(TRADES_FILE,trades)

def refresh_active_trades():
    """Update existing BUYABLE trades independently of news/watch lifetime."""
    trades = load_trades(TRADES_FILE)
    if not trades:
        return
    tickers = sorted({str(t.get("ticker", "")).upper() for t in trades.values() if t.get("ticker")})
    changed = False
    for ticker in tickers:
        snapshot = yahoo_market_snapshot(ticker)
        if not snapshot or snapshot.get("error") or not snapshot.get("bar_time_utc"):
            continue
        if snapshot.get("bar_high") is None or snapshot.get("bar_low") is None:
            continue
        for t in trades.values():
            if str(t.get("ticker", "")).upper() != ticker:
                continue
            if snapshot["bar_time_utc"] <= t.get("entry_time_utc", ""):
                continue
            before = json.dumps(t, sort_keys=True)
            update_trade(t, {"high": snapshot["bar_high"], "low": snapshot["bar_low"], "time_utc": snapshot["bar_time_utc"]})
            if json.dumps(t, sort_keys=True) != before:
                changed = True
    if changed:
        save_trades(TRADES_FILE, trades)

def market_context_for_tickers(tickers, event_time=None):
    out = []
    for ticker in tickers[:4]:
        snap = yahoo_market_snapshot(ticker, event_time=event_time)
        if snap:
            out.append(snap)
    return out

def item_id(source_name: str, item: dict) -> str:
    raw = source_name + "\n" + (item.get("url") or "") + "\n" + (item.get("title") or "")
    return hashlib.sha256(raw.encode("utf-8", errors="ignore")).hexdigest()[:20]

def load_json(path: Path, fallback):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return fallback

def save_state(state):
    STATE_FILE.write_text(json.dumps(state, indent=2, sort_keys=True) + "\n", encoding="utf-8")

def match_watchlist(text: str, watchlist: dict):
    low = text.lower()
    found = []
    for ticker, aliases in watchlist.items():
        if re.search(rf"(?<![A-Z0-9])\$?{re.escape(ticker)}(?![A-Z0-9])", text, re.I):
            found.append(ticker)
            continue
        if any(alias.lower() in low for alias in aliases if len(alias) >= 4):
            found.append(ticker)
    # Explicit exchange/symbol notation can reveal names outside the watchlist.
    for match in re.finditer(r"(?<![\w])\$([A-Z]{2,5})(?![\w])|\b(?:NASDAQ|NYSE|AMEX)\s*:\s*([A-Z]{2,5})\b", text):
        symbol = match.group(1) or match.group(2)
        if symbol not in {"USD", "ETF", "CEO", "FDA", "SEC"}:
            found.append(symbol)
    return sorted(set(found))[:4]

def phrase_match(text: str, phrase: str) -> bool:
    if len(phrase) <= 5 and " " not in phrase:
        return re.search(rf"(?<![a-z0-9]){re.escape(phrase)}(?![a-z0-9])", text, re.I) is not None
    return phrase in text

def score_item(source: dict, item: dict, watchlist: dict):
    text = f"{item.get('title','')} {item.get('snippet','')}".lower()
    score = int(source.get("weight", 1))
    hits = []
    for phrase, pts in CATALYSTS.items():
        if phrase_match(text, phrase):
            score += pts
            hits.append(phrase)
    for phrase, pts in NEGATIVE_NOISE.items():
        if phrase in text:
            score += pts
    tickers = match_watchlist(f"{item.get('title','')} {item.get('snippet','')}", watchlist)
    if tickers:
        score += 4
        hits.append("watchlist-match")
    if source.get("class") in ("primary", "investigative", "scoop") and any(k in text for k in ("exclusive", "sources say", "people familiar", "leak", "scoop")):
        score += 2
        hits.append("source+early-language")
    if any(k in text for k in ("weekly roundup", "month in review", "top 10 stocks", "best stocks to buy")):
        score -= 3
    return max(score, 0), tickers, sorted(set(hits))

def information_quality(source: dict, item: dict):
    """Point-in-time provenance features. Research metadata only; does not alter Gate v1."""
    cls=source.get("class","other")
    base={"primary":5,"scoop":4,"investigative":4,"industry":3,"leaker":2,"social":1}.get(cls,2)
    text=f"{item.get('title','')} {item.get('snippet','')}".lower()
    rumor=any(x in text for x in ("rumor","reportedly","sources say","people familiar","according to people","leak","leaked","scoop"))
    official=cls=="primary"
    return {"source_class":cls,"source_quality":base,"rumor_language":rumor,"official_source":official}

def github_api(path: str, method="GET", payload=None):
    if not TOKEN or not REPO:
        raise RuntimeError("GITHUB_TOKEN/GITHUB_REPOSITORY unavailable")
    url = "https://api.github.com" + path
    body = None if payload is None else json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, data=body, method=method, headers={
        "Authorization": f"Bearer {TOKEN}",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": USER_AGENT,
        "Content-Type": "application/json",
    })
    with urllib.request.urlopen(req, timeout=REQUEST_TIMEOUT) as resp:
        raw = resp.read()
        return json.loads(raw.decode("utf-8")) if raw else {}

def publish_live_alert(source, item, score, tickers, hits, rid, market_ctx, infoq):
    """Append one review artifact to the persistent PR branch, idempotently."""
    published = item.get("published")
    detected = now_utc()
    path = f"market-radar/live/alerts/{rid}.json"
    artifact = {
        "radar_id": rid, "detected_utc": detected.isoformat(),
        "published_utc": published.isoformat() if isinstance(published, dt.datetime) else None,
        "source": source["name"], "source_class": source.get("class"),
        "headline": item.get("title"), "url": item.get("url"),
        "score": score, "tickers": tickers, "signals": hits,
        "information_quality": infoq, "market_context": market_ctx,
        "review_status": "UNREVIEWED; not a trade recommendation",
    }
    content = base64.b64encode((json.dumps(artifact, indent=2, sort_keys=True) + "\n").encode()).decode()
    try:
        github_api(f"/repos/{REPO}/contents/{path}", method="PUT", payload={
            "message": f"alert(market-radar): {rid}", "content": content,
            "branch": "market-radar-live",
        })
    except urllib.error.HTTPError as exc:
        # GitHub refuses a second create at the same path. Keep the original
        # immutable artifact and let the issue notification retry.
        if exc.code != 422:
            raise
    return path

def create_issue(source, item, score, tickers, hits, rid, market_ctx=None):
    tick = " ".join(f"${t}" for t in tickers) if tickers else "NEW-CANDIDATE"
    reactions = [m.get("reaction") for m in (market_ctx or []) if not m.get("error")]
    if tickers and reactions and all(r == "not-yet-reacted" for r in reactions) and score >= 8:
        stage = "EARLY"
    elif any(r == "major-reprice" for r in reactions):
        stage = "MAJOR-REPRICE"
    elif any(r == "reacting" for r in reactions):
        stage = "REACTING"
    else:
        stage = "RADAR"
    title_text = item.get("title", "Untitled")
    title = f"[MARKET-{stage} {score}] {tick} — {title_text}"[:240]
    published = item.get("published")
    pubtxt = published.isoformat() if isinstance(published, dt.datetime) else "unknown/not supplied by source"
    body = (
        f"<!-- radar-id:{rid} -->\n"
        f"## First-public-source alert\n\n"
        f"- **Score:** {score}\n"
        f"- **Source:** {source['name']}\n"
        f"- **Source class:** {source.get('class','other')}\n"
        f"- **Published timestamp:** {pubtxt}\n"
        f"- **Detected UTC:** {now_utc().isoformat()}\n"
        f"- **Tickers matched:** {', '.join(tickers) if tickers else 'none — investigate candidate'}\n"
        f"- **Signals:** {', '.join(hits) if hits else 'source weight only'}\n"
        f"- **Original/public URL:** {item.get('url','')}\n\n"
        + ("### Live market reaction\n" + "\n".join(
            f"- **{m.get('ticker')}**: price {m.get('price','?')} | day {m.get('change_pct','?')}% | last 5m {m.get('change_5m_pct','?')}% | volume vs same-time {m.get('same_time_volume_ratio','?')}x | pre-news 30m {m.get('pre30m_move_pct','?')}% | first 30m after news {m.get('post30m_move_pct','?')}% | since news {m.get('since_event_move_pct','?')}% | session {m.get('market_session','?')} | **{m.get('reaction','?')}** | bar {m.get('bar_time_utc','?')}"
            if not m.get("error") else f"- **{m.get('ticker')}**: market-data error — {m.get('error')}"
            for m in (market_ctx or [])
        ) + "\n\n" if market_ctx else "")
        + f"### Headline\n{item.get('title','')}\n\n"
        f"### Public snippet\n{item.get('snippet','')[:1600] or '(none)'}\n\n"
        f"> Automated first-pass alert. Rumors/leaks remain unverified until corroborated. Review price/volume, SEC/company filings, counterparties, dilution, short interest and options before acting.\n"
    )
    payload = {"title": title, "body": body}
    if OWNER:
        payload["assignees"] = [OWNER]
    issue = github_api(f"/repos/{REPO}/issues", method="POST", payload=payload)
    return issue.get("html_url", "")

def telegram_alert(text: str):
    token = os.getenv("TELEGRAM_BOT_TOKEN", "")
    chat = os.getenv("TELEGRAM_CHAT_ID", "")
    if not token or not chat:
        return
    url = f"https://api.telegram.org/bot{token}/sendMessage"
    data = urllib.parse.urlencode({"chat_id": chat, "text": text[:3900], "disable_web_page_preview": "false"}).encode()
    req = urllib.request.Request(url, data=data, method="POST", headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=REQUEST_TIMEOUT):
            pass
    except Exception as e:
        print(f"telegram error: {e}", file=sys.stderr)

def source_items(source):
    kind = source.get("type", "rss")
    url = source.get("url", "")
    try:
        if kind == "google_news":
            return parse_feed(fetch(google_news_url(source["query"])), source)
        if kind in ("rss", "atom"):
            return parse_feed(fetch(url), source)
        if kind == "html_links":
            return parse_html_links(fetch(url), source)
        return []
    except Exception:
        fallback = source.get("fallback_query")
        if fallback:
            return parse_feed(fetch(google_news_url(fallback)), source)
        raise

def main():
    cfg = load_json(SOURCES_FILE, {})
    sources = cfg.get("sources", [])
    watchlist = cfg.get("watchlist", {})
    state = load_json(STATE_FILE, {"bootstrapped": False, "seen": []})
    old_seen = set(state.get("seen", []))
    bootstrapped = bool(state.get("bootstrapped"))
    new_seen = []
    alerts = []
    errors = []
    start = now_utc()
    active_watches = state.get("active_watches", {})
    refresh_active_trades()
    # Re-evaluate prior high-quality candidates even when their source item is already seen.
    for rid, watch in list(active_watches.items()):
        try:
            added = dt.datetime.fromisoformat(watch["added_utc"])
        except Exception:
            active_watches.pop(rid, None)
            continue
        if start - added > dt.timedelta(days=3):
            active_watches.pop(rid, None)
            continue
        event_time = None
        if watch.get("event_time_utc"):
            try:
                event_time = dt.datetime.fromisoformat(watch["event_time_utc"])
            except Exception:
                event_time = None
        for ticker in watch.get("tickers", []):
            snap = yahoo_market_snapshot(ticker, event_time=event_time)
            if not snap or snap.get("error"):
                continue
            gate = persist_market_snapshot(snap, rid, watch.get("source", "active-watch"), int(watch.get("score", THRESHOLD)), THRESHOLD)
            alerted = watch.setdefault("buyable_alerted_tickers", [])
            if gate and gate.get("state") == "BUYABLE_NOW" and ticker not in alerted:
                telegram_alert(
                    f"MARKET RADAR BUYABLE_NOW ${ticker}\n"
                    f"Entry: {gate.get('entry_price')}\n"
                    f"Reason: {gate.get('reason')}\n"
                    f"Source: {watch.get('source', 'active-watch')}"
                )
                alerted.append(ticker)

    fetched = []
    with ThreadPoolExecutor(max_workers=MAX_WORKERS) as pool:
        futures = {pool.submit(source_items, source): source for source in sources}
        for future in as_completed(futures):
            source = futures[future]
            try:
                items = future.result()
                fetched.append((source, items))
            except Exception as e:
                errors.append(f"{source.get('name')}: {type(e).__name__}: {e}")

    for source, items in fetched:
        for item in items:
            rid = item_id(source["name"], item)
            if rid in old_seen:
                continue
            published = item.get("published")
            if not isinstance(published, dt.datetime) or not dt.timedelta(0) <= start - published <= dt.timedelta(hours=BOOTSTRAP_ALERT_HOURS):
                new_seen.append(rid)
                continue
            score, tickers, hits = score_item(source, item, watchlist)
            infoq = information_quality(source, item)
            effective_threshold = THRESHOLD if tickers else max(THRESHOLD, 13 if source.get("class") == "social" else 11)
            # Market confirmation can add at most two points. Avoid a Yahoo
            # request for every low-score headline in a large public feed.
            market_ctx = (market_context_for_tickers(tickers, event_time=item.get("published"))
                          if tickers and score >= effective_threshold - 2 else [])
            valid_market = [m for m in market_ctx if not m.get("error")]
            if any(m.get("reaction") in ("reacting","major-reprice") for m in valid_market):
                score += 2
                hits.append("market-confirmation")
            if valid_market and all(m.get("reaction") == "not-yet-reacted" for m in valid_market):
                hits.append("market-not-yet-reacted")
            prov = provenance(source, item, tickers, fetched, watchlist, match_watchlist, CATALYSTS, now_utc)
            infoq.update(prov)
            infoq["rumor_only"] = bool(infoq.get("rumor_language") and not infoq.get("primary_confirmation"))
            rv = [m.get("same_time_volume_ratio") for m in valid_market if isinstance(m.get("same_time_volume_ratio"), (int, float))]
            va = [m.get("volume_acceleration_15m") for m in valid_market if isinstance(m.get("volume_acceleration_15m"), (int, float))]
            mo = [m.get("momentum_15m_pct") for m in valid_market if isinstance(m.get("momentum_15m_pct"), (int, float))]
            infoq["market_confirmation"] = {"rvol_confirmed": bool(rv and max(rv) >= 2), "volume_acceleration_confirmed": bool(va and max(va) >= 1.5), "momentum_confirmed": bool(mo and max(mo) >= 2)}
            infoq["propagation_stage"] = "PRIMARY_CONFIRMED" if infoq.get("primary_confirmation") else ("MULTI_SOURCE" if infoq.get("independent_corroboration_count", 0) >= 2 else ("CORROBORATED" if infoq.get("independent_corroboration_count", 0) else "UNCONFIRMED"))
            # FINRA's daily report is useful for later review, but an API call per
            # news item can exhaust the four-minute scheduled scan.
            infoq["finra_short_sale_volume"] = "not queried in latency-critical scan"
            for snap in valid_market:
                persist_market_snapshot(snap, rid, source["name"], score, effective_threshold, research_context=infoq)
            if tickers and score >= effective_threshold:
                active_watches[rid] = {
                    "tickers": tickers,
                    "score": score,
                    "source": source["name"],
                    "added_utc": start.isoformat(),
                    "event_time_utc": item["published"].isoformat() if isinstance(item.get("published"), dt.datetime) else None,
                    "information_quality": infoq,
                }
            if score < effective_threshold:
                new_seen.append(rid)
                continue
            try:
                publish_live_alert(source, item, score, tickers, hits, rid, market_ctx, infoq)
                url = create_issue(source, item, score, tickers, hits, rid, market_ctx=market_ctx)
                new_seen.append(rid)
                alerts.append((source["name"], item.get("title", ""), score, tickers, url))
                ticker_text = " ".join("$" + x for x in tickers) or "NEW"
                telegram_alert(f"MARKET RADAR {score} {ticker_text}\n{item.get('title','')}\n{source['name']}\n{item.get('url','')}\nIssue: {url}")
            except Exception as e:
                errors.append(f"alert {rid}: {type(e).__name__}: {e}")

    combined = list(dict.fromkeys(list(old_seen) + new_seen))
    if len(combined) > MAX_SEEN:
        combined = combined[-MAX_SEEN:]
    market_probe = yahoo_market_snapshot("CRWV")
    metrics = live_metrics(load_trades(TRADES_FILE))
    state.update({
        "bootstrapped": True,
        "market_probe": market_probe,
        "live_metrics": metrics,
        "seen": combined,
        "active_watches": active_watches,
        "last_run_utc": now_utc().isoformat(),
        "last_alert_count": len(alerts),
        "last_error_count": len(errors),
        "last_errors": errors[:30],
    })
    save_state(state)

    print(json.dumps({
        "sources": len(sources),
        "new_items": len(new_seen),
        "alerts": len(alerts),
        "live_metrics": metrics,
        "errors": errors[:20],
    }, indent=2))
    for s, title, score, tickers, url in alerts:
        print(f"ALERT {score} {tickers} {s}: {title} -> {url}")

if __name__ == "__main__":
    main()
