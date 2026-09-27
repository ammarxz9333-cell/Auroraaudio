import unittest
from live.outcome_tracker import new_trade, update_trade
class T(unittest.TestCase):
 def test_plus5_first(self):
  x=new_trade("X","t",100,"r"); update_trade(x,{"high":106,"low":98,"time_utc":"b"})
  self.assertEqual(x["first_threshold"],"PLUS5_FIRST")
 def test_minus5_first(self):
  x=new_trade("X","t",100,"r"); update_trade(x,{"high":102,"low":94,"time_utc":"b"})
  self.assertEqual(x["first_threshold"],"MINUS5_FIRST")
 def test_ambiguous_same_bar(self):
  x=new_trade("X","t",100,"r"); update_trade(x,{"high":106,"low":94,"time_utc":"b"})
  self.assertEqual(x["first_threshold"],"ORDER_UNVERIFIED")
 def test_plus10(self):
  x=new_trade("X","t",100,"r"); update_trade(x,{"high":111,"low":99,"time_utc":"b"})
  self.assertTrue(x["plus10"])
if __name__=="__main__": unittest.main()


def test_minus5_first_then_later_plus10_preserves_order_and_capture():
    t=new_trade("XYZ","2026-01-01T14:30:00+00:00",100,"r")
    update_trade(t,{"time_utc":"2026-01-01T14:35:00+00:00","high":102,"low":94})
    assert t["first_threshold"]=="MINUS5_FIRST"
    update_trade(t,{"time_utc":"2026-01-01T15:00:00+00:00","high":112,"low":99})
    assert t["first_threshold"]=="MINUS5_FIRST"
    assert t["plus10"] is True
    assert t["mfe_pct"]>=12

def test_plus5_first_order_is_locked_if_later_minus5():
    t=new_trade("XYZ","2026-01-01T14:30:00+00:00",100,"r")
    update_trade(t,{"time_utc":"2026-01-01T14:35:00+00:00","high":106,"low":99})
    update_trade(t,{"time_utc":"2026-01-01T15:00:00+00:00","high":111,"low":94})
    assert t["first_threshold"]=="PLUS5_FIRST"
    assert t["plus10"] is True and t["minus5"] is True
