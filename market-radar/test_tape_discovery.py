import unittest
from tape_discovery import tape_signal, discover_candidates

class TapeDiscoveryTests(unittest.TestCase):
    def test_premarket_accumulation_qualifies(self):
        s={"change_pct":6.0,"change_5m_pct":1.0,"change_15m_pct":2.4,
           "change_30m_pct":3.5,"same_time_volume_ratio":5.2,
           "market_session":"PRE","holds_vwap":None,"holds_open":None}
        out=tape_signal(s)
        self.assertTrue(out["qualifies"])
        self.assertIn("premarket-accumulation",out["hits"])

    def test_early_accumulation_is_not_missed(self):
        s={"change_pct":1.1,"change_5m_pct":0.6,"change_15m_pct":1.2,
           "change_30m_pct":1.6,"same_time_volume_ratio":3.4,
           "market_session":"REGULAR","holds_vwap":True,"holds_open":True}
        out=tape_signal(s)
        self.assertTrue(out["qualifies"])
        self.assertIn("early-accumulation",out["hits"])

    def test_down_day_reversal_before_previous_close(self):
        s={"price":55.8,"session_low":53.6,"change_pct":-3.2,
           "change_5m_pct":0.9,"change_15m_pct":1.4,
           "change_30m_pct":1.9,"same_time_volume_ratio":2.3,
           "market_session":"REGULAR","holds_vwap":True,"holds_open":False}
        out=tape_signal(s)
        self.assertTrue(out["qualifies"])
        self.assertIn("down-day-reversal-from-session-low",out["hits"])
        self.assertIn("cumulative-same-clock",out["summary"])

    def test_down_day_reversal_rejects_missing_low_or_weak_volume(self):
        s={"price":55.8,"change_pct":-3.2,"change_5m_pct":0.9,
           "change_15m_pct":1.4,"same_time_volume_ratio":2.3,
           "market_session":"REGULAR","holds_vwap":True}
        self.assertFalse(tape_signal(s)["qualifies"])
        s.update(session_low=53.6,same_time_volume_ratio=1.0)
        self.assertFalse(tape_signal(s)["qualifies"])
        s.update(same_time_volume_ratio=2.3,price=59)
        self.assertFalse(tape_signal(s)["qualifies"])  # already chased >6% from low

    def test_live_freshness_gate_rejects_closed_stale_or_missing_tape(self):
        from datetime import datetime, timezone, timedelta
        asof = datetime(2026,10,27,14,35,tzinfo=timezone.utc)
        snapshot = {"price":55.8,"session_low":53.6,"change_pct":-3.2,
                    "change_5m_pct":0.9,"change_15m_pct":1.4,
                    "same_time_volume_ratio":2.3,"market_session":"REGULAR",
                    "holds_vwap":True,"bar_time_utc":"2026-10-27T14:30:00Z"}
        self.assertTrue(tape_signal(snapshot,asof)["qualifies"])
        self.assertFalse(tape_signal(snapshot,asof+timedelta(minutes=20))["qualifies"])
        self.assertFalse(tape_signal(snapshot,datetime(2026,10,27,21,0,tzinfo=timezone.utc))["qualifies"])
        self.assertFalse(tape_signal({**snapshot,"bar_time_utc":"2026-10-27T14:36:00Z"},asof)["qualifies"])
        self.assertFalse(tape_signal({k:v for k,v in snapshot.items() if k!="bar_time_utc"},asof)["qualifies"])

    def test_no_rvol_no_alert(self):
        s={"change_pct":7,"change_5m_pct":2,"change_15m_pct":4,
           "change_30m_pct":6,"same_time_volume_ratio":1.1,
           "market_session":"REGULAR","holds_vwap":True,"holds_open":True}
        self.assertFalse(tape_signal(s)["qualifies"])

    def test_parabolic_chase_is_penalized(self):
        s={"change_pct":80,"change_5m_pct":0.2,"change_15m_pct":0.4,
           "change_30m_pct":1,"same_time_volume_ratio":8,
           "market_session":"REGULAR","holds_vwap":True,"holds_open":True}
        self.assertFalse(tape_signal(s)["qualifies"])

    def test_broad_screen_filters_junk_and_ranks(self):
        payload={"data":{"rows":[
          {"symbol":"AAA","lastsale":"$10","pctchange":"4.2%","volume":"2,500,000","marketCap":"500000000"},
          {"symbol":"PENNY","lastsale":"$0.20","pctchange":"40%","volume":"9000000","marketCap":"10000000"},
          {"symbol":"BBB","lastsale":"$20","pctchange":"1.0%","volume":"4,000","marketCap":"800000000"}
        ]}}
        import json
        def fake_fetch(url,headers=None): return json.dumps(payload).encode()
        rows=discover_candidates(fake_fetch,limit=10)
        self.assertEqual([x["ticker"] for x in rows],["AAA"])

    def test_broad_screen_reserves_places_for_reversals_and_small_movers(self):
        import json
        rows = [
            {"symbol":f"UP{i}","lastsale":"$10","pctchange":"18%",
             "volume":"1000000","marketCap":"900000000"}
            for i in range(25)
        ]
        rows += [
            {"symbol":"DOWN","lastsale":"$11","pctchange":"-2%",
             "volume":"250000","marketCap":"900000000"},
            {"symbol":"EARLY","lastsale":"$11","pctchange":"1%",
             "volume":"250000","marketCap":"900000000"}
        ]
        def fetch(url,headers=None):
            return json.dumps({"data":{"rows":rows}}).encode()
        names = [x["ticker"] for x in discover_candidates(fetch,limit=8)]
        self.assertEqual(len(names),8)
        self.assertIn("DOWN",names)
        self.assertIn("EARLY",names)

    def test_broad_screen_keeps_early_liquid_mover(self):
        payload={"data":{"rows":[
          {"symbol":"EARLY","lastsale":"$25","pctchange":"0.6%","volume":"50,000","marketCap":"900000000"},
          {"symbol":"JUNK","lastsale":"$2","pctchange":"0.2%","volume":"50,000","marketCap":"900000000"}
        ]}}
        import json
        def fake_fetch(url,headers=None): return json.dumps(payload).encode()
        rows=discover_candidates(fake_fetch,limit=10)
        self.assertIn("EARLY",[x["ticker"] for x in rows])

if __name__=="__main__": unittest.main()
