import json
import tempfile
import unittest
from pathlib import Path

from learning.missed_movers import SCHEMA_VERSION, save, update_case

BASE = "2026-10-27T14:{:02d}:00Z"


def snap(price, minute):
    return {
        "ticker": "TEST", "bar_time_utc": BASE.format(minute),
        "price": price, "market_session": "REGULAR",
        "change_5m_pct": None, "same_time_volume_ratio": None
    }


class LedgerTest(unittest.TestCase):
    def test_success_requires_observation_before_material_move(self):
        data = {}
        update_case(data, snap(100, 0), False, BASE.format(0))
        update_case(data, snap(102, 2), True, BASE.format(2))
        update_case(data, snap(106, 5), False, BASE.format(5))
        self.assertTrue(data["cases"][0]["caught_early"])
        self.assertFalse(data["cases"][0]["missed"])

    def test_signal_after_the_move_is_not_a_catch(self):
        data = {}
        update_case(data, snap(100, 0), False, BASE.format(0))
        update_case(data, snap(106, 5), True, BASE.format(5))
        self.assertFalse(data["cases"][0]["caught_early"])
        self.assertTrue(data["cases"][0]["missed"])
        update_case(data, snap(107, 6), True, BASE.format(6))
        self.assertFalse(data["cases"][0]["caught_early"])

    def test_rebound_from_low_counts_without_new_high(self):
        data = {}
        update_case(data, snap(100, 0), False, BASE.format(0))
        update_case(data, snap(90, 1), False, BASE.format(1))
        update_case(data, snap(92, 2), True, BASE.format(2))
        update_case(data, snap(95, 3), False, BASE.format(3))
        case = data["cases"][0]
        self.assertTrue(case["outcome_known"])
        self.assertGreater(case["max_rebound_from_low_pct"], 5)
        self.assertTrue(case["caught_early"])

    def test_legacy_not_counted_as_verified_recall(self):
        data = {"cases": [{"outcome_known": True, "caught_early": True, "missed": False}]}
        with tempfile.TemporaryDirectory() as folder:
            file = Path(folder) / "learn.json"
            save(file, data)
            saved = json.loads(file.read_text())
        self.assertEqual(saved["schema_version"], SCHEMA_VERSION)
        self.assertEqual(saved["metrics"]["eligible_cases"], 0)
        self.assertEqual(saved["metrics"]["legacy_unauditable"], 1)
        self.assertIsNone(saved["metrics"]["recall_pct"])

    def test_stale_or_replayed_observation_excluded(self):
        data = {}
        update_case(data, snap(100, 0), False, BASE.format(4))
        self.assertEqual(data, {})
        update_case(data, snap(100, 0), False, BASE.format(0))
        update_case(data, snap(105, 0), True, BASE.format(0))
        self.assertEqual(len(data["cases"][0]["observations"]), 1)


if __name__ == "__main__":
    unittest.main()
