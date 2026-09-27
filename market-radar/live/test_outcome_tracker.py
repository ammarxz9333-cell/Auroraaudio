import unittest
from outcome_tracker import new_trade, update_trade
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
