"""Point-in-time intraday reversal sensor; independent of Overkill daily signals.

It makes WATCH/DEVELOPING observations only, never buyable/order signals.
Only completed regular-session 1-minute bars with matching historical-minute
volume baselines may qualify. All thresholds are transparent heuristics, not
a calibrated backtest.
"""
from __future__ import annotations

from datetime import datetime
from math import isfinite
from zoneinfo import ZoneInfo

NY = ZoneInfo("America/New_York")


def detect_reversal(bars):
    """Return evidence from chronological, session-scoped bar dicts, or None.

    All evidence is derived from bars available at detection time. A daily
    rise, a news item, or a single abnormal minute alone never qualifies.
    """
    if len(bars) < 36:
        return None
    bars = bars[-390:]
    recent = bars[-36:]
    try:
        stamps = [datetime.fromisoformat(b["time"].replace("Z", "+00:00")).timestamp()
                  for b in recent]
        if any(b.get("market_session") != "REGULAR" for b in recent):
            return None
        if any(not (50 <= nxt - prv <= 90) for prv, nxt in zip(stamps, stamps[1:])):
            return None
        if len({datetime.fromtimestamp(t, NY).date() for t in stamps}) != 1:
            return None
        for b in recent:
            vals = (b["open"], b["high"], b["low"], b["close"],
                    b["volume"], b["vwap"])
            if not all(isinstance(v, (int, float)) and isfinite(v) for v in vals):
                return None
            if not (b["low"] <= b["close"] <= b["high"]) or b["volume"] < 0:
                return None
        for b in recent[-5:]:
            if (not isinstance(b.get("baseline_volume"), (int, float))
                    or b["baseline_volume"] <= 0
                    or b.get("baseline_days", 0) < 5):
                return None
    except (KeyError, ValueError, TypeError, OverflowError):
        return None

    b = recent[-1]
    price = b["close"]
    if price <= 0 or b["vwap"] <= 0:
        return None

    trough_index = min(range(len(bars)), key=lambda i: bars[i]["low"])
    trough = bars[trough_index]["low"]
    if trough_index < 8 or trough_index > len(bars) - 6 or trough <= 0:
        return None
    earlier_high = max(x["high"] for x in bars[:trough_index + 1])
    prior_drawdown = (earlier_high / trough - 1) * 100
    bounce = (price / trough - 1) * 100
    if prior_drawdown < 1.5 or not 1.25 <= bounce <= 6.5:
        return None

    five_move = (price / recent[-6]["close"] - 1) * 100
    fifteen_move = (price / recent[-16]["close"] - 1) * 100
    if five_move < 0.30 or fifteen_move < 0.60:
        return None

    low5 = min(x["low"] for x in recent[-5:])
    prev_low5 = min(x["low"] for x in recent[-10:-5])
    low15 = min(x["low"] for x in recent[-15:])
    prev_low15 = min(x["low"] for x in recent[-30:-15])
    if not (low5 > prev_low5 and low15 > prev_low15):
        return None

    volume5 = sum(x["volume"] for x in recent[-5:])
    baseline5 = sum(x["baseline_volume"] for x in recent[-5:])
    rvol5 = volume5 / baseline5
    if rvol5 < 1.5 or volume5 * price < 300_000:
        return None

    # A reclaim must have a prior below-VWAP observation; a breakout needs
    # a genuinely prior range (not a high from the current breakout bar).
    reclaimed = (any(x["close"] <= x["vwap"] for x in recent[-20:-5])
                 and all(x["close"] > x["vwap"] for x in recent[-3:]))
    prior_high = max(x["high"] for x in recent[-15:-5])
    breakout = price > prior_high
    if not (reclaimed or breakout) or not price > b["vwap"]:
        return None
    if (price / b["vwap"] - 1) * 100 > 2.5:
        return None
    last_range = b["high"] - b["low"]
    if last_range <= 0 or (price - b["low"]) / last_range < 0.65:
        return None

    strong = False
    if len(bars) >= 66:
        prev30 = bars[-60:-30]
        last30 = bars[-30:]
        strong = (min(x["low"] for x in last30) >
                  min(x["low"] for x in prev30)
                  and rvol5 >= 2.2 and bounce <= 5.0)

    return {
        "classification": "DEVELOPING" if strong else "WATCH EARLY",
        "price": price,
        "session_low": trough,
        "bounce_from_low_pct": round(bounce, 3),
        "prior_drawdown_pct": round(prior_drawdown, 3),
        "change_5m_pct": round(five_move, 3),
        "change_15m_pct": round(fifteen_move, 3),
        "rvol_5m": round(rvol5, 3),
        "volume_5m": volume5,
        "rvol_basis": "five completed regular-session 1m bars / same-NY-minute prior-session median baselines (>=5 days each)",
        "holds_vwap": True,
        "vwap": b["vwap"],
        "higher_lows_5m_15m": True,
        "higher_low_30m": strong,
        "vwap_reclaim": reclaimed,
        "compression_breakout": breakout,
        "trigger": prior_high if breakout else max(x["high"] for x in recent[-5:]),
        "invalidation": min(low15, b["vwap"]),
        "chase_distance_pct": round((price / b["vwap"] - 1) * 100, 3),
        "bar_time_utc": b["time"],
        "sample_bars": len(bars),
        "buyable": False,
        "limitations": [
            "Technical-only reversal, not an Overkill daily signal",
            "No confirmed catalyst/SEC clearance inferred",
            "30m confirmation and independent quote/spread checks required for any entry",
        ],
    }
