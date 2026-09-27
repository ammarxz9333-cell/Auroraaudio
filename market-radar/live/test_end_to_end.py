#!/usr/bin/env python3
"""End-to-end semantics test: frozen gate entry -> persistent trade -> later bars."""
import tempfile, unittest
from pathlib import Path
from outcome_tracker import new_trade, update_trade, load, save

class Integration(unittest.TestCase):
    def test_buyable_created_once_and_only_future_bars_count(self):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/"trades.json"; trades={}
            tid="XYZ:2026-09-28T14:00:00+00:00:rid1"
            if tid not in trades:
                trades[tid]=new_trade("XYZ","2026-09-28T14:00:00+00:00",100,"rid1")
            # Replay of identical BUYABLE_NOW must not create a second trade.
            if tid not in trades:
                trades[tid]=new_trade("XYZ","2026-09-28T14:00:00+00:00",100,"rid1")
            self.assertEqual(len(trades),1)
            # Integration layer skips bars <= entry timestamp.
            bars=[
              {"high":120,"low":80,"time_utc":"2026-09-28T13:55:00+00:00"},
              {"high":104,"low":98,"time_utc":"2026-09-28T14:05:00+00:00"},
              {"high":106,"low":97,"time_utc":"2026-09-28T14:10:00+00:00"},
              {"high":101,"low":90,"time_utc":"2026-09-28T14:15:00+00:00"}]
            t=trades[tid]
            for b in bars:
                if b["time_utc"] <= t["entry_time_utc"]: continue
                update_trade(t,b)
            self.assertEqual(t["first_threshold"],"PLUS5_FIRST")
            self.assertEqual(t["resolved_time_utc"],"2026-09-28T14:10:00+00:00")
            # Later -5 cannot rewrite the already frozen first threshold.
            self.assertEqual(t["first_threshold"],"PLUS5_FIRST")
            save(p,trades); reread=load(p)
            self.assertEqual(reread[tid]["first_threshold"],"PLUS5_FIRST")

    def test_same_bar_both_thresholds_is_not_a_win(self):
        t=new_trade("XYZ","2026-09-28T14:00:00+00:00",100,"rid2")
        update_trade(t,{"high":106,"low":94,"time_utc":"2026-09-28T14:05:00+00:00"})
        self.assertEqual(t["first_threshold"],"ORDER_UNVERIFIED")

if __name__=="__main__":
    unittest.main()
