#!/usr/bin/env python3
"""Fetch actual Yahoo Finance chart candles. Fail closed: never synthesize bars."""
import datetime as dt
import json
import os
import pathlib
import urllib.error
import urllib.parse
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent
SYMBOLS = ("CRWV", "APLD", "SOFI", "QQQ", "SPY", "GLD")
CONFIGS = {"intraday": ("5d", "5m"), "daily": ("6mo", "1d")}

def fetch(symbol, range_, interval):
    url = "https://query1.finance.yahoo.com/v8/finance/chart/" + urllib.parse.quote(symbol) + "?" + urllib.parse.urlencode({"range": range_, "interval": interval})
    request = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 (compatible; MarketRadar/1.0)", "Accept": "application/json"})
    with urllib.request.urlopen(request, timeout=20) as response:
        payload = json.load(response)
    if payload.get("chart", {}).get("error"):
        raise ValueError(payload["chart"]["error"])
    result = (payload.get("chart", {}).get("result") or [None])[0]
    if not result:
        raise ValueError("Missing chart result")
    times = result.get("timestamp") or []
    q = (result.get("indicators", {}).get("quote") or [None])[0]
    if not q:
        raise ValueError("Missing OHLC")
    bars = []
    for i, stamp in enumerate(times):
        values = [q[key][i] for key in ("open", "high", "low", "close")]
        if any(v is None or not isinstance(v, (int, float)) for v in values):
            continue
        o, h, l, c = map(float, values)
        if l <= 0 or h < l or not (l <= o <= h and l <= c <= h):
            continue
        bars.append({"t": int(stamp), "o": o, "h": h, "l": l, "c": c})
    if len(bars) < 12:
        raise ValueError("Insufficient valid candles")
    return bars

def main():
    output = ROOT / "data"
    output.mkdir(exist_ok=True)
    failed = []
    for symbol in SYMBOLS:
        try:
            series = {key: fetch(symbol, *args) for key, args in CONFIGS.items()}
            result = {"symbol": symbol, "source": "Yahoo Finance chart API (unofficial; may be delayed)", "fetched_at": dt.datetime.now(dt.timezone.utc).isoformat(), **series}
            target = output / (symbol + ".json")
            tmp = target.with_suffix(".tmp")
            tmp.write_text(json.dumps(result, separators=(",", ":")) + "\n", encoding="utf-8")
            os.replace(tmp, target)
            print(symbol, "updated", len(series["intraday"]), len(series["daily"]))
        except (urllib.error.URLError, ValueError, KeyError, IndexError, TimeoutError) as exc:
            failed.append(symbol)
            print(symbol, "FAILED, keeping last valid snapshot:", str(exc))
    if failed:
        print("Failed:", ", ".join(failed))
    if len(failed) == len(SYMBOLS):
        raise SystemExit("All feeds unavailable; no fabricated data written.")

if __name__ == "__main__":
    main()
