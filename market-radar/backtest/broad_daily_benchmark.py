#!/usr/bin/env python3
"""Predeclared, chronological daily-bar benchmark across a sampled US universe.

This is price/volume research, not a historical news-catalyst or executable
bid/ask backtest. The current Nasdaq directory creates survivorship bias.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import csv
import datetime as dt
import hashlib
import io
import json
import math
import statistics
import time
import urllib.parse
import urllib.request
from collections import Counter, defaultdict
from pathlib import Path

DIRECTORY = "https://www.nasdaqtrader.com/dynamic/SymDir/nasdaqtraded.txt"
CHART = "https://query2.finance.yahoo.com/v8/finance/chart/"
USER_AGENT = "MarketRadar-BroadPIT-Research/1.0"
SEED = "market-radar-broad-v1"
HORIZON = 5
ASSUMED_SPREAD_PCT = 0.5
ASSUMED_SLIPPAGE_PCT = 0.1
EXCHANGE_QUOTAS = {"Q": 100, "N": 60, "A": 20}


def fetch(url, attempts=3):
    for attempt in range(attempts):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, "Accept": "application/json,text/plain,*/*"})
            with urllib.request.urlopen(req, timeout=20) as resp:
                return resp.read()
        except Exception:
            if attempt + 1 == attempts:
                raise
            time.sleep(1.5 * (attempt + 1))


def directory_sample(raw, quotas=EXCHANGE_QUOTAS):
    text = raw.decode("utf-8-sig", errors="replace")
    groups = defaultdict(list)
    for row in csv.DictReader(io.StringIO(text), delimiter="|"):
        symbol = (row.get("Symbol") or "").strip()
        name = (row.get("Security Name") or "").lower()
        exchange = row.get("Listing Exchange", "")
        if (exchange not in quotas or row.get("ETF") != "N" or row.get("Test Issue") != "N"
                or not symbol.isalpha() or not 2 <= len(symbol) <= 5
                or not any(k in name for k in ("common stock", "ordinary shares", "american depositary"))
                or any(k in name for k in ("warrant", "unit ", "preferred", "right to"))):
            continue
        groups[exchange].append(symbol)
    selected = []
    for exchange, quota in quotas.items():
        ranked = sorted(set(groups[exchange]), key=lambda s: hashlib.sha256(f"{SEED}:{exchange}:{s}".encode()).hexdigest())
        selected.extend(ranked[:quota])
    return selected, {k: len(set(groups[k])) for k in quotas}


def daily_bars(payload):
    result = json.loads(payload)["chart"]["result"][0]
    stamps = result.get("timestamp") or []
    q = result["indicators"]["quote"][0]
    out = []
    for i, stamp in enumerate(stamps):
        vals = {k: (q.get(k) or [None] * len(stamps))[i] for k in ("open", "high", "low", "close", "volume")}
        if any(vals[k] is None for k in ("open", "high", "low", "close")):
            continue
        out.append({"date": dt.datetime.fromtimestamp(stamp, dt.timezone.utc).date().isoformat(),
                    **{k: float(vals[k]) for k in ("open", "high", "low", "close")},
                    "volume": int(vals["volume"] or 0)})
    return out


def download_symbol(symbol, range_value="2y"):
    query = urllib.parse.urlencode({"range": range_value, "interval": "1d", "includePrePost": "false", "events": "div,splits"})
    return daily_bars(fetch(CHART + urllib.parse.quote(symbol) + "?" + query))


def median_or_none(values):
    return statistics.median(values) if values else None


def signal_features(bars, index, spy_by_date, qqq_by_date):
    """Only bars up to and including index and contemporaneous benchmarks."""
    if index < 20 or index + HORIZON >= len(bars):
        return None
    now = bars[index]
    prior = bars[index-20:index]
    close, volume = now["close"], now["volume"]
    if close < 2 or now["open"] <= 0:
        return None
    median_volume = median_or_none([b["volume"] for b in prior])
    median_dollars = median_or_none([b["close"] * b["volume"] for b in prior])
    if not median_volume or not median_dollars or median_dollars < 2_000_000:
        return None
    spy = spy_by_date.get(now["date"])
    qqq = qqq_by_date.get(now["date"])
    if not spy or not qqq or spy.get("sma20") is None or spy.get("ret5") is None:
        return None
    high20 = max(b["high"] for b in prior)
    rng = now["high"] - now["low"]
    momentum5 = (close / bars[index-5]["close"] - 1) * 100
    ret20 = (close / bars[index-20]["close"] - 1) * 100
    rvol = volume / median_volume
    location = (close - now["low"]) / rng if rng > 0 else 0.5
    relative5 = momentum5 - spy["ret5"]
    market_ok = spy["close"] >= spy["sma20"]
    # Fixed before seeing validation or final outcomes. No parameter search.
    signal = (market_ok and rvol >= 2 and 2 <= momentum5 <= 25
              and close > high20 and location >= 0.75 and relative5 >= 2)
    return {"date": now["date"], "close": close, "rvol": round(rvol, 3),
            "momentum5_pct": round(momentum5, 3), "return20_pct": round(ret20, 3),
            "relative5_vs_spy_pct": round(relative5, 3),
            "close_location": round(location, 3),
            "breakout20": close > high20, "spy_above_sma20": market_ok,
            "qqq_close": qqq["close"], "signal": signal,
            "rank_score": round(min(rvol, 10) + relative5 / 5 + location, 4)}


def benchmark_index(bars):
    return {b["date"]: {**b,
            "sma20": statistics.mean(x["close"] for x in bars[i-19:i+1]) if i >= 19 else None,
            "ret5": (b["close"] / bars[i-5]["close"] - 1) * 100 if i >= 5 else None}
            for i, b in enumerate(bars)}


def outcome(bars, index, spread_pct=ASSUMED_SPREAD_PCT, slippage_pct=ASSUMED_SLIPPAGE_PCT):
    """Fill next session open; same daily candle hitting both barriers is ambiguous."""
    if index + HORIZON >= len(bars):
        return None
    entry = bars[index+1]["open"] * (1 + spread_pct / 200 + slippage_pct / 100)
    if entry <= 0:
        return None
    first = "UNRESOLVED"
    first_date = None
    high = entry
    low = entry
    for bar in bars[index+1:index+HORIZON+1]:
        high = max(high, bar["high"])
        low = min(low, bar["low"])
        if first == "UNRESOLVED":
            plus, minus = bar["high"] >= entry * 1.05, bar["low"] <= entry * 0.95
            if plus or minus:
                first = "ORDER_UNVERIFIED" if plus and minus else "PLUS5_FIRST" if plus else "MINUS5_FIRST"
                first_date = bar["date"]
    final_close = bars[index+HORIZON]["close"]
    return {"entry_date": bars[index+1]["date"], "entry_price": round(entry, 4),
            "outcome": first, "outcome_date": first_date,
            "return_5d_pct": round((final_close / entry - 1) * 100, 3),
            "mfe_pct": round((high / entry - 1) * 100, 3),
            "mae_pct": round((low / entry - 1) * 100, 3)}


def chronological_labels(dates, embargo_days=7):
    dates = sorted(set(dates))
    if len(dates) < 60:
        raise ValueError("insufficient distinct trading days for chronological holdout")
    train_end = dates[int(len(dates) * 0.6) - 1]
    valid_end = dates[int(len(dates) * 0.8) - 1]
    embargo = dt.timedelta(days=embargo_days)
    labels = {}
    for date in dates:
        day = dt.date.fromisoformat(date)
        if date <= train_end:
            labels[date] = "development"
        elif day <= dt.date.fromisoformat(train_end) + embargo:
            labels[date] = "embargo"
        elif date <= valid_end:
            labels[date] = "validation"
        elif day <= dt.date.fromisoformat(valid_end) + embargo:
            labels[date] = "embargo"
        else:
            labels[date] = "final_test"
    return labels, {"development_end": train_end, "validation_end": valid_end, "embargo_days": embargo_days}


def summarize(rows):
    positives = [r for r in rows if r["outcome"] == "PLUS5_FIRST"]
    negatives = [r for r in rows if r["outcome"] == "MINUS5_FIRST"]
    ambiguous = [r for r in rows if r["outcome"] == "ORDER_UNVERIFIED"]
    returns = [r["return_5d_pct"] for r in rows]
    n = len(rows)
    p = len(positives) / n if n else 0
    z = 1.96
    denom = 1 + z*z/n if n else 1
    half = z*math.sqrt((p*(1-p)+z*z/(4*n))/n)/denom if n else 0
    center = (p+z*z/(2*n))/denom if n else 0
    return {"n": len(rows), "plus5_first": len(positives), "minus5_first": len(negatives),
            "order_unverified": len(ambiguous), "unresolved": len(rows)-len(positives)-len(negatives)-len(ambiguous),
            "plus5_first_pct": round(100*len(positives)/len(rows), 2) if rows else None,
            "plus5_first_wilson_95_pct": [round(100*(center-half), 2), round(100*(center+half), 2)] if rows else None,
            "mean_5d_return_pct": round(statistics.mean(returns), 3) if returns else None,
            "median_5d_return_pct": round(statistics.median(returns), 3) if returns else None}


def select_weekly(rows, max_per_week=2):
    """At each day's close select only from that day, with no later-week ranking."""
    by_date = defaultdict(list)
    for row in rows:
        if row["signal"]:
            by_date[row["date"]].append(row)
    selected = []
    used = Counter()
    for date in sorted(by_date):
        week = dt.date.fromisoformat(date).isocalendar()[:2]
        remaining = max_per_week - used[week]
        if remaining <= 0:
            continue
        candidates = sorted(by_date[date], key=lambda r: (-r["rank_score"], r["ticker"]))
        chosen = candidates[:remaining]
        selected.extend(chosen)
        used[week] += len(chosen)
    return selected


def feature_diagnostics(rows):
    keys = ("rvol", "momentum5_pct", "return20_pct", "relative5_vs_spy_pct", "close_location")
    out = {}
    for group, subset in (("plus5_first", [r for r in rows if r["outcome"] == "PLUS5_FIRST"]),
                          ("minus5_first", [r for r in rows if r["outcome"] == "MINUS5_FIRST"]),
                          ("all_eligible", rows)):
        out[group] = {"n": len(subset), **{k: round(statistics.median(r[k] for r in subset), 3) if subset else None for k in keys}}
    return out


def missed_winner_diagnostics(rows):
    winners = [r for r in rows if r["outcome"] == "PLUS5_FIRST" and not r["signal"]]
    blockers = Counter()
    examples = []
    for row in winners:
        reasons = []
        if not row["spy_above_sma20"]: reasons.append("market_regime")
        if row["rvol"] < 2: reasons.append("volume_below_2x")
        if not 2 <= row["momentum5_pct"] <= 25: reasons.append("momentum_outside_range")
        if not row["breakout20"]: reasons.append("no_20day_breakout")
        if row["close_location"] < 0.75: reasons.append("weak_close_location")
        if row["relative5_vs_spy_pct"] < 2: reasons.append("weak_relative_strength")
        blockers.update(reasons)
        if len(examples) < 20:
            examples.append({"ticker": row["ticker"], "date": row["date"],
                             "outcome_date": row["outcome_date"], "blockers": reasons})
    return {"missed_winners": len(winners), "blocker_counts_nonexclusive": dict(blockers),
            "examples": examples, "interpretation": "Why the predeclared rule rejected later winners; not the cause of their price rise"}


def run(universe_raw, prices, spy, qqq, requested):
    symbols, pool = directory_sample(universe_raw)
    spy_map, qqq_map = benchmark_index(spy), benchmark_index(qqq)
    common_dates = sorted(set(spy_map) & set(qqq_map))
    labels, boundaries = chronological_labels(common_dates)
    rows = []
    failures = {}
    for ticker in symbols:
        bars = prices.get(ticker)
        if not bars or len(bars) < 80:
            failures[ticker] = "missing or short daily history"
            continue
        for i in range(20, len(bars)-HORIZON):
            feat = signal_features(bars, i, spy_map, qqq_map)
            if not feat or labels.get(feat["date"]) in (None, "embargo"):
                continue
            result = outcome(bars, i)
            if result:
                rows.append({"ticker": ticker, **feat, **result, "partition": labels[feat["date"]]})
    report = {"schema_version": 1, "generated_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
              "method": {"universe": "deterministic sample of CURRENT Nasdaq Trader listings; survivorship bias",
                         "selection_seed": SEED, "quotas": EXCHANGE_QUOTAS,
                         "signal": "predeclared volume/breakout/relative-strength rule at daily close",
                         "fill": "next regular daily open + assumed half-spread and 0.1% slippage",
                         "target": "first +5% or -5% in five subsequent daily bars; same-day both ambiguous",
                         "market_data": "Yahoo chart daily OHLCV, historical revisions/splits possible",
                         "catalysts": "not included; cannot infer causal news drivers",
                         "boundaries": boundaries},
              "coverage": {"directory_pool": pool, "sampled_symbols": len(symbols),
                           "with_history": len(symbols)-len(failures), "failed_symbols": failures,
                           "eligible_symbol_days": len(rows), "requested": requested},
              "partitions": {}, "diagnostics": feature_diagnostics(rows)}
    for part in ("development", "validation", "final_test"):
        subset = [r for r in rows if r["partition"] == part]
        signals = [r for r in subset if r["signal"]]
        weekly = select_weekly(subset)
        eligible_winners = sum(r["outcome"] == "PLUS5_FIRST" for r in subset)
        report["partitions"][part] = {
            "all_eligible": summarize(subset), "all_signals": summarize(signals),
            "max_two_weekly": summarize(weekly),
            "weekly_signal_count": len(weekly),
            "missed_winners": missed_winner_diagnostics(subset),
            "false_positive_examples": [{"ticker": r["ticker"], "date": r["date"],
                "entry_date": r["entry_date"], "outcome_date": r["outcome_date"],
                "rvol": r["rvol"], "relative5_vs_spy_pct": r["relative5_vs_spy_pct"]}
                for r in weekly if r["outcome"] == "MINUS5_FIRST"][:20],
            "recall_of_eligible_plus5_pct": round(100*sum(r["outcome"] == "PLUS5_FIRST" for r in weekly)/eligible_winners, 3) if eligible_winners else None,
            "by_market_regime": {
                "spy_above_sma20": summarize([r for r in subset if r["spy_above_sma20"]]),
                "spy_below_sma20": summarize([r for r in subset if not r["spy_above_sma20"]])},
        }
    return report, rows, symbols


def markdown(report):
    c = report["coverage"]
    lines = ["# Market Radar broad daily benchmark", "",
        f"Sample: {c['with_history']}/{c['sampled_symbols']} current-listed symbols; {c['eligible_symbol_days']} eligible symbol-days.",
        "", "| Chronological cohort | All eligible | Raw signals | First two per week | +5% first | -5% first |",
        "|---|---:|---:|---:|---:|---:|"]
    for part, data in report["partitions"].items():
        s = data["max_two_weekly"]
        lines.append(f"| {part} | {data['all_eligible']['n']} | {data['all_signals']['n']} | {s['n']} | {s['plus5_first']} | {s['minus5_first']} |")
    lines += ["", "Daily OHLC cannot order a target and stop inside the same candle. Those cases count as unverified.",
              "The directory is a current-listing sample, so delisted historical stocks are absent. This is not a survivorship-free or historical-news backtest.",
              "The rule was fixed before the holdout was inspected. No guaranteed hit rate follows from these data.", ""]
    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out-dir", required=True)
    ap.add_argument("--workers", type=int, default=5)
    ap.add_argument("--min-history-coverage", type=float, default=0.7)
    args = ap.parse_args()
    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)
    raw = fetch(DIRECTORY)
    symbols, _ = directory_sample(raw)
    spy = download_symbol("SPY")
    qqq = download_symbol("QQQ")
    prices = {}
    errors = {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        tasks = {pool.submit(download_symbol, symbol): symbol for symbol in symbols}
        for future in concurrent.futures.as_completed(tasks):
            symbol = tasks[future]
            try:
                prices[symbol] = future.result()
            except Exception as exc:
                errors[symbol] = f"{type(exc).__name__}: {exc}"
    report, rows, selected = run(raw, prices, spy, qqq, len(symbols))
    report["coverage"]["download_errors"] = errors
    (out / "report.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    (out / "report.md").write_text(markdown(report))
    (out / "universe.json").write_text(json.dumps({"selected": selected, "directory_sha256": hashlib.sha256(raw).hexdigest(),
        "directory_fetched_utc": report["generated_utc"]}, indent=2) + "\n")
    print(markdown(report))
    if report["coverage"]["with_history"] < args.min_history_coverage * len(selected):
        raise SystemExit("insufficient source coverage; do not interpret performance")


if __name__ == "__main__":
    main()
