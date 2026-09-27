import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from backtest import export_pit_candidates
from backtest import pit_backtester


class TapeExport(unittest.TestCase):
    def test_capture_time_is_cutoff_and_stale_bars_are_excluded(self):
        with tempfile.TemporaryDirectory() as directory:
            tape = Path(directory) / "tape"
            tape.mkdir()
            row = {"ticker": "ABC", "radar_id": "public-story",
                   "captured_utc": "2025-01-02T14:40:00+00:00",
                   "bar_time_utc": "2025-01-02T14:35:00+00:00",
                   "score_at_capture": 10, "price": 101,
                   "gate_state": "WAIT", "post30m_move_pct": 500}
            stale = {**row, "radar_id": "old", "bar_time_utc": "2025-01-01T14:35:00+00:00"}
            (tape / "ABC.jsonl").write_text("\n".join(map(json.dumps, [row, row, stale]))+"\n")
            output = Path(directory) / "cases.jsonl"
            with patch.object(sys, "argv", ["export", str(tape), "--out", str(output)]):
                export_pit_candidates.main()
            cases = [json.loads(line) for line in output.read_text().splitlines()]
            self.assertEqual(len(cases), 1)
            case = cases[0]
            self.assertEqual(case["cutoff"], row["captured_utc"])
            self.assertEqual(case["features"]["score"], 10)
            self.assertNotIn("post30m_move_pct", case["market"])
            self.assertTrue(pit_backtester.verify(pit_backtester.freeze(case, "WATCH", "frozen")))


if __name__ == "__main__":
    unittest.main()
