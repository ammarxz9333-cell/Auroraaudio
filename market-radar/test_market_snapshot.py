import datetime as dt
import json
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

import radar


class YahooMarketSnapshotTest(unittest.TestCase):
    def test_reads_ohlc_arrays_into_regular_session_snapshot(self):
        # One prior close and two current-session five-minute bars.
        timestamps = [
            1790279700,  # 2026-09-24 15:55 ET
            1790347800,  # 2026-09-25 10:50 ET
            1790348100,  # 2026-09-25 10:55 ET
        ]
        payload = {
            "chart": {
                "result": [{
                    "meta": {"regularMarketPreviousClose": 10.0},
                    "timestamp": timestamps,
                    "indicators": {"quote": [{
                        "open": [10.0, 20.0, 21.0],
                        "high": [10.5, 22.0, 21.5],
                        "low": [9.5, 19.0, 20.5],
                        "close": [10.2, 21.0, 21.2],
                        "volume": [100, 200, 300],
                    }]},
                }]
            }
        }
        with patch.object(radar, "fetch", return_value=json.dumps(payload).encode()):
            snapshot = radar.yahoo_market_snapshot("TEST")
        self.assertNotIn("error", snapshot)
        self.assertEqual(snapshot["regular_open"], 20.0)
        self.assertEqual(snapshot["session_high"], 22.0)
        self.assertEqual(snapshot["session_low"], 19.0)
        self.assertEqual(snapshot["cum_volume"], 500)

    def test_alert_freshness_rejects_old_and_undated_items(self):
        utc = dt.timezone.utc
        monday = dt.datetime(2026, 9, 28, 13, 0, tzinfo=utc)
        friday_after_close = dt.datetime(2026, 9, 25, 20, 30, tzinfo=utc)
        prior_monday = dt.datetime(2026, 9, 21, 13, 0, tzinfo=utc)
        self.assertTrue(radar.fresh_for_alert(friday_after_close, monday))
        self.assertFalse(radar.fresh_for_alert(prior_monday, monday))
        self.assertFalse(radar.fresh_for_alert(None, monday))

    def test_outcome_replay_preserves_threshold_order_between_scans(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(radar, "TRADES_FILE", Path(directory) / "trades.json"):
                entry = "2026-09-28T14:00:00+00:00"
                gate = {"state": "BUYABLE_NOW", "entry_time_utc": entry, "entry_price": 100}
                snapshot = {
                    "ticker": "TEST", "bar_time_utc": entry,
                    "bar_high": 100, "bar_low": 100,
                }
                radar.track_live_outcome(snapshot, gate, "rid")
                snapshot.update({
                    "bar_time_utc": "2026-09-28T14:15:00+00:00",
                    "bar_high": 101, "bar_low": 90,
                    "_outcome_bars": [
                        {"time_utc": "2026-09-28T14:05:00+00:00", "high": 104, "low": 98},
                        {"time_utc": "2026-09-28T14:10:00+00:00", "high": 106, "low": 97},
                        {"time_utc": "2026-09-28T14:15:00+00:00", "high": 101, "low": 90},
                    ],
                })
                radar.track_live_outcome(snapshot, {"state": "NO_ENTRY"}, "followup")
                trade = next(iter(radar.load_trades(radar.TRADES_FILE).values()))
                self.assertEqual(trade["first_threshold"], "PLUS5_FIRST")
                self.assertEqual(trade["resolved_time_utc"], "2026-09-28T14:10:00+00:00")

    def test_sec_issuer_cik_resolves_without_watchlist_guess(self):
        mapping_payload = {"0": {"cik_str": 1901279, "ticker": "NYAX", "title": "Nayax Ltd."}}
        with patch.object(radar, "fetch", return_value=json.dumps(mapping_payload).encode()):
            mapping = radar.sec_company_tickers()
        item = {"url": "https://www.sec.gov/Archives/edgar/data/1901279/000197640826000865/x.htm"}
        self.assertEqual(radar.sec_issuer_tickers({"name": "SEC Form 4 current filings"}, item, mapping), ["NYAX"])
        self.assertEqual(radar.sec_issuer_tickers({"name": "SEC Schedule 13D current filings"}, item, mapping), [])

    def test_entry_uses_observation_time_and_fresh_ask(self):
        observed = dt.datetime(2026, 9, 28, 14, 10, tzinfo=dt.timezone.utc)
        snapshot = {"ticker": "TEST", "price": 100, "previous_close": 99,
                    "regular_open": 99, "vwap": 99, "same_time_volume_ratio": 3,
                    "holds_vwap": True, "holds_open": True, "reaction": "reacting",
                    "pre30m_move_pct": 0, "market_session": "REGULAR",
                    "bar_time_utc": "2026-09-28T14:05:00+00:00"}
        quote = {"bid": 100.1, "ask": 100.2, "spread_pct": 0.1,
                 "quote_time": int(observed.timestamp()) - 30}
        with patch.object(radar, "now_utc", return_value=observed), \
             patch.object(radar, "yahoo_quote_bid_ask", return_value=quote):
            gate = radar.evaluate_entry_gate(snapshot, 9, 8)
            self.assertEqual(gate["state"], "BUYABLE_NOW")
            self.assertEqual(gate["entry_price"], 100.2)
            self.assertEqual(gate["entry_time_utc"], observed.isoformat())
            snapshot["market_session"] = "AFTER/CLOSED"
            self.assertEqual(radar.evaluate_entry_gate(snapshot, 9, 8)["state"], "NO_ENTRY")
            snapshot["market_session"] = "REGULAR"
            snapshot["bar_time_utc"] = "2026-09-25T20:00:00+00:00"
            self.assertEqual(radar.evaluate_entry_gate(snapshot, 9, 8)["state"], "INSUFFICIENT_DATA")


if __name__ == "__main__":
    unittest.main()
