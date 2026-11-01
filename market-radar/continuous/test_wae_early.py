"""Pine WAE v5 1h-zone/5m-entry deterministic no-order regression."""
import math
import unittest
from continuous.wae_early import evaluate, FIVE, HOUR


def history(direction=1, shock=0, forming=False):
    start=1700000000-1700000000%HOUR
    five=[]
    price=100.0
    for i in range(200*12):
        t=start+i*FIVE
        step=0.005*math.sin(i/17)+0.004*math.sin(i/31)
        if i>200*12-26:
            step+=direction*0.19
        if i==200*12-1:
            step+=shock
        close=price+step
        five.append(dict(time=t,open=price,high=max(price,close)+.02,
                         low=min(price,close)-.02,close=close,volume=1000.))
        price=close
    hours=[]
    for j in range(200):
        rows=five[j*12:(j+1)*12]
        hours.append(dict(time=rows[0]['time'],open=rows[0]['open'],
                          high=max(x['high'] for x in rows),
                          low=min(x['low'] for x in rows),
                          close=rows[-1]['close'],
                          volume=sum(x['volume'] for x in rows)))
    # Last hour constructed from completed 5m bars, never future 1h OHLC.
    return five[-200:],hours[:-1],five[-1]['time']+FIVE+15


class WAERegression(unittest.TestCase):
    def test_both_bearish_put_and_bullish_call_regimes(self):
        for direction in (-1,1):
            bars,hours,now=history(direction)
            out=evaluate('TESTUSDT',bars,hours,now)
            self.assertEqual(out['zone_1h'],direction)
            self.assertFalse(out['actionable'])
            self.assertFalse(out['automated_orders'])
            self.assertTrue(out['provisional_hour'])

    def test_confirmed_hour_mode_is_separate(self):
        bars,hours,now=history(1)
        out=evaluate('TESTUSDT',bars,hours,now,mode='confirmed')
        self.assertFalse(out['provisional_hour'])

    def test_forming_candle_ignored(self):
        bars,hours,now=history(-1)
        expected=evaluate('TESTUSDT',bars,hours,now)
        bars.append({**bars[-1],'time':bars[-1]['time']+FIVE,
                     'close':bars[-1]['close']*1.2,'high':bars[-1]['close']*1.2})
        self.assertEqual(evaluate('TESTUSDT',bars,hours,now),expected)

    def test_missing_5m_bar_rejected(self):
        bars,hours,now=history(-1)
        bars.pop(-30)
        self.assertEqual(evaluate('TESTUSDT',bars,hours,now)['status'],'UNAVAILABLE')

    def test_stale_rejected(self):
        bars,hours,now=history(1)
        self.assertEqual(evaluate('TESTUSDT',bars,hours,now+3600)['reason'],'stale_5m')

    def test_spread_risk_rejection(self):
        bars,hours,now=history(-1)
        self.assertEqual(evaluate('TESTUSDT',bars,hours,now,spread_pct=1)['status'],'RISK_FILTERED')

    def test_gap_no_repeat_and_immutability(self):
        bars,hours,now=history(1)
        out=evaluate('TESTUSDT',bars,hours,now)
        self.assertIn(out['status'],('NO_FRESH_SIGNAL','FRESH_CALL','LATE_CALL','EARLY_WATCH_CALL'))
        self.assertFalse(out['is_calibrated_probability'])


if __name__=='__main__':
    unittest.main()
