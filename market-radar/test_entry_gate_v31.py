#!/usr/bin/env python3
import unittest
from entry_gate_v31 import GateV31Input, entry_gate_v31

def x(**kw):
    d=dict(decision="WATCH_LOW_CONFIDENCE",gap_pct=10,premarket_reprice_pct=10,rvol=4,holds_vwap=True,holds_open=True,minutes_since_open=20,spread_pct=.5,above_open_pct=12,above_vwap_pct=4,price=5,gap_pct_abs=10,catalyst_decision="WATCH_LOW_CONFIDENCE",bar_close_location=.9)
    d.update(kw); return GateV31Input(**d)

class TestGateV31(unittest.TestCase):
    def test_strong_extension_can_pass(self): self.assertEqual(entry_gate_v31(x())["state"],"BUYABLE_NOW")
    def test_weak_close_blocks_extension(self): self.assertEqual(entry_gate_v31(x(bar_close_location=.6))["state"],"WAIT")
    def test_low_rvol_blocks_extension(self): self.assertEqual(entry_gate_v31(x(rvol=2.5))["state"],"WAIT")
    def test_below_open_still_blocks(self): self.assertEqual(entry_gate_v31(x(holds_open=False))["state"],"WAIT")
    def test_below_vwap_still_blocks(self): self.assertEqual(entry_gate_v31(x(holds_vwap=False))["state"],"WAIT")
    def test_vwap_extension_still_blocks(self): self.assertEqual(entry_gate_v31(x(above_vwap_pct=7))["state"],"WAIT")
    def test_extreme_microcap_still_blocks(self): self.assertEqual(entry_gate_v31(x(price=1.5,gap_pct=30,gap_pct_abs=30))["state"],"WAIT")

if __name__=="__main__": unittest.main()
