import json
import tempfile
import unittest
from pathlib import Path

from continuous.core import Engine, timestamp, utc, format_alert

T = timestamp('2026-10-06T20:05:00Z')
SOURCE = dict(id='penguin-ir', public=True, official=True, confidence=95, origin_group='penguin')
EVENT = dict(title='Penguin Solutions beat + raise, accelerating AI infrastructure demand', content='Raised fiscal 2027 outlook', url='https://ir.penguinsolutions.com/news/', published_at=utc(T))


def bar(price=65, t=T+30, volume=6000, **extra):
    return dict(ticker='PENG', time=utc(t), open=price-.3, high=price+.1, low=price-.4, close=price, volume=volume,
                vwap=price-.2, spread_pct=.1, baseline_volume=1000, baseline_days=20,
                baseline_asof='2026-10-05T20:00:00Z', **extra)


class CoreTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = str(Path(self.tmp.name)/'radar.db')
        self.e = Engine(self.path, {'PENG':['Penguin Solutions'], 'AI':['C3.ai Inc']})

    def tearDown(self):
        self.e.db.close()
        self.tmp.cleanup()

    def alerts(self):
        return [json.loads(r[0]) for r in self.e.db.execute('SELECT payload FROM outbox ORDER BY rowid')]

    def early(self):
        self.e.bar(bar(64, T, 2000), T)
        self.e.event(EVENT, SOURCE, T+1)
        self.e.bar(bar(), T+30)
        return self.alerts()[-1]

    def test_peng_synthetic_contract_early_to_late(self):
        a = self.early()
        self.assertEqual(a['classification'], 'CONFIRMED')
        self.assertTrue(a['buyable'])
        self.assertEqual(a['price_at_source'],64)
        self.assertEqual(a['rvol'],6)
        self.assertLess(a['source_to_detection_seconds'],60)
        for price in (73,74):
            self.e.bar(bar(price,T+60),T+60)
            self.assertEqual(self.alerts()[-1]['classification'],'LATE')
            self.assertFalse(self.alerts()[-1]['buyable'])

    def test_no_baseline_or_quote_no_buy(self):
        self.early()
        b = bar(65.2,T+60)
        b['baseline_volume'] = None
        b['spread_pct'] = None
        self.e.bar(b,T+60)
        self.assertFalse(self.alerts()[-1]['buyable'])

    def test_stale_and_future_tape_rejected(self):
        for t in (T-121,T+10):
            with self.assertRaises(ValueError):
                self.e.bar(bar(t=t),T)

    def test_lookahead_baseline_rejected(self):
        b = bar()
        b['baseline_asof']=utc(T+60)
        with self.assertRaises(ValueError): self.e.bar(b,T+30)

    def test_ambiguous_ai_not_ticker(self):
        self.assertEqual(self.e.resolve('AI infrastructure accelerates'),[])
        self.assertEqual(self.e.resolve('$AI infrastructure'),['AI'])

    def test_private_and_undated_rejected(self):
        with self.assertRaises(ValueError): self.e.event(EVENT,dict(SOURCE,public=False),T+1)
        with self.assertRaises(ValueError): self.e.event(dict(EVENT,published_at='2026-10-06'),SOURCE,T+1)

    def test_restart_dedupe_and_pending_delivery(self):
        self.early()
        count=len(self.alerts())
        self.e.db.close()
        self.e=Engine(self.path,{'PENG':['Penguin Solutions']})
        self.e.event(EVENT,SOURCE,T+31)
        self.assertEqual(len(self.alerts()),count)
        self.assertEqual(self.e.db.execute('SELECT count(*) FROM outbox WHERE sent=0').fetchone()[0],count)

    def test_dilution_on_other_story_blocks_entry(self):
        self.early()
        self.e.event(dict(EVENT,title='$PENG registered direct offering'),SOURCE,T+31)
        self.assertEqual(self.alerts()[-1]['classification'],'AVOID')

    def test_syndication_not_independent(self):
        self.early()
        self.e.event(dict(EVENT,url='https://example.com/copy'),dict(SOURCE,id='copy'),T+31)
        self.assertEqual(self.alerts()[-1]['independent_sources'],1)

    def test_fresh_rumor_requires_tape_and_source_quality(self):
        self.e.bar(bar(64,T,2000),T)
        self.e.event(EVENT,dict(SOURCE,official=False,confidence=85),T+1)
        self.e.bar(bar(),T+30)
        self.assertEqual(self.alerts()[-1]['classification'],'HIGH-CONVICTION EARLY')
        self.assertIn('heuristic',format_alert(self.alerts()[-1]))


if __name__=='__main__': unittest.main()
