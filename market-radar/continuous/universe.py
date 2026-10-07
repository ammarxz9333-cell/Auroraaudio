"""Public Nasdaq and other-exchange directory, excluding ETFs and test issues.
Includes ADRs and other non-ETF listed securities; does not cover OTC.
"""
import csv
import io
import json
import re
import time
import urllib.request


def parse_directory(text):
    result = {}
    for row in csv.DictReader(io.StringIO(text), delimiter='|'):
        symbol = row.get('Symbol') or row.get('ACT Symbol')
        name = row.get('Security Name')
        if not symbol or not name or row.get('Test Issue') != 'N' or row.get('ETF') != 'N':
            continue
        if not re.fullmatch(r'[A-Z][A-Z0-9.\-$]{0,9}', symbol):
            continue
        company = re.split(r'\s+-\s+|\s+(?:Class [A-Z] |Common Stock|Ordinary Shares|American Depositary|Warrants|Units|Preferred)', name, maxsplit=1)[0].strip()
        result[symbol] = list(dict.fromkeys([name, company]))
    return result


def load_universe(cache):
    saved = json.loads(cache.read_text()) if cache.exists() else None
    if saved and time.time() - saved['updated'] < 86400:
        return saved['symbols']
    try:
        symbols = {}
        for filename in ('nasdaqlisted.txt', 'otherlisted.txt'):
            req = urllib.request.Request('https://www.nasdaqtrader.com/dynamic/SymDir/' + filename,
                                         headers={'User-Agent':'MarketRadar public symbol directory'})
            with urllib.request.urlopen(req, timeout=20) as response:
                parsed = parse_directory(response.read().decode('utf-8'))
            if len(parsed) < 100:
                raise ValueError('incomplete directory')
            symbols.update(parsed)
        cache.parent.mkdir(parents=True, exist_ok=True)
        temp = cache.with_suffix('.tmp')
        temp.write_text(json.dumps({'updated':time.time(), 'symbols':symbols}))
        temp.replace(cache)
        return symbols
    except Exception:
        if saved:
            return saved['symbols']
        raise
