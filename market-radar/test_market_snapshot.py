import datetime as dt
import json
import unittest
from unittest.mock import patch

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


if __name__ == "__main__":
    unittest.main()
