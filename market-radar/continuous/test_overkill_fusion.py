"""Regression tests for unified Overkill signal + trusted-news ranking."""
import unittest

from continuous.overkill_fusion import evaluate

ASOF = 1793052000.0


def snapshot(action='BUY_NEXT_OPEN', **kw):
    obj = {
        'ticker': 'CRWV',
        'status': 'READY',
        'bars_confirmed': True,
        'signal_window_open': True,
        'weekly_green_regime': False,
        'monthly_green_regime': False,
        'gold_zone': False,
        'modes': {'Wave dots only': {'next_open_action': action}},
        'price_context': {'bullish_close': False,
                          'positive_daily_return': False,
                          'daily_volume_ratio': None},
    }
    obj.update(kw)
    return obj


def event(title, official=True, confidence=95, age=3600, **kw):
    payload = {'title': title, 'url': 'https://example.org/release',
               'official': official, 'confidence': confidence,
               'source_group': 'issuer-ir', 'source_id': 'issuer-ir',
               'dilution': False, 'negative': False}
    payload.update(kw)
    return {'payload': payload, 'published': ASOF - age,
            'observed': ASOF - age + 1}


class FusionTest(unittest.TestCase):
    def test_technical_signal_without_news_still_notifies(self):
        out = evaluate(snapshot(), [], ASOF)
        self.assertEqual(out['tier'], 'TECHNICAL_BUY')
        self.assertEqual(out['score'], 45)
        self.assertEqual(out['components']['positive_news'], 0)
        self.assertFalse(out['is_calibrated_probability'])
        self.assertIsNone(out['take_profit_pct'])

    def test_verified_positive_news_upgrades_only_real_signal(self):
        clean = snapshot(weekly_green_regime=True, monthly_green_regime=True,
                         gold_zone=True, price_context={
                             'bullish_close': True,
                             'positive_daily_return': True,
                             'daily_volume_ratio': 2.5})
        positive = [event('CoreWeave raises revenue guidance')]
        rated = evaluate(clean, positive, ASOF)
        self.assertEqual(rated['score'], 100)
        self.assertEqual(rated['tier'], 'HIGH_ALIGNMENT_BUY')
        self.assertEqual(rated['components'],
                         {'technical': 60, 'positive_news': 25,
                          'momentum': 15, 'risk_penalty': 0})
        no_signal = snapshot(action='NONE')
        self.assertEqual(evaluate(no_signal, positive, ASOF)['tier'],
                         'NO_SIGNAL')

    def test_unverified_rumor_or_future_news_not_awarded(self):
        s = snapshot()
        rows = [event('CoreWeave may win contract'),
                event('CoreWeave awarded major contract',
                      official=False, confidence=55),
                event('CoreWeave raises guidance', age=-300)]
        out = evaluate(s, rows, ASOF)
        self.assertEqual(out['score'], 45)
        self.assertIsNone(out['positive_catalyst'])

    def test_negative_and_dilution_override_strong_score(self):
        s = snapshot(weekly_green_regime=True, monthly_green_regime=True,
                     gold_zone=True, price_context={
                         'bullish_close': True,
                         'positive_daily_return': True,
                         'daily_volume_ratio': 2.0})
        rows = [
            event('CoreWeave raises guidance'),
            event('CoreWeave files at-the-market offering',
                  dilution=True)
        ]
        rated = evaluate(s, rows, ASOF)
        self.assertEqual(rated['tier'], 'RISK_REVIEW')
        self.assertEqual(rated['score'], 60)
        self.assertEqual(rated['components']['risk_penalty'], 40)
        self.assertEqual(len(rated['risk_headlines']), 1)

    def test_exit_and_missing_confirmation_are_not_buy_scores(self):
        x = evaluate(snapshot(action='EXIT_NEXT_OPEN'),
                     [event('Company raises revenue guidance')], ASOF)
        self.assertEqual(x['status'], 'EXIT')
        self.assertIsNone(x['score'])
        self.assertEqual(x['tier'], 'EXIT_SIGNAL')
        bad = snapshot(bars_confirmed=False)
        self.assertEqual(evaluate(bad, [], ASOF)['tier'],
                         'NOT_ACTIONABLE')
        stale = snapshot(signal_window_open=False)
        self.assertEqual(evaluate(stale, [], ASOF)['tier'],
                         'NOT_ACTIONABLE')

    def test_conflicting_modes_do_not_get_high_grade(self):
        s = snapshot(modes={
            'Wave dots only': {'next_open_action': 'BUY_NEXT_OPEN'},
            'Weekly + Fibonacci': {'next_open_action': 'EXIT_NEXT_OPEN'},
        })
        out = evaluate(s, [event('Company raises revenue guidance')], ASOF)
        self.assertEqual(out['tier'], 'MIXED_SIGNALS')


if __name__ == '__main__':
    unittest.main()
