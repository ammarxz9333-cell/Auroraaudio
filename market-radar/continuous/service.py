"""Async adapters, authenticated local ingestion, durable Telegram delivery."""
import asyncio
import hmac
import json
import logging
import os
import random
import re
import time
from collections import defaultdict
from datetime import datetime, timedelta, timezone
from pathlib import Path
from statistics import median
from zoneinfo import ZoneInfo
from types import SimpleNamespace

import aiohttp
from aiohttp import web

from continuous.core import Engine, format_alert, timestamp, utc
from continuous.classification import classify
from continuous.trending import discover
from continuous.early import rank
from radar import parse_feed, google_news_url, yahoo_market_snapshot

LOG = logging.getLogger('radar')
NY = ZoneInfo('America/New_York')


class Service:
    def __init__(self, config, db):
        self.config = config
        self.sources = {s['id']: s for s in config['sources']}
        for source in self.sources.values():
            if source.get('type') == 'google_news':
                # Query IDs are not independent publishers. Conservative until the
                # underlying publisher provenance is resolved and verified.
                source['origin_group'] = 'google-news-aggregate'
        self.engine = Engine(db, config['universe'])
        self.status = {}
        self.quotes = {}
        self.baselines = {}
        self.session_totals = {}
        self.previous_closes = {}
        self.open_prices = {}
        self.telegram_base = 'https://api.telegram.org'
        self.stopping = asyncio.Event()
        self.session = None
        self.market_cache = {}
        self.stock_locks = {}
        self.public_requests = asyncio.Semaphore(2)
        self.engine.db.execute('CREATE TABLE IF NOT EXISTS public_scan(ticker TEXT PRIMARY KEY, attempted REAL, payload TEXT)')
        self.engine.db.execute('CREATE TABLE IF NOT EXISTS trends(ticker TEXT PRIMARY KEY, observed REAL, payload TEXT)')
        self.engine.db.execute('CREATE TABLE IF NOT EXISTS candidate_journal(ticker TEXT,day TEXT,detected REAL,price REAL,payload TEXT,last_price REAL,last_observed REAL,return_pct REAL,PRIMARY KEY(ticker,day))')

    async def trend_monitor(self):
        while not self.stopping.is_set():
            data,errors,counts=await asyncio.to_thread(discover,self.engine.universe)
            now=time.time()
            if counts:
                previous={r['ticker']:(r['observed'],json.loads(r['payload'])) for r in self.engine.db.execute('SELECT * FROM trends')}
                for item in data:
                    old=previous.get(item['ticker'])
                    quote=item['quote']; prior=old[1]['quote'] if old else {}
                    if old and 30<=now-old[0]<=180 and quote.get('regularMarketTime',0)>prior.get('regularMarketTime',0) and prior.get('regularMarketPrice',0)>0 and quote.get('regularMarketPrice',0)>0:
                        item['momentum_pct']=(quote['regularMarketPrice']/prior['regularMarketPrice']-1)*100
                with self.engine.db:
                    self.engine.db.execute('DELETE FROM trends')
                    self.engine.db.executemany('INSERT INTO trends VALUES(?,?,?)',[(d['ticker'],now,json.dumps(d)) for d in data])
                current=json.loads((await self.trends(None)).text)['data']
                day=datetime.now(NY).date().isoformat()
                with self.engine.db:
                    for item in current:
                        candidate=rank(item,now)
                        if candidate:
                            self.engine.db.execute('INSERT OR IGNORE INTO candidate_journal(ticker,day,detected,price,payload) VALUES(?,?,?,?,?)',(item['ticker'],day,now,candidate['price'],json.dumps(candidate)))
                        snap=item['snapshot']
                        if snap.get('price') and snap.get('bar_time_utc') and 0<=now-timestamp(snap['bar_time_utc'])<=120:
                            self.engine.db.execute('UPDATE candidate_journal SET last_price=?,last_observed=?,return_pct=(?/price-1)*100 WHERE ticker=? AND day=? AND ? > detected',(snap['price'],now,snap['price'],item['ticker'],day,now))
            self.status['trends']={'last_attempt':now,'matched_symbols':len(data),'provider_counts':counts,'errors':errors,'refresh_seconds':60,'scope':'public bounded US trending, actives, gainers lists; not whole-market realtime'}
            await self.pause(60)

    async def trends(self,request):
        now=time.time(); output=[]
        for row in self.engine.db.execute('SELECT observed,payload FROM trends'):
            data=json.loads(row['payload']); symbol=data['ticker']; quote=data.pop('quote')
            news=[json.loads(r[0]) for r in self.engine.db.execute('SELECT payload FROM events WHERE ticker=? AND published BETWEEN ? AND ? ORDER BY published DESC LIMIT 5',(symbol,now-86400,now))]
            alert=self.engine.db.execute("SELECT payload FROM outbox WHERE json_extract(payload,'$.ticker')=? ORDER BY rowid DESC LIMIT 1",(symbol,)).fetchone()
            snapshot={'price':quote.get('regularMarketPrice'),'change_pct':quote.get('regularMarketChangePercent'),'cum_volume':quote.get('regularMarketVolume'),'bar_time_utc':utc(quote['regularMarketTime']) if quote.get('regularMarketTime') else None}
            data.update(snapshot=snapshot,news=news,rating=classify(news,snapshot,json.loads(alert[0]) if alert else None,now),observed_at=utc(row['observed']),trend_stale=now-row['observed']>180)
            if data['trend_stale']:
                data['rating']['buyable']=False
                data['rating']['decision']='بيانات الترند قديمة — أعد التحقق'
            data['opportunity_kind']='شراء مشروط' if data['rating']['buyable'] else data['rating']['decision']
            data['fetched_at']=row['observed']
            output.append(data)
        output.sort(key=lambda d:(d['trend_stale'],d['snapshot']['price'] is None,-len(d['trend_reasons']),-(d['snapshot']['change_pct'] or 0)))
        return web.json_response({'data':output,'total':len(output),'status':self.status.get('trends',{}),'coverage':'جميع الرموز المطابقة لدليلنا في قوائم الترند العامة المتاحة؛ ليست كل أسهم السوق'})

    async def early_candidates(self,request):
        trends=json.loads((await self.trends(None)).text); data=[]
        for item in trends['data']:
            candidate=rank(item,time.time())
            if candidate:
                item['early']=candidate; data.append(item)
        data.sort(key=lambda d:-d['early']['score'])
        journal=[dict(r) for r in self.engine.db.execute('SELECT ticker,detected,price,last_price,last_observed,return_pct FROM candidate_journal ORDER BY detected DESC LIMIT 50')]
        return web.json_response({'data':data,'total':len(data),'journal':journal,'limitation':'ترتيب استكشافي؛ العائد المرصود ليس اختبار نجاح ولا يشمل تكلفة التداول أو الحركة بين اللقطات'})

    async def market_sweep(self):
        """Slow public full-directory sweep; never claim realtime coverage."""
        while not self.stopping.is_set():
            # Oldest/unattempted first makes restart continue rather than start over.
            prior = {r[0]:r[1] for r in self.engine.db.execute('SELECT ticker,attempted FROM public_scan')}
            symbols = sorted(self.engine.universe,key=lambda s:prior.get(s,0))
            for symbol in symbols:
                if self.stopping.is_set():
                    return
                async with self.public_requests:
                    snapshot = await asyncio.to_thread(yahoo_market_snapshot,symbol)
                with self.engine.db:
                    self.engine.db.execute('INSERT OR REPLACE INTO public_scan VALUES(?,?,?)',(symbol,time.time(),json.dumps(snapshot)))
                self.status['public_scan']={'last_symbol':symbol,'last_attempt':time.time(),'mode':'slow public snapshots, not realtime'}
                await self.pause(2)

    async def stocks(self, request):
        query = request.query.get('q', '').strip().lower()[:100]
        try:
            offset = max(0, int(request.query.get('offset', '0')))
            limit = min(25, max(1, int(request.query.get('limit', '12'))))
        except ValueError:
            raise web.HTTPBadRequest()
        news_symbols = {r[0] for r in self.engine.db.execute('SELECT DISTINCT ticker FROM events WHERE published>=?',(time.time()-86400,))}
        symbols = [s for s,names in self.engine.universe.items() if not query or query in s.lower() or any(query in n.lower() for n in names)]
        if request.query.get('news') == '1':
            symbols = [s for s in symbols if s in news_symbols]
        symbols.sort(key=lambda s: (s not in news_symbols, s not in ('PENG','AAPL','NVDA','MSFT','AMZN','GOOGL','TSLA','META','CRWV'), s))
        return web.json_response({'total':len(symbols),'offset':offset,'limit':limit,
            'data':[{'ticker':s,'name':self.engine.universe[s][-1],'has_news':s in news_symbols} for s in symbols[offset:offset+limit]]})

    async def opportunities(self, request):
        now=time.time()
        candidates={}
        catalysts={}
        for row in self.engine.db.execute('SELECT ticker,payload,published FROM events WHERE published BETWEEN ? AND ? ORDER BY published DESC',(now-86400,now)):
            event=json.loads(row['payload'])
            # "Acquisition Corporation" in an issuer name and a generic earnings
            # loss are not positive catalysts.
            catalyst=re.search(r'\b(beats?|approval|approved|contract|partnership|merger)\b|\brais\w*.*(?:guidance|outlook)|\bacqui(?:res|red|sition of)\b|business combination agreement|\bAI\b.*(?:cloud|demand|infrastructure|contract)',event['title'],re.I)
            negative=re.search(r'\b(loss|misses|missed|lowers|lowered|cuts|cut guidance|investigation|fraud)\b',event['title'],re.I)
            if event.get('confidence',0)>=55 and catalyst and not negative:
                candidates.setdefault(row['ticker'],row['published'])
                catalysts.setdefault(row['ticker'],event['title'])
        # Bounded enrichment; keep the HTTP response useful even when public feeds fail.
        async def inspect(symbol):
            try:
                response=await asyncio.wait_for(self.stock_details(SimpleNamespace(match_info={'ticker':symbol})),30)
                return json.loads(response.text)
            except (Exception,asyncio.TimeoutError):
                return None
        results=await asyncio.gather(*(inspect(s) for s in list(candidates)[:25]))
        eligible=[]
        watch=[]
        excluded=[]
        for data in results:
            if not data:
                continue
            rating=data['rating']; snapshot=data.get('snapshot') or {}
            data['catalyst_title']=catalysts[data['ticker']]
            try:
                fresh=0<=time.time()-timestamp(snapshot['bar_time_utc'])<=600
            except (KeyError,ValueError,TypeError):
                fresh=False
            if rating['classification'] in ('LATE','AVOID'):
                data['opportunity_kind']='مستبعد: '+rating['label']
                excluded.append(data)
                continue
            if not fresh or snapshot.get('price') is None or (not rating['buyable'] and (snapshot.get('holds_vwap') is not True or snapshot.get('change_pct',-1)<0)):
                data['opportunity_kind']='قائمة انتظار — خبر محفّز، شروط السعر لم تكتمل'
                data['watch_reason']='السعر غير متاح أو قديم' if not fresh or snapshot.get('price') is None else 'انتظار استعادة VWAP وتحسن حركة السعر'
                watch.append(data)
                continue
            data['opportunity_kind']='فرصة دخول مشروطة' if rating['buyable'] else 'مرشّح للمتابعة — الدخول غير مؤكد'
            eligible.append(data)
        eligible.sort(key=lambda d:(not d['rating']['buyable'],-(d['rating']['score'] or 0),-candidates[d['ticker']]))
        watch.sort(key=lambda d:(not bool((d.get('snapshot') or {}).get('price')),-candidates[d['ticker']]))
        return web.json_response({'data':eligible,'watch':watch,'excluded':excluded,'candidate_count':len(candidates),'evaluated':sum(d is not None for d in results),'unexamined':max(0,len(candidates)-25),'confirmed_entries':sum(d['rating']['buyable'] for d in eligible),'updated_at':utc(time.time())})

    async def stock_details(self, request):
        symbol = request.match_info['ticker'].upper()
        if symbol not in self.engine.universe:
            raise web.HTTPNotFound()
        async with self.stock_locks.setdefault(symbol, asyncio.Lock()):
            cached = self.market_cache.get(symbol)
            if cached and time.time() - cached['fetched_at'] < 60:
                return web.json_response(cached)
            async with self.public_requests:
                existing = self.engine.db.execute('SELECT payload FROM events WHERE ticker=? AND published>=? ORDER BY published DESC LIMIT 1',(symbol,time.time()-86400)).fetchone()
                if not existing:
                    policy = {'id':'public-company-news','origin_group':'google-news-aggregate','type':'google_news','public':True,'confidence':55,'official':False}
                    query = '"' + self.engine.universe[symbol][-1] + '" stock'
                    try:
                        async with self.session.get(google_news_url(query)) as response:
                            response.raise_for_status()
                            raw = await response.content.read(4_000_001)
                            if len(raw) <= 4_000_000:
                                for item in list(parse_feed(raw, policy))[:15]:
                                    if item['published']:
                                        try:
                                            self.engine.event(dict(title=item['title'],content=item['snippet'],url=item['url'],published_at=item['published'].isoformat()),policy,time.time())
                                        except ValueError:
                                            pass
                    except Exception:
                        pass
                rows = self.engine.db.execute('SELECT payload FROM events WHERE ticker=? ORDER BY published DESC LIMIT 5',(symbol,)).fetchall()
                news = [json.loads(r['payload']) for r in rows]
                # Headlines can concern unrelated catalysts. The oldest issuer headline
                # must never become a fabricated price anchor for the current catalyst.
                scanned = self.engine.db.execute('SELECT attempted,payload FROM public_scan WHERE ticker=?',(symbol,)).fetchone()
                snapshot = json.loads(scanned['payload']) if scanned and time.time()-scanned['attempted']<60 else await asyncio.to_thread(yahoo_market_snapshot, symbol)
                self.engine.evaluate(symbol,time.time())
                latest = self.engine.db.execute("SELECT payload FROM outbox WHERE json_extract(payload,'$.ticker')=? ORDER BY rowid DESC LIMIT 1",(symbol,)).fetchone()
                recent_news = [json.loads(r[0]) for r in self.engine.db.execute('SELECT payload FROM events WHERE ticker=? AND published BETWEEN ? AND ?',(symbol,time.time()-86400,time.time()))]
                rating = classify(recent_news,snapshot,json.loads(latest[0]) if latest else None,time.time())
                payload = {'ticker':symbol,'name':self.engine.universe[symbol][-1], 'news':news,
                    'rating':rating,
                    'snapshot':snapshot,'fetched_at':time.time(), 'price_source':'Yahoo Finance public 5-minute snapshot; not authenticated real-time tape',
                    'stream_confirmed':False}
                self.market_cache[symbol] = payload
                return web.json_response(payload)

    async def ingest(self, request):
        key = os.getenv('RADAR_INGEST_TOKEN', '')
        if not key or not hmac.compare_digest(request.headers.get('Authorization', ''), 'Bearer ' + key):
            raise web.HTTPUnauthorized()
        try:
            data = await request.json()
            source = self.sources[data['source_id']]
            if not source.get('allow_push'):
                raise ValueError('push disabled for source')
            self.engine.event(data, source, time.time())
        except (KeyError, TypeError, ValueError):
            raise web.HTTPBadRequest(text='invalid public-source event')
        return web.json_response({'accepted': True})

    async def health(self, request):
        now = time.time()
        pending = self.engine.db.execute('SELECT count(*) FROM outbox WHERE sent=0').fetchone()[0]
        tape_age = now - self.status.get('tape', {}).get('last_ok', 0)
        scan = self.engine.db.execute("SELECT COUNT(*),SUM(CASE WHEN payload!='null' THEN 1 ELSE 0 END),SUM(CASE WHEN attempted>=? AND payload!='null' THEN 1 ELSE 0 END) FROM public_scan",(now-300,)).fetchone()
        live_symbols = self.engine.db.execute('SELECT COUNT(DISTINCT ticker) FROM bars WHERE time>=?',(now-90,)).fetchone()[0]
        return web.json_response({'alive': True, 'ready': tape_age < 120 and bool(os.getenv('TELEGRAM_BOT_TOKEN')) and bool(os.getenv('TELEGRAM_CHAT_ID')),
                                 'coverage':{'public_attempted':scan[0],'public_available':scan[1] or 0,'public_fetched_last_5m':scan[2] or 0,'live_tape_symbols':live_symbols,'full_market_realtime':False,'minimum_public_cycle_minutes':round(len(self.engine.universe)*2/60)},
                                 'tape_age_seconds': tape_age, 'outbox_pending': pending, 'universe_symbols':len(self.engine.universe), 'news_sources':len(self.sources), 'adapters': self.status})

    async def local_alerts(self, request):
        rows = self.engine.db.execute("SELECT payload,sent FROM outbox WHERE rowid IN (SELECT MAX(rowid) FROM outbox GROUP BY json_extract(payload,'$.ticker')) ORDER BY rowid DESC LIMIT 300").fetchall()
        return web.json_response([{'text': format_alert(json.loads(r['payload'])), 'sent': bool(r['sent']), 'alert':json.loads(r['payload'])} for r in rows])

    async def dashboard(self, request):
        return web.Response(text=Path('continuous/dashboard.html').read_text(encoding='utf-8'), content_type='text/html')

    async def poll(self, source):
        interval = max(10, source.get('interval_seconds', 30))
        etag = None
        while not self.stopping.is_set():
            wait = interval
            try:
                url = google_news_url(source['query']) if source['type'] == 'google_news' else source['url']
                async with self.session.get(url, headers={'If-None-Match': etag} if etag else {}) as response:
                    if response.status == 429:
                        wait = max(interval, float(response.headers.get('Retry-After', 60)))
                        self.status[source['id']]={'error':'rate_limited','http_status':429,'retry_seconds':wait,'at':time.time()}
                    elif response.status != 304:
                        response.raise_for_status()
                        raw = await response.content.read(4_000_001)
                        if len(raw) > 4_000_000:
                            raise ValueError('feed too large')
                        etag = response.headers.get('ETag')
                        for item in parse_feed(raw, source):
                            if item['published'] is None:
                                continue
                            event = dict(title=item['title'], content=item['snippet'], url=item['url'], published_at=item['published'].isoformat())
                            try:
                                self.engine.event(event, source, time.time())
                            except ValueError:
                                LOG.warning('Rejected feed item from %s', source['id'])
                    if response.status != 429:
                        self.status[source['id']] = {'last_ok': time.time(), 'http_status': response.status}
            except Exception as exc:
                self.status[source['id']] = {'error': type(exc).__name__, 'at': time.time()}
                LOG.warning('Source %s unavailable: %s', source['id'], type(exc).__name__)
                wait = max(wait, 60)
            await self.pause(wait + random.random())

    async def pause(self, seconds):
        try:
            await asyncio.wait_for(self.stopping.wait(), seconds)
        except asyncio.TimeoutError:
            pass

    async def deliver(self):
        token, chat = os.getenv('TELEGRAM_BOT_TOKEN'), os.getenv('TELEGRAM_CHAT_ID')
        while not self.stopping.is_set():
            if not token or not chat:
                await self.pause(5)
                continue
            rows = self.engine.db.execute('SELECT * FROM outbox WHERE sent=0 AND next_try<=? LIMIT 20', (time.time(),)).fetchall()
            for row in rows:
                try:
                    async with self.session.post(f'{self.telegram_base}/bot{token}/sendMessage', json={'chat_id': chat, 'text': format_alert(json.loads(row['payload'])), 'disable_web_page_preview': True}) as response:
                        result = await response.json()
                        if not result.get('ok'):
                            delay = result.get('parameters', {}).get('retry_after', min(300, 2 ** min(row['attempts'] + 1, 8)))
                            raise DeliveryRetry(delay)
                    with self.engine.db:
                        self.engine.db.execute('UPDATE outbox SET sent=1 WHERE id=?', (row['id'],))
                except Exception as exc:
                    delay = exc.delay if isinstance(exc, DeliveryRetry) else 30
                    with self.engine.db:
                        self.engine.db.execute('UPDATE outbox SET attempts=attempts+1,next_try=? WHERE id=?', (time.time() + delay, row['id']))
                    LOG.warning('Telegram delivery failed (%s); queued retry', type(exc).__name__)
            with self.engine.db:
                self.engine.db.execute('DELETE FROM outbox WHERE sent=1 AND next_try<? AND rowid NOT IN (SELECT rowid FROM outbox ORDER BY rowid DESC LIMIT 5000)', (time.time() - 86400,))
            await self.pause(1)

    async def seed_baselines(self, headers, feed):
        """Only completed prior NY sessions enter volume baseline; paginate all bars."""
        today = datetime.now(NY).date()
        start = datetime.now(timezone.utc) - timedelta(days=35)
        symbols = self.tape_symbols(feed)
        for offset in range(0, len(symbols), 100):
            history = defaultdict(lambda: defaultdict(list))
            params = {'symbols': ','.join(symbols[offset:offset+100]), 'timeframe': '1Min', 'start': start.isoformat(),
                      'end': datetime.now(timezone.utc).isoformat(), 'feed': feed, 'limit': 10000, 'adjustment': 'raw', 'sort':'asc'}
            while True:
                async with self.session.get('https://data.alpaca.markets/v2/stocks/bars', params=params, headers=headers) as response:
                    response.raise_for_status()
                    result = await response.json()
                for symbol, bars in result.get('bars', {}).items():
                    for b in bars:
                        stamp = timestamp(b['t'])
                        local = datetime.fromtimestamp(stamp, NY)
                        slot = local.strftime('%H:%M')
                        if local.date() < today:
                            history[symbol][slot].append((local.date().isoformat(), b['v']))
                            if slot == '15:59':
                                self.previous_closes[symbol] = b['c']
                                with self.engine.db:
                                    self.engine.db.execute('INSERT OR REPLACE INTO bars VALUES(?,?,?)',(symbol,stamp+60,json.dumps(dict(ticker=symbol,time=utc(stamp+60),open=b['o'],high=b['h'],low=b['l'],close=b['c'],volume=b['v'],vwap=b.get('vw',b['c']),spread_pct=None))))
                        elif stamp + 60 <= time.time():
                            # Today's completed bars provide point-in-time price anchors and session VWAP.
                            self.consume_bar(dict(b, S=symbol), seed=True)
                page = result.get('next_page_token')
                if not page:
                    break
                params['page_token'] = page
            for symbol, slots in history.items():
                self.baselines[symbol] = {slot: {'volume': median(v for _, v in values[-20:]), 'days': len({d for d, _ in values[-20:]}),
                                                   'asof': utc(datetime.combine(today, datetime.min.time(), NY).timestamp())}
                                          for slot, values in slots.items() if median(v for _, v in values[-20:]) > 0}
        self.status['baseline'] = {'last_ok': time.time(), 'symbols': len(self.baselines)}

    def consume_bar(self, b, seed=False):
        symbol = b['S']
        stamp = timestamp(b['t'])
        local = datetime.fromtimestamp(stamp, NY)
        session_key = (symbol, local.date().isoformat())
        totals = self.session_totals.setdefault(session_key, {})
        if local.strftime('%H:%M') == '09:30':
            self.open_prices[session_key] = b['o']
        totals[stamp] = (b['v'], b.get('vw', b['c']))
        # Recompute so corrected bars and reconnect replay cannot double count.
        eligible = [(v, p) for t, (v, p) in totals.items() if t <= stamp]
        volume = sum(v for v, _ in eligible)
        vwap = sum(v*p for v, p in eligible) / volume if volume else b['c']
        baseline = self.baselines.get(symbol, {}).get(local.strftime('%H:%M'), {})
        quote = self.quotes.get(symbol)
        spread = (quote['ap'] / quote['bp'] - 1) * 100 if quote and quote['bp'] > 0 and abs(timestamp(quote['t']) - (stamp + 60)) <= 10 else None
        previous_close = self.previous_closes.get(symbol)
        opening = self.open_prices.get(session_key)
        benchmark = self.engine.db.execute('SELECT payload FROM bars WHERE ticker=? AND time=?', ('SPY',stamp+60)).fetchone()
        benchmark_close = self.previous_closes.get('SPY')
        relative = ((b['c']/previous_close-1) - (json.loads(benchmark['payload'])['close']/benchmark_close-1))*100 if previous_close and benchmark and benchmark_close else None
        bar = dict(ticker=symbol, time=utc(stamp + 60), open=b['o'], high=b['h'], low=b['l'], close=b['c'], volume=b['v'],
                   gap_pct=(opening/previous_close-1)*100 if opening and previous_close else None, relative_strength_pct=relative,
                   vwap=vwap, spread_pct=spread, baseline_volume=baseline.get('volume'), baseline_days=baseline.get('days', 0), baseline_asof=baseline.get('asof'))
        if seed:
            with self.engine.db:
                self.engine.db.execute('INSERT OR REPLACE INTO bars VALUES(?,?,?)', (symbol, stamp+60, json.dumps(bar)))
        else:
            self.engine.bar(bar, time.time())
            self.status['tape'] = {'last_ok': time.time(), 'feed': os.getenv('ALPACA_FEED', 'sip')}

    def tape_symbols(self, feed):
        symbols = list(self.engine.universe)
        if feed == 'iex':
            # Basic accounts permit 30 concurrent symbols; never reconnect forever
            # with an oversized subscription. News collection remains independent.
            priority = [s for s in ('PENG', 'SPY') if s in symbols]
            symbols = (priority + [s for s in symbols if s not in priority])[:30]
        self.status['tape_coverage'] = {'feed': feed, 'symbols': symbols,
            'excluded': len(self.engine.universe) - len(symbols),
            'volume_basis': 'IEX venue only' if feed == 'iex' else 'consolidated SIP'}
        return symbols

    async def tape(self):
        key, secret = os.getenv('ALPACA_API_KEY'), os.getenv('ALPACA_SECRET_KEY')
        if not key or not secret:
            self.status['tape'] = {'error': 'missing Alpaca credentials'}
            return
        feed = os.getenv('ALPACA_FEED', 'sip')
        if feed not in ('sip', 'iex'):
            raise ValueError('unsupported feed; delayed feeds cannot claim early detection')
        headers = {'APCA-API-KEY-ID': key, 'APCA-API-SECRET-KEY': secret}
        delay = 1
        seeded_day = None
        while not self.stopping.is_set():
            try:
                if seeded_day != datetime.now(NY).date():
                    self.baselines.clear()
                    self.session_totals.clear()
                    await self.seed_baselines(headers, feed)
                    seeded_day = datetime.now(NY).date()
                async with self.session.ws_connect('wss://stream.data.alpaca.markets/v2/' + feed, heartbeat=20) as ws:
                    await ws.send_json({'action': 'auth', 'key': key, 'secret': secret})
                    subscribed = False
                    async for message in ws:
                        if message.type != aiohttp.WSMsgType.TEXT:
                            break
                        for b in json.loads(message.data):
                            if b.get('T') == 'error':
                                raise ValueError('provider rejected authentication/subscription')
                            if b.get('T') == 'success' and b.get('msg') == 'authenticated':
                                symbols = self.tape_symbols(feed)
                                await ws.send_json({'action': 'subscribe', 'bars': symbols, 'quotes': symbols})
                                subscribed = True
                            elif b.get('T') == 'q':
                                self.quotes[b['S']] = b
                            elif b.get('T') == 'b' and subscribed:
                                self.consume_bar(b)
                                delay = 1
                        if seeded_day != datetime.now(NY).date():
                            break
            except Exception as exc:
                self.status['tape'] = {'error': type(exc).__name__, 'at': time.time()}
                LOG.warning('Tape disconnected (%s); reconnecting', type(exc).__name__)
            await self.pause(delay)
            delay = min(60, delay * 2)

    async def radar_bridge(self):
        """Read-only bridge to Radar Intelligence's actual mentions schema."""
        dsn = os.getenv('RADAR_INTELLIGENCE_DSN')
        if not dsn:
            return
        import psycopg
        project = int(os.environ['RADAR_INTELLIGENCE_PROJECT_ID'])
        while not self.stopping.is_set():
            try:
                async with await psycopg.AsyncConnection.connect(dsn) as conn:
                    async with conn.cursor() as cursor:
                        # Sliding window intentionally re-reads rows: insert dedupe handles late commits and reordering.
                        await cursor.execute('SELECT source,url,title,content,published_at FROM mentions WHERE project_id=%s AND published_at >= now() - interval \'24 hours\' ORDER BY published_at', (project,))
                        for source, url, title, content, published in await cursor.fetchall():
                            policy = self.sources.get('ri:' + source)
                            if policy and url and published and policy.get('public'):
                                self.engine.event(dict(url=url, title=title or '', content=content, published_at=published.isoformat()), policy, time.time())
                self.status['radar_intelligence'] = {'last_ok': time.time()}
            except Exception as exc:
                self.status['radar_intelligence'] = {'error': type(exc).__name__}
                LOG.warning('Radar Intelligence bridge unavailable (%s)', type(exc).__name__)
            await self.pause(10)

    async def maintain(self):
        universe_checked = time.time()
        while not self.stopping.is_set():
            if os.getenv('RADAR_NASDAQ_UNIVERSE') == '1' and time.time() - universe_checked >= 86400:
                from continuous.universe import load_universe
                broad = await asyncio.to_thread(load_universe, Path('runtime/universe.json'))
                broad.update(self.config['universe'])
                self.engine.universe.update(broad)
                universe_checked = time.time()
            now=time.time()
            with self.engine.db:
                self.engine.db.execute('DELETE FROM bars WHERE time<?',(now-172800,))
                self.engine.db.execute('DELETE FROM events WHERE published<?',(now-172800,))
                self.engine.db.execute("DELETE FROM meta WHERE key NOT IN (SELECT ticker || ':' || story FROM events)")
            today=datetime.now(NY).date().isoformat()
            self.session_totals={k:v for k,v in self.session_totals.items() if k[1]==today}
            self.open_prices={k:v for k,v in self.open_prices.items() if k[1]==today}
            await self.pause(600)

    async def run(self):
        app = web.Application(client_max_size=128*1024)
        app.add_routes([web.get('/', self.dashboard),web.get('/early',self.early_candidates),web.get('/trends',self.trends),web.get('/opportunities',self.opportunities), web.get('/stocks',self.stocks),web.get('/stock/{ticker}',self.stock_details),web.get('/alerts', self.local_alerts), web.get('/health', self.health), web.post('/events', self.ingest)])
        runner = web.AppRunner(app)
        await runner.setup()
        await web.TCPSite(runner, os.getenv('RADAR_BIND', '127.0.0.1'), int(os.getenv('RADAR_PORT', '8787'))).start()
        timeout = aiohttp.ClientTimeout(total=15)
        async with aiohttp.ClientSession(timeout=timeout, headers={'User-Agent': os.getenv('RADAR_USER_AGENT', 'MarketRadar public-source monitor')}) as session:
            self.session = session
            if os.getenv('RADAR_FULL_UNIVERSE') == '1':
                try:
                    async with session.get('https://www.sec.gov/files/company_tickers.json') as response:
                        response.raise_for_status()
                        issuers = await response.json(content_type=None)
                    for issuer in issuers.values():
                        self.engine.universe.setdefault(issuer['ticker'],[issuer['title']])
                    self.status['universe'] = {'symbols':len(self.engine.universe), 'origin':'SEC issuer directory; includes securities beyond common stock'}
                except Exception as exc:
                    raise RuntimeError('Full universe requested but directory unavailable') from exc
            self.engine.universe.setdefault('SPY',['SPDR S&P 500 ETF Trust'])
            tasks = [asyncio.create_task(self.deliver()), asyncio.create_task(self.tape()), asyncio.create_task(self.radar_bridge()),asyncio.create_task(self.maintain()),asyncio.create_task(self.market_sweep()),asyncio.create_task(self.trend_monitor())]
            tasks.extend(asyncio.create_task(self.poll(s)) for s in self.sources.values() if s.get('enabled',True) and s.get('type') in ('rss', 'google_news'))
            try:
                await asyncio.gather(*tasks)
            finally:
                self.stopping.set()
                for task in tasks:
                    task.cancel()
                await asyncio.gather(*tasks, return_exceptions=True)
                await runner.cleanup()
                self.engine.db.close()


class DeliveryRetry(Exception):
    def __init__(self, delay):
        self.delay = max(1, float(delay))


def main():
    logging.basicConfig(level=logging.INFO, format='%(asctime)s %(levelname)s %(message)s')
    config = json.loads(Path(os.getenv('RADAR_CONFIG', 'continuous/sources.json')).read_text())
    if os.getenv('RADAR_NASDAQ_UNIVERSE') == '1':
        from continuous.universe import load_universe
        broad = load_universe(Path('runtime/universe.json'))
        broad.update(config['universe'])
        config['universe'] = broad
    if os.getenv('RADAR_UNIVERSE_FILE'):
        config['universe'].update(json.loads(Path(os.environ['RADAR_UNIVERSE_FILE']).read_text()))
    db = Path(os.getenv('RADAR_DB', 'runtime/radar.sqlite'))
    db.parent.mkdir(parents=True, exist_ok=True)
    asyncio.run(Service(config, str(db)).run())


if __name__ == '__main__':
    main()
