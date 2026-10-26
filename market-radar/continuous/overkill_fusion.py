"""Evidence-weighted ranking of the SAVED Overkill Independent v2 signals.

The indicator gates action; news/volume change its RANK, never fabricate a BUY.
This heuristic is not a calibrated success probability or trading advice.
"""
from __future__ import annotations

import json
import re
from datetime import datetime, timezone

# Positive evidence is deliberately narrower than broad keyword sentiment.
POSITIVE = re.compile(
    r"\b(?:"
    r"raises? (?:full.year |annual |revenue )?(?:guidance|outlook)|"
    r"upgrades? (?:to |from )?(?:buy|overweight|outperform)|"
    r"beats? (?:earnings|revenue|estimates|expectations)|"
    r"approval (?:granted|received)|receives? (?:fda )?approval|"
    r"awarded? (?:a |new |major )?(?:contract|grant)|"
    r"wins? (?:a |new |major )?contract|"
    r"announces? (?:a |new )?(?:major )?(?:customer|contract)|"
    r"signs? (?:a |new )?(?:multiyear|multi.year|major) contract"
    r")\b", re.I)
NEGATIVE = re.compile(
    r"\b(?:bankruptcy|default|fraud|investigation|"
    r"misses? (?:earnings|revenue|estimates)|"
    r"lowers? (?:guidance|outlook)|cuts? (?:guidance|outlook)|"
    r"contract cancellation|cancels? contract|"
    r"downgrades? to (?:sell|underperform))\b", re.I)
NEGATION = re.compile(
    r"\b(?:not|no|denies?|rumou?r|unconfirmed|possible|may|could|"
    r"seeks|plans to|reports of)\b", re.I)


def _events(rows, asof):
    """Only actually observed, date-valid public events enter the scoring."""
    out = []
    for row in rows:
        try:
            record = dict(row)
            payload = record.get('payload')
            event = json.loads(payload) if isinstance(payload, str) else payload
            published = float(record['published'])
            observed = float(record.get('observed', published))
            if not (0 <= asof - published <= 48 * 3600 and
                    observed <= asof and published <= asof):
                continue
            if not isinstance(event, dict) or not event.get('url'):
                continue
            if not str(event['url']).startswith(('http://', 'https://')):
                continue
            out.append((event, published))
        except (ValueError, TypeError, KeyError):
            continue
    return out


def evaluate(snapshot, event_rows, asof):
    """Score REAL next-open strategy signals only, never an intraday approximation.

    Technical <=60, verified news <=25, daily price/volume <=15.
    Risk always overrides the HIGH ranking, independently of the arithmetic.
    """
    if (snapshot.get('status') != 'READY' or
            snapshot.get('bars_confirmed') is not True or
            snapshot.get('signal_window_open') is not True):
        return {'status': 'NO_VALID_SIGNAL', 'score': None,
                'tier': 'NOT_ACTIONABLE', 'components': None}
    modes = {k: v.get('next_open_action', 'NONE')
             for k, v in snapshot.get('modes', {}).items()
             if isinstance(v, dict)}
    buys = [m for m, a in modes.items() if a == 'BUY_NEXT_OPEN']
    exits = [m for m, a in modes.items() if a == 'EXIT_NEXT_OPEN']
    if not (buys or exits):
        return {'status': 'NO_SIGNAL', 'score': None,
                'tier': 'NO_SIGNAL', 'components': None}
    if exits and not buys:
        return {'status': 'EXIT', 'score': None, 'tier': 'EXIT_SIGNAL',
                'components': None, 'buy_modes': [], 'exit_modes': exits}
    tech = 45 + 5 * bool(snapshot.get('weekly_green_regime')) + \
        5 * bool(snapshot.get('monthly_green_regime')) + \
        5 * bool(snapshot.get('gold_zone'))
    ctx = snapshot.get('price_context') or {}
    momentum = 0
    evidence = []
    missing = []
    if ctx.get('bullish_close') is True:
        momentum += 5
        evidence.append('completed_daily_bull_candle')
    if ctx.get('positive_daily_return') is True:
        momentum += 5
        evidence.append('completed_daily_positive_return')
    ratio = ctx.get('daily_volume_ratio')
    if isinstance(ratio, (int, float)) and ratio >= 1.5:
        momentum += 5
        evidence.append('relative_daily_volume_1.5_plus')
    elif ratio is None:
        missing.append('daily_volume_ratio_unavailable')

    events = _events(event_rows, float(asof))
    supported = []
    negative = []
    risk = []
    for event, stamp in events:
        title = str(event.get('title', ''))
        official = event.get('official') is True
        confidence = float(event.get('confidence') or 0)
        reliable = official and confidence >= 70 or confidence >= 80
        if not reliable:
            continue
        if event.get('dilution'):
            risk.append((event, stamp, 'dilution'))
            continue
        if event.get('negative') or NEGATIVE.search(title):
            negative.append((event, stamp))
            continue
        if POSITIVE.search(title) and not NEGATION.search(title):
            supported.append((event, stamp))
    supported.sort(key=lambda p: (p[0].get('official') is True,
                                  float(p[0].get('confidence') or 0),
                                  p[1]), reverse=True)
    news = 0
    source = None
    if supported:
        best = supported[0][0]
        independent = len({e.get('source_group', e.get('source_id'))
                           for e, _ in supported})
        news = 25 if best.get('official') is True else (
            18 if independent >= 2 else 10)
        source = {'title': str(best.get('title', ''))[:200],
                  'url': best['url'], 'official': bool(best.get('official')),
                  'independent_source_groups': independent}
        evidence.append('verified_positive_public_catalyst')
    else:
        missing.append('no_verified_positive_catalyst_last_48h')
    penalty = 40 if risk else 25 if negative else 0
    if risk:
        evidence.append('DILUTION_RISK')
    if negative:
        evidence.append('NEGATIVE_NEWS_RISK')

    score = max(0, min(100, tech + momentum + news - penalty))
    if risk or negative:
        tier = 'RISK_REVIEW'
    elif exits:
        tier = 'MIXED_SIGNALS'
    elif score >= 80:
        tier = 'HIGH_ALIGNMENT_BUY'
    elif score >= 65:
        tier = 'SUPPORTED_BUY'
    else:
        tier = 'TECHNICAL_BUY'
    return {
        'status': 'BUY', 'tier': tier, 'score': score,
        'components': {'technical': tech, 'positive_news': news,
                       'momentum': momentum, 'risk_penalty': penalty},
        'buy_modes': buys, 'exit_modes': exits,
        'positive_catalyst': source,
        'risk_headlines': [
            {'title': str(e.get('title', ''))[:200], 'url': e['url'],
             'kind': kind} for e, _, kind in risk
        ][:3] + [
            {'title': str(e.get('title', ''))[:200], 'url': e['url'],
             'kind': 'negative'} for e, _ in negative
        ][:3],
        'evidence': evidence, 'missing_evidence': missing,
        'is_calibrated_probability': False, 'automatic_trading': False,
        'take_profit_pct': None,
        'assumption': 'One next-open hypothetical signal after completed daily candle.'
    }
