"""Read-only signals matching Overkill_Independent_Backtest_v2.pine.

Independent reconstruction, not the proprietary Overkill Trading indicator.
WaveTrend 9/12/3, strong +/-53 crosses, previous CLOSED weekly/monthly trend,
previous 100 daily candles for Fibonacci 0.50-0.78 / -0.05-0.35 zones.
No fixed take-profit, no optional stop by default, no broker orders.
Input MUST be completed, chronological, SPLIT-ADJUSTED daily OHLC.
"""
from __future__ import annotations

from datetime import datetime, timezone
from math import isfinite

MODES = ('Wave dots only', 'Weekly + Fibonacci', 'Monthly + Fibonacci')
MIN_BARS = 260


def _ema(values, length):
    k = 2 / (length + 1)
    out = []
    prev = values[0]
    for value in values:
        prev = k * value + (1 - k) * prev
        out.append(prev)
    return out


def _wave(bars):
    typical = [(b['h'] + b['l'] + b['c']) / 3 for b in bars]
    esa = _ema(typical, 9)
    dev = _ema([abs(p - a) for p, a in zip(typical, esa)], 9)
    ci = [(p - a) / (.015 * d) if d else 0.0
          for p, a, d in zip(typical, esa, dev)]
    fast = _ema(ci, 12)
    slow = [None if i < 2 else (fast[i] + fast[i - 1] + fast[i - 2]) / 3
            for i in range(len(fast))]
    buy = [i > 2 and fast[i] > slow[i] and fast[i - 1] <= slow[i - 1]
           and slow[i] <= -53 for i in range(len(fast))]
    sell = [i > 2 and fast[i] < slow[i] and fast[i - 1] >= slow[i - 1]
            and slow[i] >= 53 for i in range(len(fast))]
    return buy, sell


def _parse(raw):
    if not isinstance(raw, list) or len(raw) < MIN_BARS:
        raise ValueError('insufficient_history')
    bars = []
    for item in raw:
        val = item.get('t', item.get('time'))
        if isinstance(val, (int, float)):
            d = datetime.fromtimestamp(val / 1000 if val > 1e11 else val,
                                       timezone.utc)
        elif isinstance(val, str):
            d = datetime.fromisoformat(val.replace('Z', '+00:00'))
            if d.tzinfo is None:
                raise ValueError('timestamp_needs_timezone')
            d = d.astimezone(timezone.utc)
        else:
            raise ValueError('invalid_timestamp')
        o, h, l, c = (float(item.get(k, item.get({
            'o': 'open', 'h': 'high', 'l': 'low', 'c': 'close'}[k])))
                      for k in 'ohlc')
        if not all(isfinite(x) and x > 0 for x in (o, h, l, c)) or (
                l > min(o, c) or h < max(o, c)):
            raise ValueError('invalid_ohlc')
        if bars and d.date() <= bars[-1]['date']:
            raise ValueError('unordered_or_duplicate_daily_bars')
        if bars and (d.date() - bars[-1]['date']).days > 9:
            raise ValueError('unexpected_daily_gap')
        bars.append({'date': d.date(), 'o': o, 'h': h, 'l': l, 'c': c})
    return bars


def _periods(bars, monthly):
    aggregates, group_indices, last_key = [], [], None
    for b in bars:
        date = b['date']
        key = ((date.year, date.month) if monthly
               else date.toordinal() - date.weekday())
        if key != last_key:
            aggregates.append(dict(b))
            last_key = key
        else:
            agg = aggregates[-1]
            agg['h'] = max(agg['h'], b['h'])
            agg['l'] = min(agg['l'], b['l'])
            agg['c'] = b['c']
        group_indices.append(len(aggregates) - 1)
    return aggregates, group_indices


def compute(raw):
    """Evaluate the final COMPLETED daily bar, for NEXT-open (not intraday)."""
    bars = _parse(raw)
    day_buy, day_sell = _wave(bars)
    week, widx = _periods(bars, False)
    month, midx = _periods(bars, True)
    week_buy, week_sell = _wave(week)
    month_buy, month_sell = _wave(month)
    week_regime = month_regime = 0
    indicators = []
    for i, b in enumerate(bars):
        new_week = i == 0 or widx[i] != widx[i - 1]
        new_month = i == 0 or midx[i] != midx[i - 1]
        wb = new_week and widx[i] > 0 and week_buy[widx[i] - 1]
        ws = new_week and widx[i] > 0 and week_sell[widx[i] - 1]
        mb = new_month and midx[i] > 0 and month_buy[midx[i] - 1]
        ms = new_month and midx[i] > 0 and month_sell[midx[i] - 1]
        if wb:
            week_regime = 1
        elif ws:
            week_regime = -1
        if mb:
            month_regime = 1
        elif ms:
            month_regime = -1
        fib = None
        if i >= 100:
            top = max(x['h'] for x in bars[i-100:i])
            low = min(x['l'] for x in bars[i-100:i])
            if top > low:
                fib = (top - b['c']) / (top - low)
        gold = fib is not None and .50 <= fib <= .78
        red = fib is not None and -.05 <= fib <= .35
        indicators.append({
            'buy_dot': day_buy[i], 'sell_dot': day_sell[i],
            'week_green': week_regime == 1,
            'month_green': month_regime == 1,
            'week_closed_red': ws, 'month_closed_red': ms,
            'gold': gold, 'red': red, 'fib': fib})
    result = {}
    for mode, name in enumerate(MODES):
        def eligible(ind):
            entry = ind['buy_dot'] and (
                mode == 0 or (ind['gold'] and (
                    ind['week_green'] if mode == 1 else ind['month_green'])))
            exit_ = (ind['sell_dot'] if mode == 0 else
                     (ind['sell_dot'] and ind['red']) or (
                         ind['week_closed_red'] if mode == 1
                         else ind['month_closed_red']))
            return entry, exit_

        holding = False
        last_executed = None
        for j in range(1, len(bars)):
            buy, sell = eligible(indicators[j - 1])
            if holding and sell:
                holding = False
                last_executed = {
                    'action': 'EXIT',
                    'signal_date': bars[j-1]['date'].isoformat(),
                    'fill_date': bars[j]['date'].isoformat()}
            elif not holding and buy:
                holding = True
                last_executed = {
                    'action': 'BUY',
                    'signal_date': bars[j-1]['date'].isoformat(),
                    'fill_date': bars[j]['date'].isoformat()}
        buy, sell = eligible(indicators[-1])
        action = ('EXIT_NEXT_OPEN' if holding and sell else
                  'BUY_NEXT_OPEN' if not holding and buy else 'NONE')
        result[name] = {
            'next_open_action': action,
            'simulated_position': 'LONG' if holding else 'FLAT',
            'last_executed_signal': last_executed}
    last = indicators[-1]
    return {
        'strategy': 'Overkill Independent v2',
        'date': bars[-1]['date'].isoformat(), 'bar_count': len(bars),
        'bars_confirmed': 'caller_must_verify',
        'price_basis': 'split_adjusted_daily_required',
        'daily_green_dot': last['buy_dot'],
        'daily_red_dot': last['sell_dot'],
        'fib_retracement': last['fib'],
        'gold_zone': last['gold'], 'red_zone': last['red'],
        'weekly_green_regime': last['week_green'],
        'monthly_green_regime': last['month_green'],
        'weekly_confirmed_red': last['week_closed_red'],
        'monthly_confirmed_red': last['month_closed_red'],
        'modes': result, 'take_profit_pct': None,
        'automatic_trading': False,
        'warning': ('Read-only research signal for the NEXT session open; '
                    'hypothetical position, not brokerage holdings.')}
