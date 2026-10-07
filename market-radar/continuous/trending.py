"""Public US trend discovery. Provider lists are bounded, not the entire market."""
import json
from radar import fetch

LISTS = {
    'بحث رائج': 'https://query1.finance.yahoo.com/v1/finance/trending/US?count=250',
    'الأكثر تداولًا': 'https://query1.finance.yahoo.com/v1/finance/screener/predefined/saved?scrIds=most_actives&count=250',
    'الأكثر ارتفاعًا': 'https://query1.finance.yahoo.com/v1/finance/screener/predefined/saved?scrIds=day_gainers&count=250',
}


def normalize(results, universe):
    data={}
    for reason,quotes in results.items():
        for quote in quotes:
            symbol=quote.get('symbol','')
            if symbol not in universe:
                continue
            item=data.setdefault(symbol,{'ticker':symbol,'name':universe[symbol][-1],'trend_reasons':[],'quote':{}})
            item['trend_reasons'].append(reason)
            for key in ('regularMarketPrice','regularMarketChangePercent','regularMarketVolume','regularMarketTime','averageDailyVolume3Month','marketCap','trailingPE','forwardPE','epsTrailingTwelveMonths','priceToBook','fiftyTwoWeekHigh','fiftyTwoWeekLow'):
                value=quote.get(key)
                if isinstance(value,dict):
                    value=value.get('raw')
                if isinstance(value,(int,float)):
                    item['quote'][key]=value
    return list(data.values())


def discover(universe):
    results={}; errors={}
    for reason,url in LISTS.items():
        try:
            result=json.loads(fetch(url))['finance']['result']
            if not result:
                raise ValueError('empty provider response')
            results[reason]=result[0].get('quotes',[])
        except Exception as exc:
            errors[reason]=type(exc).__name__
    return normalize(results,universe),errors,{key:len(value) for key,value in results.items()}
