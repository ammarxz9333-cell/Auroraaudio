#!/usr/bin/env python3
"""Market Radar outcome evaluator.

Reads alert artifacts from the persistent market-radar-live branch, samples
subsequent Yahoo Finance 5-minute bars, and writes point-in-time outcome and
calibration metrics. It never changes the live alert threshold automatically;
changes should be justified by accumulated out-of-sample evidence.
"""
from __future__ import annotations

import base64
import datetime as dt
import json
import os
import statistics
import urllib.parse
import urllib.request
from pathlib import Path
from zoneinfo import ZoneInfo

ROOT = Path(__file__).resolve().parent
LEARNING_DIR = ROOT / "learning"
OUTCOMES_FILE = LEARNING_DIR / "outcomes.json"
METRICS_FILE = LEARNING_DIR / "metrics.json"
REPO = os.getenv("GITHUB_REPOSITORY", "ammarxz9333-cell/Auroraaudio")
TOKEN = os.getenv("GITHUB_TOKEN", "")
USER_AGENT = os.getenv("RADAR_USER_AGENT", "Ammar-Market-Radar-Learner/1.0")
NY = ZoneInfo("America/New_York")
MAX_ALERT_FILES = int(os.getenv("RADAR_LEARNING_MAX_ALERTS", "1000"))


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
        "Accept": "application/vnd.github+json",
    }
    if TOKEN and "api.github.com" in url:
        h["Authorization"] = f"Bearer {TOKEN}"
        h["X-GitHub-Api-Version"] = "2022-11-28"
    if headers:
        h.update(headers)
    req = urllib.request.Request(url, headers=h)
    with urllib.request.urlopen(req, timeout=15) as resp:
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
                "message": "chore(market-radar): refresh learning metrics",
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


def list_alert_paths():
    owner, repo = REPO.split("/", 1)
    quoted_ref = urllib.parse.quote("market-radar-live", safe="")
    data = github_json(
        f"/repos/{owner}/{repo}/contents/market-radar/live/alerts?ref={quoted_ref}"
    )
    if not isinstance(data, list):
        return []
    paths = [
        x.get("path")
        for x in data
        if x.get("type") == "file" and str(x.get("name", "")).endswith(".json")
    ]
    return sorted([p for p in paths if p])[-MAX_ALERT_FILES:]


def fetch_alert(path: str):
    owner, repo = REPO.split("/", 1)
    quoted_path = urllib.parse.quote(path, safe="/")
    quoted_ref = urllib.parse.quote("market-radar-live", safe="")
    data = github_json(
        f"/repos/{owner}/{repo}/contents/{quoted_path}?ref={quoted_ref}"
    )
    raw = base64.b64decode(data["content"]).decode("utf-8")
    return json.loads(raw)


def parse_utc(value):
    if not value:
        return None
    try:
        parsed = dt.datetime.fromisoformat(str(value).replace("Z", "+00:00"))
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=dt.timezone.utc)
        return parsed.astimezone(dt.timezone.utc)
    except Exception:
        return None


def yahoo_bars(ticker: str):
    url = (
        "https://query1.finance.yahoo.com/v8/finance/chart/"
        + urllib.parse.quote(ticker)
        + "?interval=5m&range=1mo&includePrePost=true&events=div%2Csplits"
    )
    data = request_json(url, headers={"Accept": "application/json"})
    result = ((data.get("chart") or {}).get("result") or [None])[0]
    if not result:
        return []
    stamps = result.get("timestamp") or []
    quote = (((result.get("indicators") or {}).get("quote") or [{}])[0]) or {}
    closes = quote.get("close") or []
    bars = []
    for i, ts in enumerate(stamps):
        close = closes[i] if i < len(closes) else None
        if close is None:
            continue
        when = dt.datetime.fromtimestamp(ts, tz=dt.timezone.utc)
        bars.append({"ts": when, "close": float(close)})
    return bars


def first_bar_at_or_after(bars, target):
    for bar in bars:
        if bar["ts"] >= target:
            return bar
    return None


def pct(a, b):
    if a in (None, 0) or b is None:
        return None
    return ((b / a) - 1.0) * 100.0


def window_extremes(bars, entry_time, entry_price, hours):
    end = entry_time + dt.timedelta(hours=hours)
    vals = [b["close"] for b in bars if entry_time <= b["ts"] <= end]
    if not vals or not entry_price:
        return None, None
    mfe = ((max(vals) / entry_price) - 1.0) * 100.0
    mae = ((min(vals) / entry_price) - 1.0) * 100.0
    return mfe, mae


def regular_session_closes(bars, start_time):
    by_day = {}
    for bar in bars:
        local = bar["ts"].astimezone(NY)
        minute = local.hour * 60 + local.minute
        if 9 * 60 + 30 <= minute < 16 * 60 and bar["ts"] >= start_time:
            by_day.setdefault(local.date().isoformat(), []).append(bar)
    closes = []
    for day in sorted(by_day):
        closes.append(by_day[day][-1])
    return closes


def rounded(value):
    return round(value, 4) if isinstance(value, (int, float)) else value


def benchmark_returns(bench_bars, detected, horizon_minutes):
    if not bench_bars:
        return {}
    entry = first_bar_at_or_after(bench_bars, detected)
    if not entry:
        return {}
    out = {}
    for label, minutes in horizon_minutes:
        bar = first_bar_at_or_after(bench_bars, detected + dt.timedelta(minutes=minutes))
        out[label] = rounded(pct(entry["close"], bar["close"])) if bar else None
    return out


def evaluate_observation(meta, ticker, bars, spy_bars=None, qqq_bars=None):
    detected = parse_utc(meta.get("detected_utc"))
    if not detected or not bars:
        return None

    entry = first_bar_at_or_after(bars, detected)
    if not entry:
        return None

    entry_price = entry["close"]
    horizon_minutes = (
        ("5m", 5),
        ("30m", 30),
        ("1h", 60),
        ("24h", 1440),
        ("48h", 2880),
        ("3d", 4320),
        ("5d", 7200),
        ("10d", 14400),
    )
    spy_returns = benchmark_returns(spy_bars or [], detected, horizon_minutes)
    qqq_returns = benchmark_returns(qqq_bars or [], detected, horizon_minutes)

    horizons = {}
    for label, minutes in horizon_minutes:
        target = detected + dt.timedelta(minutes=minutes)
        bar = first_bar_at_or_after(bars, target)
        stock_return = pct(entry_price, bar["close"]) if bar else None
        spy_return = spy_returns.get(label)
        qqq_return = qqq_returns.get(label)
        horizons[label] = {
            "price": rounded(bar["close"]) if bar else None,
            "return_pct": rounded(stock_return),
            "spy_return_pct": rounded(spy_return),
            "qqq_return_pct": rounded(qqq_return),
            "alpha_vs_spy_pct": rounded(stock_return - spy_return)
            if stock_return is not None and spy_return is not None else None,
            "alpha_vs_qqq_pct": rounded(stock_return - qqq_return)
            if stock_return is not None and qqq_return is not None else None,
            "bar_utc": bar["ts"].isoformat() if bar else None,
        }

    mfe_24h, mae_24h = window_extremes(bars, entry["ts"], entry_price, 24)
    mfe_48h, mae_48h = window_extremes(bars, entry["ts"], entry_price, 48)
    closes = regular_session_closes(bars, detected)
    close_1 = closes[0] if closes else None
    close_2 = closes[1] if len(closes) > 1 else None

    age_hours = (now_utc() - detected).total_seconds() / 3600.0
    return {
        "radar_id": meta.get("radar_id"),
        "ticker": ticker,
        "detected_utc": detected.isoformat(),
        "published_utc": meta.get("published_utc"),
        "stage": meta.get("stage"),
        "score": meta.get("score"),
        "direction": meta.get("direction"),
        "source": meta.get("source"),
        "source_class": meta.get("source_class"),
        "signals": meta.get("signals") or [],
        "headline": meta.get("headline"),
        "url": meta.get("url"),
        "entry": {
            "price": rounded(entry_price),
            "bar_utc": entry["ts"].isoformat(),
        },
        "horizons": horizons,
        "regular_close_1": {
            "price": rounded(close_1["close"]) if close_1 else None,
            "return_pct": rounded(pct(entry_price, close_1["close"])) if close_1 else None,
            "bar_utc": close_1["ts"].isoformat() if close_1 else None,
        },
        "regular_close_2": {
            "price": rounded(close_2["close"]) if close_2 else None,
            "return_pct": rounded(pct(entry_price, close_2["close"])) if close_2 else None,
            "bar_utc": close_2["ts"].isoformat() if close_2 else None,
        },
        "mfe_24h_pct": rounded(mfe_24h),
        "mae_24h_pct": rounded(mae_24h),
        "mfe_48h_pct": rounded(mfe_48h),
        "mae_48h_pct": rounded(mae_48h),
        "mature_24h": age_hours >= 24,
        "mature_48h": age_hours >= 48,
        "mature_3d": age_hours >= 72,
        "mature_5d": age_hours >= 120,
        "mature_10d": age_hours >= 240,
        "evaluated_utc": now_utc().isoformat(),
    }


def safe_mean(values):
    vals = [x for x in values if isinstance(x, (int, float))]
    return round(statistics.mean(vals), 3) if vals else None


def rate(rows, predicate):
    if not rows:
        return None
    return round(100.0 * sum(1 for x in rows if predicate(x)) / len(rows), 2)


def breakdown(rows, key_fn):
    groups = {}
    for row in rows:
        key = str(key_fn(row) or "unknown")
        groups.setdefault(key, []).append(row)
    out = {}
    for key, vals in sorted(groups.items()):
        mature = [v for v in vals if v.get("mature_24h")]
        out[key] = {
            "n": len(vals),
            "mature_24h": len(mature),
            "hit_5pct_24h_rate": rate(mature, lambda x: (x.get("mfe_24h_pct") or -999) >= 5),
            "adverse_5pct_24h_rate": rate(mature, lambda x: (x.get("mae_24h_pct") or 999) <= -5),
            "avg_mfe_24h_pct": safe_mean([x.get("mfe_24h_pct") for x in mature]),
            "avg_mae_24h_pct": safe_mean([x.get("mae_24h_pct") for x in mature]),
        }
    return out


def score_band(row):
    try:
        score = float(row.get("score") or 0)
    except Exception:
        score = 0
    if score >= 14:
        return "14+"
    if score >= 12:
        return "12-13"
    if score >= 10:
        return "10-11"
    return "8-9"


def build_metrics(rows):
    mature24 = [r for r in rows if r.get("mature_24h")]
    mature48 = [r for r in rows if r.get("mature_48h")]
    mature3d = [r for r in rows if r.get("mature_3d")]
    mature5d = [r for r in rows if r.get("mature_5d")]
    mature10d = [r for r in rows if r.get("mature_10d")]
    high24 = [r for r in mature24 if float(r.get("score") or 0) >= 10]

    def horizon_mean(sample, horizon, field):
        return safe_mean([
            ((r.get("horizons") or {}).get(horizon) or {}).get(field)
            for r in sample
        ])

    return {
        "generated_utc": now_utc().isoformat(),
        "definition": {
            "entry": "first available 5-minute bar at or after alert detection",
            "hit_5pct_24h": "MFE >= +5% during first 24 elapsed hours",
            "hit_10pct_48h": "MFE >= +10% during first 48 elapsed hours",
            "adverse_5pct_24h": "MAE <= -5% during first 24 elapsed hours",
            "high_confidence_tracking": "score >= 10; descriptive only until sample size is adequate",
            "relative_strength": "stock forward return minus SPY/QQQ return over the same timestamp-aligned horizon",
        },
        "overall": {
            "observations": len(rows),
            "mature_24h": len(mature24),
            "mature_48h": len(mature48),
            "hit_5pct_24h_rate": rate(mature24, lambda x: (x.get("mfe_24h_pct") or -999) >= 5),
            "hit_10pct_48h_rate": rate(mature48, lambda x: (x.get("mfe_48h_pct") or -999) >= 10),
            "adverse_5pct_24h_rate": rate(mature24, lambda x: (x.get("mae_24h_pct") or 999) <= -5),
            "avg_mfe_24h_pct": safe_mean([x.get("mfe_24h_pct") for x in mature24]),
            "avg_mae_24h_pct": safe_mean([x.get("mae_24h_pct") for x in mature24]),
            "avg_24h_return_pct": horizon_mean(mature24, "24h", "return_pct"),
            "avg_24h_alpha_vs_spy_pct": horizon_mean(mature24, "24h", "alpha_vs_spy_pct"),
            "avg_24h_alpha_vs_qqq_pct": horizon_mean(mature24, "24h", "alpha_vs_qqq_pct"),
            "avg_3d_return_pct": horizon_mean(mature3d, "3d", "return_pct"),
            "avg_3d_alpha_vs_spy_pct": horizon_mean(mature3d, "3d", "alpha_vs_spy_pct"),
            "avg_5d_return_pct": horizon_mean(mature5d, "5d", "return_pct"),
            "avg_5d_alpha_vs_spy_pct": horizon_mean(mature5d, "5d", "alpha_vs_spy_pct"),
            "avg_10d_return_pct": horizon_mean(mature10d, "10d", "return_pct"),
            "avg_10d_alpha_vs_spy_pct": horizon_mean(mature10d, "10d", "alpha_vs_spy_pct"),
        },
        "high_confidence_score_10_plus": {
            "mature_24h": len(high24),
            "hit_5pct_24h_rate": rate(high24, lambda x: (x.get("mfe_24h_pct") or -999) >= 5),
            "adverse_5pct_24h_rate": rate(high24, lambda x: (x.get("mae_24h_pct") or 999) <= -5),
            "avg_mfe_24h_pct": safe_mean([x.get("mfe_24h_pct") for x in high24]),
            "avg_mae_24h_pct": safe_mean([x.get("mae_24h_pct") for x in high24]),
        },
        "by_score_band": breakdown(rows, score_band),
        "by_stage": breakdown(rows, lambda x: x.get("stage")),
        "by_direction": breakdown(rows, lambda x: x.get("direction")),
        "by_source_class": breakdown(rows, lambda x: x.get("source_class")),
        "by_source": breakdown(rows, lambda x: x.get("source")),
    }


def main():
    existing = load_json(OUTCOMES_FILE, {"observations": []})
    old_rows = existing.get("observations", [])
    rows_by_key = {
        f"{r.get('radar_id')}::{r.get('ticker')}": r
        for r in old_rows
        if r.get("radar_id") and r.get("ticker")
    }

    alert_paths = list_alert_paths()
    alerts = []
    known_ids = {r.get("radar_id") for r in old_rows if r.get("radar_id")}

    for path in alert_paths:
        try:
            meta = fetch_alert(path)
        except Exception as exc:
            print(f"alert-fetch-error {path}: {type(exc).__name__}: {exc}")
            continue
        rid = meta.get("radar_id")
        tickers = meta.get("tickers") or []
        if not rid or not tickers:
            continue
        if rid not in known_ids:
            alerts.append(meta)
        else:
            # Keep metadata for immature observations so recent horizons are refreshed.
            if any(
                not rows_by_key.get(f"{rid}::{ticker}", {}).get("mature_48h")
                for ticker in tickers
            ):
                alerts.append(meta)

    ticker_cache = {}
    benchmark_cache = {}
    for benchmark in ("SPY", "QQQ"):
        try:
            benchmark_cache[benchmark] = yahoo_bars(benchmark)
        except Exception as exc:
            print(f"benchmark-data-error {benchmark}: {type(exc).__name__}: {exc}")
            benchmark_cache[benchmark] = []

    for meta in alerts:
        for ticker in meta.get("tickers") or []:
            if ticker not in ticker_cache:
                try:
                    ticker_cache[ticker] = yahoo_bars(ticker)
                except Exception as exc:
                    print(f"market-data-error {ticker}: {type(exc).__name__}: {exc}")
                    ticker_cache[ticker] = []

            row = evaluate_observation(
                meta,
                ticker,
                ticker_cache[ticker],
                spy_bars=benchmark_cache.get("SPY"),
                qqq_bars=benchmark_cache.get("QQQ"),
            )
            if row:
                rows_by_key[f"{row['radar_id']}::{ticker}"] = row

    rows = sorted(
        rows_by_key.values(),
        key=lambda r: (r.get("detected_utc", ""), r.get("radar_id", ""), r.get("ticker", "")),
    )
    outcomes_obj = {
        "updated_utc": now_utc().isoformat(),
        "observations": rows,
    }
    metrics_obj = build_metrics(rows)
    save_json(OUTCOMES_FILE, outcomes_obj)
    save_json(METRICS_FILE, metrics_obj)

    remote_commits = {}
    try:
        remote_commits["outcomes"] = persist_remote_json(
            "market-radar/learning/outcomes.json", outcomes_obj
        )
        remote_commits["metrics"] = persist_remote_json(
            "market-radar/learning/metrics.json", metrics_obj
        )
    except Exception as exc:
        remote_commits["error"] = f"{type(exc).__name__}: {exc}"

    print(json.dumps({
        "alert_files_seen": len(alert_paths),
        "observations": len(rows),
        "tickers_refreshed": len(ticker_cache),
        "benchmarks_refreshed": sorted(benchmark_cache),
        "remote_commits": remote_commits,
    }, indent=2))


if __name__ == "__main__":
    main()
