#!/usr/bin/env python3
"""Daily missed-mover audit for Market Radar.

Pulls Yahoo Finance's public US day-gainers screener, compares strong movers
with the persistent live-alert feed, and stores a dated recall audit. This is
research telemetry: it does not create trades or change thresholds.
"""
from __future__ import annotations

import base64
import datetime as dt
import json
import os
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SOURCES_FILE = ROOT / "sources.json"
OUT_FILE = ROOT / "learning" / "missed-movers.json"
REPO = os.getenv("GITHUB_REPOSITORY", "ammarxz9333-cell/Auroraaudio")
TOKEN = os.getenv("GITHUB_TOKEN", "")
USER_AGENT = os.getenv("RADAR_USER_AGENT", "Ammar-Market-Radar-MissedMover/1.0")
MIN_GAIN_PCT = float(os.getenv("RADAR_MISSED_MIN_GAIN_PCT", "8"))
MIN_PRICE = float(os.getenv("RADAR_MISSED_MIN_PRICE", "1"))
MIN_VOLUME = int(os.getenv("RADAR_MISSED_MIN_VOLUME", "500000"))
MAX_MOVERS = int(os.getenv("RADAR_MISSED_MAX_MOVERS", "100"))
ALERT_LOOKBACK_HOURS = int(os.getenv("RADAR_MISSED_ALERT_LOOKBACK_HOURS", "72"))


def now_utc():
    return dt.datetime.now(dt.timezone.utc)


def load_json(path: Path, fallback):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return fallback


def save_json(path: Path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def request_json(url: str, headers=None):
    h = {
        "User-Agent": USER_AGENT,
        "Accept": "application/json",
    }
    if TOKEN and "api.github.com" in url:
        h["Authorization"] = f"Bearer {TOKEN}"
        h["X-GitHub-Api-Version"] = "2022-11-28"
        h["Accept"] = "application/vnd.github+json"
    if headers:
        h.update(headers)
    req = urllib.request.Request(url, headers=h)
    with urllib.request.urlopen(req, timeout=20) as resp:
        return json.loads(resp.read().decode("utf-8"))


def github_json(path: str):
    return request_json("https://api.github.com" + path)


def github_put(path: str, payload: dict):
    url = "https://api.github.com" + path
    body = json.dumps(payload).encode("utf-8")
    headers = {
        "User-Agent": USER_AGENT,
        "Accept": "application/vnd.github+json",
        "Content-Type": "application/json",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    if TOKEN:
        headers["Authorization"] = f"Bearer {TOKEN}"
    req = urllib.request.Request(url, data=body, method="PUT", headers=headers)
    with urllib.request.urlopen(req, timeout=20) as resp:
        return json.loads(resp.read().decode("utf-8"))


def persist_remote_json(path: str, value, branch="main-v2"):
    if not TOKEN:
        return None
    owner, repo = REPO.split("/", 1)
    api_path = f"/repos/{owner}/{repo}/contents/{urllib.parse.quote(path, safe='/')}"
    last_error = None
    for attempt in range(3):
        try:
            current = github_json(api_path + "?ref=" + urllib.parse.quote(branch, safe=""))
            raw = json.dumps(value, indent=2, sort_keys=True) + "\n"
            payload = {
                "message": "chore(market-radar): refresh missed-mover audit",
                "content": base64.b64encode(raw.encode("utf-8")).decode("ascii"),
                "sha": current.get("sha"),
                "branch": branch,
            }
            result = github_put(api_path, payload)
            return ((result.get("commit") or {}).get("sha"))
        except Exception as exc:
            last_error = exc
            import time
            time.sleep(0.75 * (attempt + 1))
    raise RuntimeError(f"remote persistence failed: {last_error}")


def parse_utc(value):
    if not value:
        return None
    try:
        d = dt.datetime.fromisoformat(str(value).replace("Z", "+00:00"))
        if d.tzinfo is None:
            d = d.replace(tzinfo=dt.timezone.utc)
        return d.astimezone(dt.timezone.utc)
    except Exception:
        return None


def list_alert_paths():
    owner, repo = REPO.split("/", 1)
    ref = urllib.parse.quote("market-radar-live", safe="")
    data = github_json(
        f"/repos/{owner}/{repo}/contents/market-radar/live/alerts?ref={ref}"
    )
    if not isinstance(data, list):
        return []
    return sorted(
        x.get("path")
        for x in data
        if x.get("type") == "file"
        and str(x.get("name", "")).endswith(".json")
        and x.get("path")
    )[-1000:]


def fetch_alert(path: str):
    owner, repo = REPO.split("/", 1)
    ref = urllib.parse.quote("market-radar-live", safe="")
    qpath = urllib.parse.quote(path, safe="/")
    data = github_json(
        f"/repos/{owner}/{repo}/contents/{qpath}?ref={ref}"
    )
    return json.loads(base64.b64decode(data["content"]).decode("utf-8"))


def recent_alerts_by_ticker():
    cutoff = now_utc() - dt.timedelta(hours=ALERT_LOOKBACK_HOURS)
    by_ticker = {}
    errors = []

    try:
        paths = list_alert_paths()
    except Exception as exc:
        return {}, [f"alert-list: {type(exc).__name__}: {exc}"]

    for path in paths:
        try:
            alert = fetch_alert(path)
        except Exception as exc:
            errors.append(f"alert-fetch {path}: {type(exc).__name__}: {exc}")
            continue

        detected = parse_utc(alert.get("detected_utc"))
        if not detected or detected < cutoff:
            continue

        for ticker in alert.get("tickers") or []:
            by_ticker.setdefault(ticker.upper(), []).append({
                "radar_id": alert.get("radar_id"),
                "detected_utc": alert.get("detected_utc"),
                "published_utc": alert.get("published_utc"),
                "score": alert.get("score"),
                "stage": alert.get("stage"),
                "direction": alert.get("direction"),
                "source": alert.get("source"),
                "headline": alert.get("headline"),
                "url": alert.get("url"),
            })

    return by_ticker, errors


def yahoo_day_gainers():
    url = (
        "https://query1.finance.yahoo.com/v1/finance/screener/predefined/saved"
        "?count=100&scrIds=day_gainers"
    )
    data = request_json(url)
    quotes = (((data.get("finance") or {}).get("result") or [{}])[0].get("quotes") or [])
    out = []

    for q in quotes:
        symbol = str(q.get("symbol") or "").upper().strip()
        change = q.get("regularMarketChangePercent")
        price = q.get("regularMarketPrice")
        volume = q.get("regularMarketVolume")
        quote_type = str(q.get("quoteType") or "")
        market = str(q.get("market") or "")

        if not symbol or not isinstance(change, (int, float)):
            continue
        if quote_type and quote_type != "EQUITY":
            continue
        if market and market != "us_market":
            continue
        if change < MIN_GAIN_PCT:
            continue
        if isinstance(price, (int, float)) and price < MIN_PRICE:
            continue
        if isinstance(volume, (int, float)) and volume < MIN_VOLUME:
            continue

        out.append({
            "ticker": symbol,
            "name": q.get("shortName") or q.get("longName"),
            "exchange": q.get("fullExchangeName") or q.get("exchange"),
            "price": round(float(price), 4) if isinstance(price, (int, float)) else None,
            "change_pct": round(float(change), 3),
            "volume": int(volume) if isinstance(volume, (int, float)) else None,
            "avg_volume_3m": int(q["averageDailyVolume3Month"])
            if isinstance(q.get("averageDailyVolume3Month"), (int, float)) else None,
            "market_cap": int(q["marketCap"])
            if isinstance(q.get("marketCap"), (int, float)) else None,
        })

    return sorted(out, key=lambda x: x["change_pct"], reverse=True)[:MAX_MOVERS]


def summarize_overlap(rows):
    without_alert = [x for x in rows if not x["alerted_within_lookback"]]
    return {
        "eligible_movers": len(rows),
        "recent_alert_overlap": len(rows) - len(without_alert),
        "no_recent_alert": len(without_alert),
        "no_recent_alert_watchlist_members": sum(1 for x in without_alert if x["watchlist_member"]),
        # The screener supplies no timestamp for the first crossing of +8%.
        "advance_warning_recall_pct": None,
    }


def main():
    cfg = load_json(SOURCES_FILE, {})
    watchlist = set((cfg.get("watchlist") or {}).keys())
    alerts_by_ticker, alert_errors = recent_alerts_by_ticker()
    errors = list(alert_errors)

    try:
        movers = yahoo_day_gainers()
    except Exception as exc:
        movers = []
        errors.append(f"day-gainers: {type(exc).__name__}: {exc}")

    rows = []
    for mover in movers:
        ticker = mover["ticker"]
        matching = alerts_by_ticker.get(ticker, [])
        rows.append({
            **mover,
            "watchlist_member": ticker in watchlist,
            "alerted_within_lookback": bool(matching),
            "matching_alerts": matching,
            "no_recent_alert": not bool(matching),
        })

    today = now_utc().date().isoformat()
    history = load_json(OUT_FILE, {"days": {}})
    days = history.setdefault("days", {})
    days[today] = {
        "generated_utc": now_utc().isoformat(),
        "investigation_protocol": {
            "timeline": "Reconstruct several trading days before the move through the move itself using point-in-time public timestamps.",
            "research_domains": [
                "SEC filings/exhibits and company IR",
                "customer/supplier/partner releases",
                "government contracts, procurement, permits and regulatory records",
                "analyst initiations/upgrades/downgrades and target changes",
                "index additions/rebalances and ETF/passive-flow effects",
                "earnings, guidance, balance sheet, dilution, ATM/S-3/424B5 and buybacks",
                "insider/Form 4 and 13D/13G ownership changes",
                "short interest, float, borrow and options positioning when public",
                "sector/peer/sympathy moves and commodity/macro exposures",
                "public rumors, alleged leaks, Reddit, Stocktwits and X with provenance labels",
                "intraday price/volume/VWAP/relative-strength structure before acceleration",
            ],
            "required_root_cause_labels": [
                "source-coverage-gap",
                "ticker-entity-extraction",
                "catalyst-vocabulary-gap",
                "score-threshold",
                "watchlist-bias",
                "source-latency",
                "market-data-latency",
                "deduplication",
                "direction-classification",
                "market-reaction-filter",
                "sector-sympathy-not-modeled",
                "analyst-action-not-modeled",
                "index-flow-not-modeled",
                "rumor-not-modeled",
                "price-volume-momentum-not-modeled",
                "genuinely-unpredictable",
            ],
            "required_output": "For every mover: earliest clue, full catalyst/rumor timeline, pre-move features, why missed, machine-detectable pattern, false-positive tradeoff, and proposed reversible model/source change.",
        },
        "criteria": {
            "min_gain_pct": MIN_GAIN_PCT,
            "min_price": MIN_PRICE,
            "min_volume": MIN_VOLUME,
            "alert_lookback_hours": ALERT_LOOKBACK_HOURS,
            "source": "Yahoo Finance predefined day_gainers public endpoint",
        },
        "timing_limit": "The day-gainers endpoint supplies a current quote, not the first time the move crossed the threshold. A recent alert can have arrived after the move. Overlap is not advance-warning recall.",
        "summary": summarize_overlap(rows),
        "movers": rows,
        "errors": errors[:50],
    }

    # Retain roughly one year of trading-day audits.
    for key in sorted(days)[:-280]:
        days.pop(key, None)

    save_json(OUT_FILE, history)
    remote_commit = None
    try:
        remote_commit = persist_remote_json(
            "market-radar/learning/missed-movers.json", history
        )
    except Exception as exc:
        errors.append(f"remote-persist: {type(exc).__name__}: {exc}")
    print(json.dumps({
        **days[today]["summary"],
        "remote_commit": remote_commit,
    }, indent=2))
    if errors:
        print(json.dumps({"errors": errors[:20]}, indent=2))


if __name__ == "__main__":
    main()
