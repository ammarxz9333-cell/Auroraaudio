"""Integration regression: never mark a signal caught if its issue failed."""
import datetime as dt
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import radar


class PublishedAlertsLearningTest(unittest.TestCase):
    def exercise_scan(self, issue_url=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sources = root / "sources.json"
            state = root / "state.json"
            ledger = root / "missed.json"
            sources.write_text(json.dumps({"sources": [], "watchlist": {}}))
            observed = dt.datetime(2026, 10, 27, 14, 35, tzinfo=dt.timezone.utc)
            snapshot = {
                "ticker": "TEST", "bar_time_utc": observed.isoformat(),
                "price": 100.0, "change_pct": 1.0,
                "market_session": "REGULAR",
            }
            signal = {"qualifies": True, "score": 10,
                      "hits": ["synthetic-test-only"], "summary": "test"}
            if issue_url is None:
                publish = patch.object(radar, "create_issue",
                    side_effect=RuntimeError("GitHub unavailable"))
            else:
                publish = patch.object(radar, "create_issue",
                    return_value=issue_url)
            with (patch.object(radar, "SOURCES_FILE", sources),
                  patch.object(radar, "STATE_FILE", state),
                  patch.object(radar, "TRADES_FILE", root / "trades.json"),
                  patch.object(radar, "MISSED_MOVERS_FILE", ledger),
                  patch.object(radar, "TAPE_DIR", root / "tape"),
                  patch.dict(os.environ, {"RADAR_TAPE_PRIORITY_SYMBOLS": "TEST",
                                          "RADAR_TAPE_CANDIDATES": "1"}),
                  patch.object(radar, "discover_candidates", return_value=[]),
                  patch.object(radar, "yahoo_market_snapshot", return_value=snapshot),
                  patch.object(radar, "now_utc", return_value=observed),
                  patch.object(radar, "tape_signal", return_value=signal),
                  patch.object(radar, "sec_company_tickers", return_value={}),
                  patch.object(radar, "persist_market_snapshot"),
                  patch.object(radar, "telegram_alert") as phone,
                  publish):
                radar.main()
                cases = json.loads(ledger.read_text())["cases"]
                record = json.loads(state.read_text())
            self.assertEqual(len(cases), 1)
            self.assertEqual(len(cases[0]["observations"]), 1)
            self.assertEqual(cases[0]["ticker"], "TEST")
            self.assertFalse(cases[0]["outcome_known"])
            return cases[0], record, phone.call_count

    def test_rule_match_without_actual_issue_cannot_credit_detection(self):
        case, state, phones = self.exercise_scan(None)
        self.assertIsNone(case["first_qualified_at"])
        self.assertEqual(state["last_alert_count"], 0)
        self.assertNotIn("tape:TEST", state["tape_alerts"])
        self.assertEqual(phones, 0)
        self.assertTrue(any("GitHub unavailable" in e
                            for e in state["last_errors"]))

    def test_published_issue_credits_detection(self):
        case, state, phones = self.exercise_scan(
            "https://github.com/example/test/issues/123")
        self.assertIsNotNone(case["first_qualified_at"])
        self.assertEqual(state["last_alert_count"], 1)
        self.assertIn("tape:TEST", state["tape_alerts"])
        self.assertEqual(phones, 1)


if __name__ == "__main__":
    unittest.main()
