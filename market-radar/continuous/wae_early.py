"""WAE 1H Zones + WAE 5m Signals (user Pine v5 defaults), research only.
Completed 5m bars; optional provisional 1h zone built from available 5m bars.
The early/chase filter is a separate ranking, never alters Pine CALL/PUT.
"""
from __future__ import annotations
import math
from datetime import datetime, timezone

FIVE, HOUR = 300, 3600


def _ema(values, n):
    out, prev = [], None
    alpha = 2 / (n + 1)
    for x in values:
        prev = x if prev is None else alpha*x + (1-alpha)*prev
        out.append(prev)
    return out


def _parse(raw, span, asof):
    rows = []
    for b in raw:
        if isinstance(b, dict):
            t = b.get('time', b.get('t'))
            if isinstance(t, str) and not t.isdigit():
                t = datetime.fromisoformat(t.replace('Z','+00:00')).timestamp()
            t = int(t)
            o, h, l, c, v = (float(b[k]) for k in
                             ('open','high','low','close','volume'))
        else:
            # Bybit v5: [start ms, open, high, low, close, volume, turnover].
            t = int(b[0])
            o, h, l, c, v = (float(b[i]) for i in (1,2,3,4,5))
        if t > 1e11:
            t //= 1000
        if (t % span or not all(map(math.isfinite,(o,h,l,c,v))) or
                min(o,h,l,c)<=0 or v<0 or l>min(o,c) or h<max(o,c)):
            raise ValueError('bad_ohlcv')
        rows.append(dict(t=t,open=o,high=h,low=l,close=c,volume=v))
    rows.sort(key=lambda b:b['t'])
    if any(b['t']-a['t'] != span for a,b in zip(rows,rows[1:])):
        raise ValueError('gapped_or_duplicate_bars')
    return [b for b in rows if b['t']+span <= asof]


def _wae(bars):
    prices = [b['close'] for b in bars]
    if not prices:
        return []
    e20, e40 = _ema(prices,20),_ema(prices,40)
    tr = []
    for i,b in enumerate(bars):
        prior = prices[i-1] if i else prices[i]
        tr.append(max(b['high']-b['low'],abs(b['high']-prior),
                      abs(b['low']-prior)))
    states, rma = [], None
    for i,b in enumerate(bars):
        if i == 99:
            rma = sum(tr[:100])/100
        elif i>99:
            rma = (99*rma+tr[i])/100
        if i<99:
            states.append(None)
            continue
        t1 = 150*((e20[i]-e40[i])-(e20[i-1]-e40[i-1]))
        window = prices[i-19:i+1]
        avg = sum(window)/20
        sd = math.sqrt(sum((x-avg)**2 for x in window)/20)
        threshold = max(4*sd,3.7*rma)
        states.append(1 if t1>threshold else
                      -1 if -t1>threshold else 0)
    return states


def _partial(fives, hour, through):
    a = [b for b in fives if hour <= b['t'] <= through]
    if not a or a[0]['t'] != hour or any(
            y['t']-x['t']!=FIVE for x,y in zip(a,a[1:])):
        return None
    return {'t':hour,'open':a[0]['open'],
            'high':max(b['high'] for b in a),
            'low':min(b['low'] for b in a),
            'close':a[-1]['close'],
            'volume':sum(b['volume'] for b in a)}


def evaluate(symbol, raw_five, raw_hour, asof, mode='live',
             spread_pct=None, max_zone_age_minutes=15, max_move_pct=1.5):
    """Replays 40 completed 5m bars with original start and gapBars=3 logic.
    NO_SIGNAL if WAE already triggered earlier; never issue a stale entry.
    """
    result = dict(strategy='WAE_1H_5M',symbol=symbol,mode=mode,
                  status='UNAVAILABLE',actionable=False,
                  automated_orders=False,is_calibrated_probability=False)
    if mode not in ('live','confirmed'):
        return {**result,'reason':'unknown_zone_mode'}
    try:
        asof = int(asof)
        five = _parse(raw_five,FIVE,asof)
        hour = _parse(raw_hour,HOUR,asof)
        if len(five)<140 or len(hour)<130:
            raise ValueError('insufficient_history')
        if asof-five[-1]['t']-FIVE>600:
            raise ValueError('stale_5m')
        if hour[-1]['t']+HOUR < (five[-1]['t']//HOUR)*HOUR:
            raise ValueError('stale_1h')
        five_states = _wae(five)
        replay = []
        for i in range(len(five)-40,len(five)):
            b=five[i]
            hour_start=b['t']//HOUR*HOUR
            prior=[x for x in hour if x['t']<hour_start]
            if len(prior)<125 or prior[-1]['t'] != hour_start-HOUR:
                raise ValueError('misaligned_hourly_data')
            if mode=='live':
                partial=_partial(five,hour_start,b['t'])
                if partial is None:
                    raise ValueError('missing_5m_hour_open')
                hstate=_wae(prior+[partial])[-1]
            else:
                hstate=_wae(prior)[-1]
            fstate=five_states[i]
            if hstate is None or fstate is None:
                raise ValueError('missing_warmup')
            replay.append(dict(t=b['t'],price=b['close'],hour=hstate,
                               five=fstate,call=hstate==1 and fstate==1,
                               put=hstate==-1 and fstate==-1))
        prev_c=prev_p=False
        lc=lp=-10000
        pend_c=pend_p=False
        for idx,x in enumerate(replay):
            cs=x['call'] and not prev_c
            ps=x['put'] and not prev_p
            call=x['call'] and (cs or pend_c) and idx-lc>3
            put=x['put'] and (ps or pend_p) and idx-lp>3
            if call:
                lc,pend_c=idx,False
            elif not x['call']:
                pend_c=False
            elif cs:
                pend_c=True
            if put:
                lp,pend_p=idx,False
            elif not x['put']:
                pend_p=False
            elif ps:
                pend_p=True
            x['signal']='CALL' if call else 'PUT' if put else None
            prev_c,prev_p=x['call'],x['put']
        latest=replay[-1]
        zone=latest['hour']
        begin=next((i for i in range(len(replay)-1,-1,-1)
                    if replay[i]['hour']==zone and
                    (i==0 or replay[i-1]['hour']!=zone)),None)
        start=replay[begin] if begin is not None else None
        age=(latest['t']-start['t'])//60 if start and zone else None
        move=(100*zone*(latest['price']/start['price']-1)
              if start and zone else None)
        early=(age is not None and age<=max_zone_age_minutes
               and move is not None and move<=max_move_pct)
        signal=latest['signal']
        if signal:
            status=('FRESH_' if early else 'LATE_')+signal
        elif zone and age is not None and age<=max_zone_age_minutes:
            status='EARLY_WATCH_'+('CALL' if zone==1 else 'PUT')
        else:
            status='NO_FRESH_SIGNAL'
        if spread_pct is not None and float(spread_pct)>0.35:
            status='RISK_FILTERED'
        return {**result,'status':status,'signal':signal,
                'bar_start_utc':datetime.fromtimestamp(latest['t'],timezone.utc).isoformat(),
                'bar_end_utc':datetime.fromtimestamp(latest['t']+FIVE,timezone.utc).isoformat(),
                'price':latest['price'],'zone_1h':zone,
                'entry_5m':latest['five'],'zone_age_minutes':age,
                'move_since_zone_pct':round(move,3) if move is not None else None,
                'provisional_hour':mode=='live',
                'recent_signals':[{'time':datetime.fromtimestamp(x['t'],timezone.utc).isoformat(),
                                   'signal':x['signal'],'price':x['price']}
                                  for x in replay if x['signal']][-6:]}
    except (ValueError,TypeError,IndexError,OverflowError,ZeroDivisionError) as exc:
        return {**result,'reason':str(exc)[:100]}
