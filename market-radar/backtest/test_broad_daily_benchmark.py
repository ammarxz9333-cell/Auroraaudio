import datetime as dt
import unittest
import tempfile
from unittest.mock import patch

from backtest import broad_daily_benchmark as b
from backtest import make_cohort_split
from backtest import pit_backtester
from backtest import historical_gate_replay_v2 as replay


def bars(n=90):
    start = dt.date(2025, 1, 1)
    return [{"date": (start + dt.timedelta(days=i)).isoformat(),
             "open": 100.0, "high": 101.0, "low": 99.0,
             "close": 100.0, "volume": 100000} for i in range(n)]


class StrictPointInTime(unittest.TestCase):
    def test_intraday_replay_uses_next_bar_open(self):
        base = dt.datetime(2025, 1, 2, 14, 30, tzinfo=dt.timezone.utc)
        bars5 = [
            {"time": base, "open": 100, "high": 130, "low": 70, "close": 120},
            {"time": base+dt.timedelta(minutes=5), "open": 102, "high": 104, "low": 99, "close": 103},
        ]
        result = replay.forward_outcome(bars5, 0, 0.5)
        self.assertEqual(result["raw_next_open"], 102)
        self.assertEqual(result["outcome"], "UNRESOLVED")

    def test_full_intraday_replay_never_counts_signal_bar_and_reuses_cache(self):
        def bar(day, hour, minute, opn, high, low, close, volume):
            t = dt.datetime.fromisoformat(f"{day}T{hour:02d}:{minute:02d}:00+00:00")
            return {"time": t, "ny": t.astimezone(replay.NY), "open": opn,
                    "high": high, "low": low, "close": close, "volume": volume}
        series = [bar("2025-01-01", 14, 30, 100, 101, 99, 100, 100),
                  bar("2025-01-02", 14, 25, 100, 100, 100, 100, 100),
                  bar("2025-01-02", 14, 30, 100, 130, 70, 110, 1000),
                  bar("2025-01-02", 14, 35, 102, 104, 99, 103, 100)]
        case = {"ticker": "ABC", "decision": "WATCH", "cutoff": "2025-01-02T14:30:00+00:00"}
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(replay, "yahoo_5m", return_value=series):
                first = replay.replay_case(case, tmp)
            with patch.object(replay, "yahoo_5m", side_effect=AssertionError("cache was not used")):
                second = replay.replay_case(case, tmp)
        signal = first["spread_scenarios"]["0.5"]
        self.assertIsNotNone(signal)
        self.assertEqual(signal["raw_next_open"], 102)
        self.assertEqual(signal["outcome"], "UNRESOLVED")
        self.assertEqual(second["bars_sha256"], first["bars_sha256"])

    def test_signal_bar_extreme_does_not_count_and_next_open_pays_cost(self):
        data = bars(35)
        data[25]["high"] = 130
        data[25]["low"] = 70
        data[26]["open"] = 101
        result = b.outcome(data, 25)
        self.assertEqual(result["entry_date"], data[26]["date"])
        self.assertGreater(result["entry_price"], 101)
        self.assertEqual(result["outcome"], "UNRESOLVED")

    def test_same_following_day_both_barriers_is_not_a_win(self):
        data = bars(35)
        data[26].update({"high": 110, "low": 90})
        self.assertEqual(b.outcome(data, 25)["outcome"], "ORDER_UNVERIFIED")

    def test_future_prices_cannot_change_past_signal_features(self):
        data = bars()
        data[30].update({"close": 103, "high": 104, "low": 99, "volume": 500000})
        spy = b.benchmark_index(bars())
        qqq = b.benchmark_index(bars())
        before = b.signal_features(data, 30, spy, qqq)
        data[31].update({"close": 999, "high": 1000, "low": 1, "volume": 99999999})
        self.assertEqual(before, b.signal_features(data, 30, spy, qqq))

    def test_chronological_cohorts_and_embargo(self):
        dates = [(dt.date(2025, 1, 1)+dt.timedelta(days=i)).isoformat() for i in range(100)]
        labels, bounds = b.chronological_labels(dates, embargo_days=7)
        self.assertEqual(labels[dates[59]], "development")
        self.assertEqual(labels[dates[60]], "embargo")
        self.assertEqual(labels[dates[80]], "embargo")
        self.assertEqual(labels[dates[90]], "final_test")
        rows = [{"ticker": "ABC", "cutoff": d+"T20:00:00+00:00"} for d in dates]
        parts, _ = make_cohort_split.chronological_split(rows)
        self.assertTrue(all(r["cutoff"][:10] <= bounds["development_end"] for r in parts["development"]))
        self.assertTrue(all(r["cutoff"][:10] > bounds["validation_end"] for r in parts["final_test"]))

    def test_weekly_limit_never_uses_later_day_rank(self):
        rows = [{"date": f"2025-01-{d:02d}", "ticker": str(d), "signal": True,
                 "rank_score": 1 if d < 8 else 100} for d in (6, 7, 8)]
        picked = b.select_weekly(rows)
        self.assertEqual([r["date"] for r in picked], ["2025-01-06", "2025-01-07"])

    def test_freeze_rejects_market_bar_after_cutoff(self):
        snap = {"ticker": "ABC", "cutoff": "2025-01-01T20:00:00+00:00",
                "evidence": [], "market": {"bar_time_utc": "2025-01-01T20:05:00+00:00"}, "features": {}}
        with self.assertRaisesRegex(ValueError, "market bar after cutoff"):
            pit_backtester.freeze(snap, "WATCH", "test")

    def test_full_report_includes_winners_and_nonwinners_universe(self):
        directory = ("Nasdaq Traded|Symbol|Security Name|Listing Exchange|ETF|Test Issue\n"
                     "Y|ABC|ABC Common Stock|Q|N|N\n"
                     "Y|ETF|Fund ETF|Q|Y|N\n"
                     "File Creation Time: 20250101\n").encode()
        data = bars(100)
        report, rows, selected = b.run(directory, {"ABC": data}, data, data, 1)
        self.assertEqual(selected, ["ABC"])
        self.assertGreater(len(rows), 0)
        self.assertEqual(report["partitions"]["final_test"]["all_eligible"]["n"] > 0, True)
        self.assertEqual(sum(r["signal"] for r in rows), 0)


if __name__ == "__main__":
    unittest.main()
