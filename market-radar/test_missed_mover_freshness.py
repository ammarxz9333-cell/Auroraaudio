import datetime as dt
import json
import tempfile
from pathlib import Path
from unittest.mock import patch

import missed_movers


def test_us_close_guard_handles_daylight_saving():
    utc = dt.timezone.utc
    assert not missed_movers.audit_ready(dt.datetime(2026, 9, 28, 19, 59, tzinfo=utc))
    assert missed_movers.audit_ready(dt.datetime(2026, 9, 28, 20, 15, tzinfo=utc))
    assert not missed_movers.audit_ready(dt.datetime(2026, 12, 28, 20, 59, tzinfo=utc))
    assert missed_movers.audit_ready(dt.datetime(2026, 12, 28, 21, 15, tzinfo=utc))


def test_old_session_quote_cannot_be_called_todays_mover():
    current = dt.datetime(2026, 9, 28, 19, tzinfo=dt.timezone.utc)
    old = current - dt.timedelta(days=3)
    quote = lambda stamp, symbol: {
        "regularMarketTime": int(stamp.timestamp()), "symbol": symbol,
        "regularMarketChangePercent": 11, "regularMarketPrice": 5,
        "regularMarketVolume": 1000000, "quoteType": "EQUITY", "market": "us_market",
    }
    payload = {"finance": {"result": [{"quotes": [quote(old, "OLD"), quote(current, "NEW")]}]}}
    with patch.object(missed_movers, "request_json", return_value=payload):
        rows = missed_movers.yahoo_day_gainers(current.astimezone(missed_movers.NY).date())
    assert [row["ticker"] for row in rows] == ["NEW"]
    assert rows[0]["quote_time_utc"] == current.isoformat()


def test_premature_run_removes_stale_same_day_audit():
    at = dt.datetime(2026, 9, 28, 5, 30, tzinfo=dt.timezone.utc)
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "missed.json"
        path.write_text(json.dumps({"days": {"2026-09-28": {"movers": [{"ticker": "OLD"}]}}}))
        with patch.object(missed_movers, "OUT_FILE", path), \
             patch.object(missed_movers, "now_utc", return_value=at), \
             patch.object(missed_movers, "persist_remote_json") as persist, \
             patch.object(missed_movers, "yahoo_day_gainers") as screen:
            missed_movers.main()
        assert "2026-09-28" not in json.loads(path.read_text())["days"]
        persist.assert_called_once()
        screen.assert_not_called()
