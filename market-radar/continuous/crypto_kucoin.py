"""Independent, read-only KuCoin spot-reference CRYPTO WATCH layer for Market Radar.

Not Overkill v2, not a derivatives signal, not a short order. Uses completed
one-hour candles and public KuCoin spot liquidity. No calibrated probabilities.
"""
from __future__ import annotations

import asyncio
import math
import re
from datetime import datetime, timezone

KLINE_URL = "https://api.kucoin.com/api/v1/market/candles"
TICKERS_URL = "https://api.kucoin.com/api/v1/market/allTickers"
HOUR = 3600
MIN_BARS = 300
SYMBOL_RE = re.compile(r"^[A-Z0-9]{2,20}-USDT$")
STABLE_BASES = {"USDC", "USDT", "DAI", "FDUSD", "TUSD", "USDD", "PYUSD", "EURC"}


def _unavailable(symbol, reason):
    return {"symbol": symbol, "status": "UNAVAILABLE", "reason": reason,
            "score": None, "actionable": False, "automated_orders": False}


def _ema(prices, period):
    weight = 2.0 / (period + 1.0)
    current = prices[0]
    for price in prices[1:]:
        current = weight * price + (1 - weight) * current
    return current


def classify_hourly(symbol, raw, asof, liquidity=None):
    """Score a COMPLETE, consecutive 1h OHLCV sequence, fail closed.

    KuCoin REST candle layout: start, open, close, high, low, base volume,
    quote turnover. Last row may still be forming; always discard it.
    All scoring features use only already-closed candles.
    """
    if not isinstance(symbol, str) or not SYMBOL_RE.fullmatch(symbol):
        return _unavailable(str(symbol)[:32], "unsupported_symbol")
    if not isinstance(raw, list):
        return _unavailable(symbol, "missing_candles")
    try:
        asof = int(asof)
        candles = []
        starts_seen = set()
        for row in raw:
            if not isinstance(row, (list, tuple)) or len(row) < 6:
                raise ValueError("malformed_candle")
            start = int(row[0])
            op, close, high, low, volume = (float(row[i]) for i in (1, 2, 3, 4, 5))
            if (start % HOUR != 0 or start in starts_seen or
                    not all(math.isfinite(x) for x in (op, close, high, low, volume)) or
                    min(op, close, high, low) <= 0 or volume < 0 or
                    low > min(op, close) or high < max(op, close)):
                raise ValueError("invalid_or_duplicate_candle")
            starts_seen.add(start)
            if start + HOUR <= asof:   # never use the forming candle
                candles.append((start, close, volume))
        candles.sort()
        if len(candles) < MIN_BARS:
            return _unavailable(symbol, "insufficient_completed_1h_history")
        candles = candles[-MIN_BARS:]
        if any(b[0] - a[0] != HOUR for a, b in zip(candles, candles[1:])):
            return _unavailable(symbol, "missing_hourly_candles")
        last_close = candles[-1][0] + HOUR
        if not (0 <= asof - last_close <= 2 * HOUR):
            return _unavailable(symbol, "stale_hourly_candles")
        prices = [r[1] for r in candles]
        volumes = [r[2] for r in candles]
        if not all(v > 0 for v in volumes[-21:]):
            return _unavailable(symbol, "missing_volume")
        baseline = sum(volumes[-21:-1]) / 20
        ratio = volumes[-1] / baseline
        if not math.isfinite(ratio):
            return _unavailable(symbol, "invalid_volume_ratio")
        close = prices[-1]
        e20, e50, e200 = (_ema(prices, n) for n in (20, 50, 200))
        move_6h = (close / prices[-7] - 1) * 100
        move_1h = (close / prices[-2] - 1) * 100
        bullish = close > e20 > e50 > e200
        bearish = close < e20 < e50 < e200
        direction = "LONG" if bullish else "SHORT" if bearish else None
        momentum = ((move_6h >= 1) if direction == "LONG" else
                    (move_6h <= -1) if direction == "SHORT" else False)
        volume_ok = ratio >= 1.25
        score = (50 if direction else 0) + (25 if momentum else 0) + (
            25 if volume_ok else 0)
        extended = abs(move_1h) >= 8 or abs(move_6h) >= 15
        status = ("EXTENDED_WAIT" if extended and direction else
                  f"{direction}_WATCH" if direction and momentum and volume_ok
                  else "NO_SETUP")
        liquidity = liquidity if isinstance(liquidity, dict) else {}
        return {
            "symbol": symbol, "market": "CRYPTO", "venue": "KuCoin",
            "instrument": "spot_reference_only", "interval": "1hour",
            "status": status, "score": score,
            "signal_date_utc": datetime.fromtimestamp(last_close, timezone.utc).isoformat(),
            "observed_at_utc": datetime.fromtimestamp(asof, timezone.utc).isoformat(),
            "close": close, "ema20": round(e20, 8), "ema50": round(e50, 8),
            "ema200": round(e200, 8), "return_6h_pct": round(move_6h, 3),
            "return_1h_pct": round(move_1h, 3),
            "volume_ratio_20h": round(ratio, 3),
            "turnover_24h_usdt": liquidity.get("turnover_24h_usdt"),
            "spread_pct": liquidity.get("spread_pct"),
            "evidence": {"trend_aligned": bool(direction),
                         "momentum_6h": bool(momentum),
                         "volume_confirmation": bool(volume_ok),
                         "extended": bool(extended)},
            "actionable": False, "automated_orders": False,
            "is_calibrated_probability": False,
            "note": "Research WATCH only. Spot candle trends do not establish "
                    "futures eligibility, borrow availability, or executable prices."
        }
    except (ValueError, TypeError, OverflowError, IndexError, ZeroDivisionError):
        return _unavailable(symbol, "malformed_or_inconsistent_market_data")


def select_liquid_usdt(tickers, top_n=30):
    """Select bounded liquid KuCoin spot pairs; NOT all of crypto."""
    if not isinstance(tickers, list):
        return []
    selected = []
    for row in tickers:
        try:
            symbol = row["symbol"]
            if not isinstance(symbol, str) or not SYMBOL_RE.fullmatch(symbol):
                continue
            base = symbol[:-5]
            if base in STABLE_BASES or re.search(r"(?:3L|3S|2L|2S|UP|DOWN)$", base):
                continue
            turnover = float(row["volValue"])
            bid, ask = float(row["buy"]), float(row["sell"])
            if not all(math.isfinite(v) for v in (turnover, bid, ask)):
                continue
            if turnover < 1_000_000 or bid <= 0 or ask <= bid:
                continue
            spread = 200 * (ask - bid) / (ask + bid)
            if spread > 0.8:
                continue
            selected.append((turnover, symbol, spread))
        except (ValueError, KeyError, TypeError, ZeroDivisionError):
            continue
    selected.sort(reverse=True)
    return [{"symbol": symbol, "turnover_24h_usdt": round(turnover, 2),
             "spread_pct": round(spread, 4)}
            for turnover, symbol, spread in selected[:max(1, min(int(top_n), 75))]]


async def scan_kucoin(session, asof, top_n=30):
    """Public, read-only bounded scan. No exchange account or broker orders."""
    report = {"status": "UNAVAILABLE", "venue": "KuCoin spot",
              "market": "CRYPTO", "requested": 0, "scanned": 0,
              "watch": [], "errors": [], "automated_orders": False,
              "coverage": "top liquid eligible USDT spot pairs, not entire market"}
    try:
        async with session.get(TICKERS_URL) as response:
            response.raise_for_status()
            tickers = await response.json()
        if tickers.get("code") != "200000":
            raise ValueError("ticker_provider_error")
        selected = select_liquid_usdt(tickers["data"]["ticker"], top_n)
        report["requested"] = len(selected)
        semaphore = asyncio.Semaphore(3)

        async def inspect(item):
            symbol = item["symbol"]
            try:
                async with semaphore:
                    async with session.get(KLINE_URL, params={
                        "symbol": symbol, "type": "1hour",
                        "startAt": int(asof) - 340 * HOUR,
                        "endAt": int(asof)
                    }) as response:
                        response.raise_for_status()
                        data = await response.json()
                if data.get("code") != "200000":
                    raise ValueError("candles_provider_error")
                return classify_hourly(symbol, data["data"], asof, item)
            except (Exception, asyncio.CancelledError) as exc:
                if isinstance(exc, asyncio.CancelledError):
                    raise
                return _unavailable(symbol, type(exc).__name__)
        results = await asyncio.gather(*(inspect(x) for x in selected))
        report["scanned"] = len(results)
        report["watch"] = sorted(
            [x for x in results if x["status"] in ("LONG_WATCH", "SHORT_WATCH", "EXTENDED_WAIT")],
            key=lambda x: (-x["score"], x["symbol"]))
        report["errors"] = [
            {"symbol": x["symbol"], "reason": x["reason"]}
            for x in results if x["status"] == "UNAVAILABLE"][:15]
        report["evaluated"] = sum(x["status"] != "UNAVAILABLE" for x in results)
        report["status"] = "COMPLETE" if not report["errors"] else "PARTIAL"
    except (Exception, asyncio.CancelledError) as exc:
        if isinstance(exc, asyncio.CancelledError):
            raise
        report["errors"] = [{"reason": type(exc).__name__}]
    return report
