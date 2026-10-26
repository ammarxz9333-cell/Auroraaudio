"""Regression tests for SAME saved Overkill Independent v2 live signal path."""
import asyncio
import json
import math
import sqlite3
import unittest
from datetime import datetime, timedelta, timezone
from zoneinfo import ZoneInfo

from continuous.overkill_v2_signals import MODES, compute
from continuous.overkill_v2_monitor import cutoff_date, scan_daily, selection


def synthetic(count=820, finish=datetime(2026, 10, 23, tzinfo=timezone.utc)):
    days = []
    day = finish.date()
    while len(days) < count:
        if day.weekday() < 5:
            days.append(day)
        day -= timedelta(days=1)
    days.reverse()
    bars = []
    for i, d in enumerate(days):
        px = 100 + 6 * math.sin(i / 13) + 3 * math.sin(i / 29) + .02 * i
        bars.append({'t':d.isoformat() + 'T05:00:00Z',
                     'o':px, 'h':px + 2, 'l':px - 2, 'c':px})
    return bars


class Response:
    def __init__(self, payload):
        self.payload = payload
    async def __aenter__(self):
        return self
    async def __aexit__(self, *args):
        pass
    def raise_for_status(self):
        pass
    async def json(self):
        return self.payload


class FakeSession:
    def __init__(self, bars):
        self.bars = bars
        self.requests = []
    def get(self, url, params, headers):
        self.requests.append((url, dict(params), dict(headers)))
        names = params['symbols'].split(',')
        return Response({'bars':{name:self.bars for name in names},
                         'next_page_token':None})


class TestSignals(unittest.TestCase):
    def test_rules_preserved(self):
        out = compute(synthetic())
        self.assertEqual(set(out['modes']), set(MODES))
        self.assertIsNone(out['take_profit_pct'])
        self.assertFalse(out['automatic_trading'])
        self.assertEqual(out['price_basis'], 'split_adjusted_daily_required')
        self.assertEqual(out['date'], '2026-10-23')
        self.assertEqual(out['modes']['Wave dots only']['simulated_position'] in ('LONG','FLAT'), True)
        for name in MODES:
            self.assertIn(out['modes'][name]['next_open_action'],
                          ('NONE', 'BUY_NEXT_OPEN', 'EXIT_NEXT_OPEN'))

    def test_invalid_series_fail_closed(self):
        bars = synthetic()
        with self.assertRaisesRegex(ValueError, 'insufficient_history'):
            compute(bars[:100])
        with self.assertRaisesRegex(ValueError, 'unordered_or_duplicate'):
            compute(bars[:-1] + [bars[-2]])
        with self.assertRaisesRegex(ValueError, 'invalid_ohlc'):
            compute(bars[:-1] + [{**bars[-1], 'l':10000}])
        with self.assertRaisesRegex(ValueError, 'unexpected_daily_gap'):
            compute(bars[:-1] + [{**bars[-1], 't':'2030-01-01T05:00:00Z'}])

    def test_split_normalized_scale_wave_signal(self):
        bars = synthetic()
        base = compute(bars)
        norm = compute([{**b, **{k:b[k]/5 for k in ('o','h','l','c')}} for b in bars])
        self.assertEqual(base['daily_green_dot'], norm['daily_green_dot'])
        self.assertEqual(base['daily_red_dot'], norm['daily_red_dot'])
        self.assertEqual(base['modes']['Wave dots only'], norm['modes']['Wave dots only'])
        # Monthly thresholds can be exceptionally sensitive to floating-point
        # ties. This is why unproven exact Pine numerical parity is NOT claimed.

    def test_cutoff_time_and_206_symbol_contract(self):
        ny = ZoneInfo('America/New_York')
        self.assertEqual(cutoff_date(datetime(2026,10,26,19,50,tzinfo=ny)).isoformat(), '2026-10-25')
        self.assertEqual(cutoff_date(datetime(2026,10,26,20,31,tzinfo=ny)).isoformat(), '2026-10-26')
        self.assertEqual(len(selection()),207)
        self.assertEqual(len(set(selection())),207)
        self.assertIn('CRWV', selection())

    def test_alpaca_scanner_read_only_and_no_incomplete_day(self):
        ny = ZoneInfo('America/New_York')
        db = sqlite3.connect(':memory:')
        db.execute('CREATE TABLE overkill_v2_snapshots(ticker TEXT PRIMARY KEY,day TEXT,observed REAL,payload TEXT)')
        bars = synthetic()
        bars.append({**bars[-1], 't':'2026-10-26T05:00:00Z', 'c':200, 'h':201, 'l':100, 'o':150})
        fake = FakeSession(bars)
        report = asyncio.run(scan_daily(fake,db,
            datetime(2026,10,26,8,0,tzinfo=ny),
            {'APCA-API-KEY-ID':'synthetic', 'APCA-API-SECRET-KEY':'synthetic'},
            feed='iex',symbols=['TEST']))
        self.assertEqual(report['status'],'complete')
        self.assertEqual(report['ready'],1)
        params = fake.requests[0][1]
        self.assertEqual(params['timeframe'],'1Day')
        self.assertEqual(params['adjustment'],'split')
        self.assertEqual(params['feed'],'iex')
        snap = json.loads(db.execute('SELECT payload FROM overkill_v2_snapshots').fetchone()[0])
        self.assertEqual(snap['date'],'2026-10-23')
        self.assertEqual(snap['next_expected_open_date'],'2026-10-26')
        self.assertTrue(snap['signal_window_open'])
        self.assertFalse(snap['actionable'])
        db.close()


if __name__ == '__main__':
    unittest.main()
