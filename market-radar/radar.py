#!/usr/bin/env python3
"""Market Radar: first-public-source monitor for GitHub Actions.

Uses only Python stdlib. Reads sources.json and state.json from this directory.
Creates GitHub issues for high-scoring new public items and optionally Telegram alerts.
"""
from __future__ import annotations

import datetime as dt
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
import urllib.request
import xml.etree.ElementTree as ET
from html.parser import HTMLParser
from pathlib import Path
from statistics import median
from zoneinfo import ZoneInfo

ROOT = Path(__file__).resolve().parent
SOURCES_FILE = ROOT / "sources.json"
STATE_FILE = ROOT / "state.json"
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
}

NEGATIVE_NOISE = {
    "podcast": -1, "opinion": -1, "sponsored": -2, "advertisement": -3,
    "price target": -1, "technical analysis": -1, "watchlist": -1,
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
        closes = quote.get("close") or []
        volumes = quote.get("volume") or []
        bars = []
        for i, ts in enumerate(stamps):
            close = closes[i] if i < len(closes) else None
            vol = volumes[i] if i < len(volumes) else None
            if close is None:
                continue
            when_utc = dt.datetime.fromtimestamp(ts, tz=dt.timezone.utc)
            when_ny = when_utc.astimezone(NY)
            bars.append({"ts":ts,"utc":when_utc,"ny":when_ny,"close":float(close),"volume":int(vol or 0)})
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

        today_cum = sum(b["volume"] for b in by_day.get(today_key, []) if (b["ny"].hour*60+b["ny"].minute) <= cutoff)
        hist_cums = []
        for d, dbars in by_day.items():
            if d == today_key:
                continue
            cum = sum(b["volume"] for b in dbars if (b["ny"].hour*60+b["ny"].minute) <= cutoff)
            if cum > 0:
                hist_cums.append(cum)
        vol_ratio = (today_cum / median(hist_cums)) if today_cum > 0 and hist_cums else None

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
            "same_time_volume_ratio": round(vol_ratio, 2) if vol_ratio is not None else None,
            "market_session": market_session,
            "reaction": reaction,
            "event_price": round(event_price, 4) if event_price is not None else None,
            "pre30m_move_pct": round(pre30_move, 3) if pre30_move is not None else None,
            "post30m_move_pct": round(post30_move, 3) if post30_move is not None else None,
            "since_event_move_pct": round(since_event_move, 3) if since_event_move is not None else None,
            "bar_time_utc": latest["utc"].isoformat(),
        }
    except Exception as e:
        return {"ticker":ticker,"error":f"{type(e).__name__}: {e}"}

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
    return sorted(set(found))

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

def create_issue(source, item, score, tickers, hits, rid, market_ctx=None):
    tick = " ".join(f"${t}" for t in tickers) if tickers else "NEW-CANDIDATE"
    title_text = item.get("title", "Untitled")
    title = f"[MARKET-RADAR {score}] {tick} — {title_text}"[:240]
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
            new_seen.append(rid)
            score, tickers, hits = score_item(source, item, watchlist)
            market_ctx = market_context_for_tickers(tickers, event_time=item.get("published")) if tickers else []
            valid_market = [m for m in market_ctx if not m.get("error")]
            if any(m.get("reaction") in ("reacting","major-reprice") for m in valid_market):
                score += 2
                hits.append("market-confirmation")
            if valid_market and all(m.get("reaction") == "not-yet-reacted" for m in valid_market):
                hits.append("market-not-yet-reacted")
            effective_threshold = THRESHOLD if tickers else max(THRESHOLD, 13 if source.get("class") == "social" else 11)
            if score < effective_threshold:
                continue
            if not bootstrapped:
                published = item.get("published")
                if not isinstance(published, dt.datetime):
                    continue
                age = start - published
                if age.total_seconds() < 0 or age > dt.timedelta(hours=BOOTSTRAP_ALERT_HOURS):
                    continue
            try:
                url = create_issue(source, item, score, tickers, hits, rid, market_ctx=market_ctx)
                alerts.append((source["name"], item.get("title", ""), score, tickers, url))
                telegram_alert(f"MARKET RADAR {score} {' '.join('$'+x for x in tickers) or 'NEW'}\n{item.get('title','')}\n{source['name']}\n{item.get('url','')}\nIssue: {url}")
            except Exception as e:
                errors.append(f"issue {rid}: {type(e).__name__}: {e}")

    combined = list(dict.fromkeys(list(old_seen) + new_seen))
    if len(combined) > MAX_SEEN:
        combined = combined[-MAX_SEEN:]
    market_probe = yahoo_market_snapshot("CRWV")
    state.update({
        "bootstrapped": True,
        "market_probe": market_probe,
        "seen": combined,
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
        "errors": errors[:20],
    }, indent=2))
    for s, title, score, tickers, url in alerts:
        print(f"ALERT {score} {tickers} {s}: {title} -> {url}")

if __name__ == "__main__":
    main()
