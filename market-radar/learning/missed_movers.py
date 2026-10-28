#!/usr/bin/env python3
"""Point-in-time learning ledger: only BEFORE-the-move detections count.

Legacy outcome_known cases without a timestamped first 5% crossing are
UNAUDITABLE, not proof of either predictive success or failure.
"""
from __future__ import annotations

import datetime as dt
import json
from pathlib import Path

SCHEMA_VERSION = 3
MOVE_THRESHOLD_PCT = 5.0
EARLY_WINDOW_MINUTES = 90


def _time(value):
    obj = dt.datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    if obj.tzinfo is None:
        raise ValueError("naive timestamp")
    return obj.astimezone(dt.timezone.utc)


def load(path: Path):
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(data, dict):
            return data
    except (OSError, ValueError):
        pass
    return {"schema_version": SCHEMA_VERSION, "updated_utc": None,
            "cases": [], "metrics": {}}


def save(path: Path, data: dict):
    path.parent.mkdir(parents=True, exist_ok=True)
    data["schema_version"] = SCHEMA_VERSION
    data["updated_utc"] = dt.datetime.now(dt.timezone.utc).isoformat()
    cases = data.get("cases", [])
    known = [c for c in cases if c.get("outcome_known")]
    eligible = [c for c in known if c.get("first_material_move_at")]
    caught = [c for c in eligible if c.get("caught_early") is True]
    missed = [c for c in eligible if c.get("missed") is True]
    data["metrics"] = {
        "outcome_known": len(known),
        "eligible_cases": len(eligible),
        "legacy_unauditable": len(known) - len(eligible),
        "caught_early": len(caught),
        "missed": len(missed),
        "recall_pct": round(100 * len(caught) / len(eligible), 2) if eligible else None,
        "warning": ("Only captured qualified signals strictly before the first "
                    "observed 5% move and within 90 minutes of first capture count. "
                    "Legacy cases without first_material_move_at are excluded.")
    }
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def update_case(data: dict, snapshot: dict, tape_qualified: bool, captured_utc: str):
    ticker = str(snapshot.get("ticker") or "").upper()
    bar_time = snapshot.get("bar_time_utc")
    price = snapshot.get("price")
    if not ticker or not bar_time or not isinstance(price, (int, float)) or price <= 0:
        return
    try:
        captured = _time(captured_utc)
        observed_bar = _time(bar_time)
    except (ValueError, TypeError):
        return
    if observed_bar > captured or (captured - observed_bar).total_seconds() > 120:
        return  # stale/future snapshots are not evidence of early opportunity
    day = observed_bar.astimezone(dt.timezone.utc).date().isoformat()
    key = f"{ticker}:{day}"
    cases = data.setdefault("cases", [])
    case = next((c for c in cases if c.get("key") == key), None)
    obs = {
        "captured_utc": captured.isoformat(), "bar_time_utc": observed_bar.isoformat(),
        "price": price,
        "change_pct": snapshot.get("change_pct"),
        "change_5m_pct": snapshot.get("change_5m_pct"),
        "change_15m_pct": snapshot.get("change_15m_pct"),
        "change_30m_pct": snapshot.get("change_30m_pct"),
        "same_time_volume_ratio": snapshot.get("same_time_volume_ratio"),
        "market_session": snapshot.get("market_session"),
        "holds_vwap": snapshot.get("holds_vwap"),
        "qualified": bool(tape_qualified),
    }
    if case is None:
        case = {
            "key": key, "ticker": ticker, "date": day,
            "first_observation": obs, "observations": [],
            "first_material_move_at": None, "first_qualified_at": None,
            "min_observed_price": float(price),
            "caught_early": False, "outcome_known": False, "missed": False,
        }
        cases.append(case)
    elif case.get("observations"):
        previous = case["observations"][-1]
        if (captured <= _time(previous["captured_utc"])
                or observed_bar <= _time(previous["bar_time_utc"])):
            # A delayed/replayed candle is NOT a second independent price
            # observation, even if fetched at a later wall-clock time.
            return

    first = float(case["first_observation"]["price"])
    minimum = min(float(case.get("min_observed_price", first)), float(price))
    case["min_observed_price"] = minimum
    move = (float(price) / first - 1) * 100
    rebound = (float(price) / minimum - 1) * 100
    case["max_move_from_first_pct"] = max(
        float(case.get("max_move_from_first_pct") or 0), move)
    case["max_rebound_from_low_pct"] = max(
        float(case.get("max_rebound_from_low_pct") or 0), rebound)

    if tape_qualified and not case.get("first_qualified_at"):
        case["first_qualified_at"] = captured.isoformat()
    if (move >= MOVE_THRESHOLD_PCT or rebound >= MOVE_THRESHOLD_PCT) and not case.get("first_material_move_at"):
        case["first_material_move_at"] = captured.isoformat()
    if case.get("first_material_move_at"):
        move_time = _time(case["first_material_move_at"])
        detected = _time(case["first_qualified_at"]) if case.get("first_qualified_at") else None
        first_seen = _time(case["first_observation"]["captured_utc"])
        early = bool(detected and first_seen <= detected < move_time
                     and (detected - first_seen).total_seconds()
                     <= EARLY_WINDOW_MINUTES * 60)
        case["outcome_known"] = True
        case["caught_early"] = early
        case["missed"] = not early
        if case["missed"]:
            case["miss_reason"] = {
                "first_same_time_rvol": case["first_observation"].get("same_time_volume_ratio"),
                "first_material_move_at": case["first_material_move_at"],
                "first_qualified_at": case.get("first_qualified_at"),
                "note": "No qualifying observed signal strictly before the first material move."
            }
        else:
            case.pop("miss_reason", None)
    case.setdefault("observations", []).append(obs)
    case["observations"] = case["observations"][-40:]
    data["cases"] = cases[-500:]
