import json
import tempfile
import unittest
from pathlib import Path

from continuous.core import Engine, utc, timestamp, format_alert
from continuous.reversal import detect_reversal

START = timestamp("2026-10-27T13:31:00Z")


def sample(i, rvol=1.9):
    if i < 12:
        p = 100.1 - 0.15 * i
    elif i <= 20:
        p = 98.3 - 0.4 * (i - 12)
    else:
        p = 95.1 + 0.08 * (i - 20)
    volume = int(4000 * rvol) if i >= 65 else 4000
    return {
        "ticker": "XYZ", "time": utc(START + i * 60),
        "open": p - 0.06, "high": p + 0.05, "low": p - 0.15,
        "close": p, "volume": volume, "vwap": 98.4,
        "market_session": "REGULAR", "feed": "alpaca_sip",
        "baseline_volume": 4000, "baseline_days": 10,
        "baseline_asof": "2026-10-26T20:00:00Z",
        "spread_pct": 0.2,
    }


class ReversalTest(unittest.TestCase):
    def test_technical_watch_without_news_and_no_entry(self):
        bars = [sample(i) for i in range(70)]
        evidence = detect_reversal(bars)
        self.assertIsNotNone(evidence)
        self.assertEqual(evidence["classification"], "WATCH EARLY")
        self.assertFalse(evidence["buyable"])
        self.assertLess(evidence["bounce_from_low_pct"], 6.5)
        self.assertGreaterEqual(evidence["rvol_5m"], 1.5)

    def test_developing_requires_more_volume_and_30m_higher_low(self):
        bars = [sample(i, rvol=3.0) for i in range(70)]
        self.assertEqual(detect_reversal(bars)["classification"], "DEVELOPING")

    def test_bad_session_missing_baseline_and_discontinuous_bars_rejected(self):
        bars = [sample(i) for i in range(70)]
        self.assertIsNone(detect_reversal([{**b, "market_session": "EXTENDED"} for b in bars]))
        self.assertIsNone(detect_reversal(bars[:-1] + [{**bars[-1], "baseline_volume": None}]))
        self.assertIsNone(detect_reversal(bars[:-1] + [{**bars[-1], "time": utc(START + 10000)}]))
        self.assertIsNone(detect_reversal([{**b, "volume": 0} for b in bars]))

    def test_engine_persists_one_alert_and_blocks_replay(self):
        with tempfile.TemporaryDirectory() as tmp:
            db = str(Path(tmp) / "radar.sqlite")
            engine = Engine(db, {"XYZ": ["Example XYZ Inc"]})
            try:
                for i in range(70):
                    b = sample(i)
                    engine.bar(b, timestamp(b["time"]))
                alerts = [json.loads(r[0]) for r in engine.db.execute(
                    "SELECT payload FROM outbox WHERE json_extract(payload,'$.strategy')='EARLY_TECHNICAL_REVERSAL'")]
                self.assertEqual(len(alerts), 1)
                self.assertEqual(alerts[0]["classification"], "WATCH EARLY")
                self.assertFalse(alerts[0]["buyable"])
                self.assertIsNone(alerts[0]["first_public_at"])
                message = format_alert(alerts[0])
                self.assertIn('RVOL 5m', message)
                self.assertIn('BUYABLE_NOW: NO', message)
                self.assertIn('$XYZ', message)
                self.assertNotIn('\\$XYZ', message)
                b = sample(69)
                engine.bar(b, timestamp(b["time"]))
                self.assertEqual(engine.db.execute(
                    "SELECT COUNT(*) FROM outbox WHERE json_extract(payload,'$.strategy')='EARLY_TECHNICAL_REVERSAL'").fetchone()[0], 1)
                engine.db.close()
                engine = Engine(db, {"XYZ": ["Example XYZ Inc"]})
                engine.bar(b, timestamp(b["time"]))
                self.assertEqual(engine.db.execute(
                    "SELECT COUNT(*) FROM outbox WHERE json_extract(payload,'$.strategy')='EARLY_TECHNICAL_REVERSAL'").fetchone()[0], 1)
            finally:
                engine.db.close()


if __name__ == "__main__":
    unittest.main()
