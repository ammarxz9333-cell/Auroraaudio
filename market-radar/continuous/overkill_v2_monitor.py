"""Optional read-only daily scanner: same saved Overkill Independent v2.

Uses Alpaca 1Day bars with authoritative adjustment=split, never minute
bars as daily confirmation. Outputs no orders and cannot alter Radar news
BUYABLE_NOW decisions. Disabled until OVERKILL_V2_ENABLED=1.
"""
import json
import os
from datetime import date, datetime, time as dtime, timedelta, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

from continuous.overkill_v2_signals import compute

NY = ZoneInfo('America/New_York')
REPORT = Path('backtest/results/overkill-independent-v2-no-target-2026-10-26.json')


def cutoff_date(now):
    local = now.astimezone(NY)
    # Today's 1Day candle may evolve until extended trading ends at 20:00.
    return (local.date() if (local.hour, local.minute) >= (20, 30)
            else local.date() - timedelta(days=1))


def next_weekday(day):
    day += timedelta(days=1)
    while day.weekday() >= 5:
        day += timedelta(days=1)
    return day


def selection():
    override = os.getenv('OVERKILL_V2_SYMBOLS', '').strip()
    if override:
        names = [x.strip().upper() for x in override.split(',')]
    else:
        # Preserve the frozen 206-symbol validation cohort; add the user's
        # CRWV watch symbol without rewriting historical test membership.
        names = json.loads(REPORT.read_text(encoding='utf-8'))['stocks']['tickers'] + ['CRWV']
    valid = (x for x in names if x.isascii() and x.replace('.', '').isalpha()
             and len(x) <= 8)
    return list(dict.fromkeys(valid))[:210]


async def scan_daily(session, db, now, headers, feed='sip', symbols=None):
    if not all(headers.get(k) for k in ('APCA-API-KEY-ID', 'APCA-API-SECRET-KEY')):
        return {'status': 'disabled', 'reason': 'missing_alpaca_credentials'}
    if feed not in ('sip', 'iex'):
        raise ValueError('unsupported_feed')
    cutoff = cutoff_date(now)
    symbols = list(dict.fromkeys(symbols if symbols is not None else selection()))
    if not symbols:
        return {'status': 'disabled', 'reason': 'empty_symbols'}
    start = (cutoff - timedelta(days=1260)).isoformat()
    end = datetime.combine(cutoff + timedelta(days=1), dtime.min,
                           timezone.utc).isoformat()
    scanned = valid = buy = exit_ = 0
    errors = []
    for i in range(0, len(symbols), 6):
        chunk = symbols[i:i+6]
        params = dict(symbols=','.join(chunk), timeframe='1Day',
                      start=start, end=end, feed=feed,
                      adjustment='split', limit=10000, sort='asc')
        gathered = {name: [] for name in chunk}
        try:
            pages = 0
            while True:
                pages += 1
                if pages > 20:
                    raise ValueError('too_many_pages')
                async with session.get(
                    'https://data.alpaca.markets/v2/stocks/bars',
                    params=params, headers=headers
                ) as response:
                    response.raise_for_status()
                    payload = await response.json()
                for name, bars in payload.get('bars', {}).items():
                    if name in gathered:
                        gathered[name].extend(bars)
                token = payload.get('next_page_token')
                if not token:
                    break
                params['page_token'] = token
        except Exception as exc:
            errors.append({'symbols':chunk, 'reason':type(exc).__name__})
            continue
        for name in chunk:
            scanned += 1
            try:
                complete = [b for b in gathered[name]
                            if date.fromisoformat(b['t'][:10]) <= cutoff]
                result = compute(complete)
                # Measured only from CONFIRMED daily bars. Never infer RVOL
                # from the same-day incomplete candle or invent missing volume.
                latest = complete[-1]
                prior = complete[-2]
                volumes = [float(b.get('v', 0)) for b in complete[-21:-1]]
                current_volume = float(latest.get('v', 0))
                daily_ratio = (current_volume / (sum(volumes) / 20)
                               if len(volumes) == 20 and current_volume > 0
                               and all(v > 0 for v in volumes) else None)
                result['price_context'] = {
                    'bullish_close': float(latest['c']) > float(latest['o']),
                    'positive_daily_return': float(latest['c']) > float(prior['c']),
                    'daily_volume_ratio': daily_ratio,
                    'volume_basis': ('confirmed daily volume / prior 20-day mean'
                                     if daily_ratio is not None else 'unavailable'),
                }
                last = date.fromisoformat(result['date'])
                local = now.astimezone(NY)
                next_session = next_weekday(last)
                # Fail closed when a completed candle is stale OR expected next
                # open passed. Holiday calendars need separate verification.
                current = (cutoff - last).days <= 6
                pending_open = (local.date() < next_session or (
                    local.date() == next_session and
                    (local.hour, local.minute) < (9, 30)))
                result.update(
                    ticker=name, status='READY' if current else 'STALE',
                    bars_confirmed=True,
                    source='alpaca_1Day_split_adjusted',
                    actionable=False,
                    asof_utc=now.astimezone(timezone.utc).isoformat(),
                    next_expected_open_date=next_session.isoformat(),
                    signal_window_open=bool(current and pending_open))
                if not current or not pending_open:
                    for mode in result['modes'].values():
                        mode['next_open_action'] = 'NONE'
                if current:
                    valid += 1
                    buy += sum(v['next_open_action'] == 'BUY_NEXT_OPEN'
                               for v in result['modes'].values())
                    exit_ += sum(v['next_open_action'] == 'EXIT_NEXT_OPEN'
                                 for v in result['modes'].values())
                with db:
                    db.execute(
                        'INSERT OR REPLACE INTO overkill_v2_snapshots'
                        '(ticker,day,observed,payload) VALUES(?,?,?,?)',
                        (name, result['date'], now.timestamp(),
                         json.dumps(result)))
            except (ValueError, TypeError, KeyError, OverflowError) as exc:
                errors.append({'symbol':name, 'reason':str(exc)[:90]})
                with db:
                    db.execute(
                        'INSERT OR REPLACE INTO overkill_v2_snapshots'
                        '(ticker,day,observed,payload) VALUES(?,?,?,?)',
                        (name, '', now.timestamp(), json.dumps({
                            'ticker':name, 'status':'UNAVAILABLE',
                            'reason':str(exc)[:90], 'modes':{},
                            'actionable':False})))
    return {'status':'complete' if not errors else 'partial',
            'cutoff':cutoff.isoformat(), 'requested':len(symbols),
            'scanned':scanned, 'ready':valid,
            'mode_buy_signals':buy, 'mode_exit_signals':exit_,
            'errors':errors[:20],
            'data_basis':f'Alpaca 1Day split-adjusted {feed}; no live orders'}
