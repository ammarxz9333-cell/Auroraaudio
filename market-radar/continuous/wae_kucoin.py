"""Public KuCoin FUTURES WAE scanner (CALL + PUT), opt-in/read-only.

Uses exchange-specific 5m/1h price history; never substitutes Bybit
prices for KuCoin execution levels. No credentials or order endpoints.
"""
import asyncio
import re
import time

from continuous.wae_early import evaluate

KLINES = 'https://api.kucoin.com/api/ua/v2/market/kline'
CONTRACTS = 'https://api-futures.kucoin.com/api/v1/contracts/active'
USDT = re.compile(r'^[A-Z0-9]{2,30}USDTM$')


def choose_contracts(rows, limit=20):
    ranked = []
    for c in rows if isinstance(rows,list) else []:
        try:
            symbol = c.get('symbol','')
            if not USDT.fullmatch(symbol):
                continue
            # Skip suspended contracts when this information is exposed.
            if c.get('status') in ('CLOSED','SUSPENDED','DELISTED'):
                continue
            if c.get('isOpen') is False:
                continue
            value = float(c.get('turnoverOf24h',0) or 0)
            if value < 500_000:
                continue
            ranked.append((value,symbol))
        except (ValueError,TypeError):
            continue
    ranked.sort(reverse=True)
    return [s for _,s in ranked[:min(40,max(1,int(limit)))]]


async def _get_json(session,url,params=None):
    async with session.get(url,params=params) as response:
        response.raise_for_status()
        return await response.json()


async def scan(session, asof=None, top_n=20, mode='live', symbols=None):
    asof = int(asof or time.time())
    output = {'strategy':'WAE_1H_5M','venue':'KuCoin Futures',
              'mode':mode, 'status':'UNAVAILABLE','checked':0,
              'fresh_calls':[], 'fresh_puts':[],
              'early_watch_calls':[], 'early_watch_puts':[],
              'late_excluded':[], 'errors':[],
              'automated_orders':False,
              'disclaimer':'Watch only. Execution permission and spread not verified.'}
    try:
        if symbols is None:
            contracts=await _get_json(session,CONTRACTS)
            if contracts.get('code')!='200000':
                raise ValueError('contract_directory_error')
            symbols=choose_contracts(contracts.get('data'),top_n)
        else:
            symbols=[s for s in dict.fromkeys(symbols) if isinstance(s,str)
                     and USDT.fullmatch(s)][:40]
        if not symbols:
            return {**output,'status':'NO_LIQUID_CONTRACTS'}
        output['requested']=len(symbols)
        sem=asyncio.Semaphore(4)

        async def inspect(symbol):
            async with sem:
                try:
                    async def candle(interval):
                        result=await _get_json(session,KLINES,{
                            'symbol':symbol,'tradeType':'FUTURES',
                            'klineType':'TRADE','interval':interval})
                        if result.get('code')!='200000':
                            raise ValueError('kucoin_kline_error')
                        payload=result.get('data',{})
                        rows=payload.get('list') if isinstance(payload,dict) else None
                        if not isinstance(rows,list):
                            raise ValueError('unexpected_kline_shape')
                        # KuCoin: start (sec), open, high, low, close, base-volume.
                        return [dict(time=int(r[0]),open=r[1],high=r[2],
                                     low=r[3],close=r[4],volume=r[5]) for r in rows]
                    five,hour=await asyncio.gather(candle('5min'),candle('1hour'))
                    return evaluate(symbol,five,hour,asof,mode=mode)
                except Exception as exc:
                    return {'symbol':symbol,'status':'UNAVAILABLE',
                            'reason':type(exc).__name__}
        reports=await asyncio.gather(*(inspect(s) for s in symbols))
        output['checked']=len(reports)
        for r in reports:
            status=r['status']
            if status=='FRESH_CALL':
                output['fresh_calls'].append(r)
            elif status=='FRESH_PUT':
                output['fresh_puts'].append(r)
            elif status=='EARLY_WATCH_CALL':
                output['early_watch_calls'].append(r)
            elif status=='EARLY_WATCH_PUT':
                output['early_watch_puts'].append(r)
            elif status.startswith('LATE_'):
                output['late_excluded'].append(r)
            elif status=='UNAVAILABLE':
                output['errors'].append({'symbol':r['symbol'],
                                         'reason':r.get('reason')})
        output['errors']=output['errors'][:20]
        output['status']='COMPLETE' if not output['errors'] else 'PARTIAL'
        output['fetched_at']=int(time.time())
    except Exception as exc:
        output['errors']=[{'reason':type(exc).__name__}]
    return output
