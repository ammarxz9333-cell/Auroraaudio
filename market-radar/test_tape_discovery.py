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
          {"symbol":"BBB","lastsale":"$20","pctchange":"1.0%","volume":"100,000","marketCap":"800000000"}
        ]}}
        import json
        def fake_fetch(url,headers=None): return json.dumps(payload).encode()
        rows=discover_candidates(fake_fetch,limit=10)
        self.assertEqual([x["ticker"] for x in rows],["AAA"])

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
