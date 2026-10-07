"""Async adapters, authenticated local ingestion, durable Telegram delivery."""
import asyncio
import hmac
import json
import logging
import os
import random
import time
from collections import defaultdict
from datetime import datetime, timedelta, timezone
from pathlib import Path
from statistics import median
from zoneinfo import ZoneInfo

import aiohttp
from aiohttp import web

from continuous.core import Engine, format_alert, timestamp, utc
from radar import parse_feed, google_news_url

LOG = logging.getLogger('radar')
NY = ZoneInfo('America/New_York')


class Service:
    def __init__(self, config, db):
        self.config = config
        self.sources = {s['id']: s for s in config['sources']}
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
        return web.json_response({'alive': True, 'ready': tape_age < 120 and bool(os.getenv('TELEGRAM_BOT_TOKEN')) and bool(os.getenv('TELEGRAM_CHAT_ID')),
                                 'tape_age_seconds': tape_age, 'outbox_pending': pending, 'adapters': self.status})

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
        symbols = list(self.engine.universe)
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
                                symbols = list(self.engine.universe)
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
        while not self.stopping.is_set():
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
        app.add_routes([web.get('/health', self.health), web.post('/events', self.ingest)])
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
            tasks = [asyncio.create_task(self.deliver()), asyncio.create_task(self.tape()), asyncio.create_task(self.radar_bridge()),asyncio.create_task(self.maintain())]
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
    if os.getenv('RADAR_UNIVERSE_FILE'):
        config['universe'].update(json.loads(Path(os.environ['RADAR_UNIVERSE_FILE']).read_text()))
    db = Path(os.getenv('RADAR_DB', 'runtime/radar.sqlite'))
    db.parent.mkdir(parents=True, exist_ok=True)
    asyncio.run(Service(config, str(db)).run())


if __name__ == '__main__':
    main()
