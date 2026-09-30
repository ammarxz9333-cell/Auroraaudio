import tempfile
import unittest
from pathlib import Path
from learning.missed_movers import load, save, update_case

class MissedMoverLearningTests(unittest.TestCase):
    def snap(self, price, qualified=False):
        return {"ticker":"TEST","bar_time_utc":"2026-09-30T14:00:00+00:00",
          "price":price,"change_pct":1.0,"change_5m_pct":0.5,"change_15m_pct":1.0,
          "change_30m_pct":1.5,"same_time_volume_ratio":3.0,
          "market_session":"REGULAR","holds_vwap":True}

    def test_records_true_live_miss_without_future_backfill(self):
        d={"cases":[]}
        update_case(d,self.snap(100),False,"2026-09-30T14:00:10+00:00")
        s=self.snap(106); s["bar_time_utc"]="2026-09-30T14:30:00+00:00"
        update_case(d,s,False,"2026-09-30T14:30:10+00:00")
        c=d["cases"][0]
        self.assertTrue(c["missed"])
        self.assertEqual(c["miss_reason"]["first_same_time_rvol"],3.0)

    def test_early_catch_is_not_a_miss(self):
        d={"cases":[]}
        update_case(d,self.snap(100),True,"2026-09-30T14:00:10+00:00")
        s=self.snap(106); s["bar_time_utc"]="2026-09-30T14:30:00+00:00"
        update_case(d,s,False,"2026-09-30T14:30:10+00:00")
        self.assertFalse(d["cases"][0]["missed"])

    def test_save_computes_recall(self):
        d={"cases":[{"outcome_known":True,"caught_early":True,"missed":False},
                    {"outcome_known":True,"caught_early":False,"missed":True}]}
        with tempfile.TemporaryDirectory() as td:
            p=Path(td)/"m.json"; save(p,d); x=load(p)
            self.assertEqual(x["metrics"]["recall_pct"],50.0)

if __name__=="__main__": unittest.main()
