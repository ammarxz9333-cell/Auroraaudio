import asyncio
import json
import os
import tempfile
import time
import unittest
from unittest.mock import patch

import aiohttp
from aiohttp import web
from aiohttp.test_utils import TestServer, TestClient

from continuous.service import Service
from continuous.core import utc


class ServiceTest(unittest.IsolatedAsyncioTestCase):
    async def test_free_feed_subscription_limit_and_coverage(self):
        self.s.engine.universe.update({f'T{i}': [f'Issuer {i}'] for i in range(40)})
        self.s.engine.universe['SPY'] = ['SPDR']
        symbols = self.s.tape_symbols('iex')
        self.assertEqual(len(symbols), 30)
        self.assertEqual(symbols[:2], ['PENG', 'SPY'])
        self.assertGreater(self.s.status['tape_coverage']['excluded'], 0)
        self.assertEqual(len(self.s.tape_symbols('sip')), len(self.s.engine.universe))

    async def asyncSetUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.s = Service({'universe':{'PENG':['Penguin Solutions']},'sources':[{'id':'ir','public':True,'official':True,'confidence':95,'allow_push':True}]},self.tmp.name+'/radar.db')
        self.env = patch.dict(os.environ,{'RADAR_INGEST_TOKEN':'test-token','TELEGRAM_BOT_TOKEN':'fake-token','TELEGRAM_CHAT_ID':'test-chat'})
        self.env.start()
        app = web.Application()
        app.router.add_post('/events',self.s.ingest)
        app.router.add_get('/health',self.s.health)
        app.router.add_get('/stocks',self.s.stocks)
        app.router.add_get('/opportunities',self.s.opportunities)
        app.router.add_get('/trends',self.s.trends)
        app.router.add_get('/stock/{ticker}',self.s.stock_details)
        self.client=TestClient(TestServer(app))
        await self.client.start_server()

    async def asyncTearDown(self):
        await self.client.close()
        self.env.stop()
        self.s.engine.db.close()
        self.tmp.cleanup()

    async def test_authenticated_ingestion_to_telegram_retry(self):
        event=dict(source_id='ir',title='Penguin Solutions raises outlook on AI demand',url='https://example.org/public',published_at=utc(time.time()-1))
        response=await self.client.post('/events',json=event)
        self.assertEqual(response.status,401)
        response=await self.client.post('/events',json=event,headers={'Authorization':'Bearer test-token'})
        self.assertEqual(response.status,200)
        attempts=[]
        async def telegram(request):
            payload=await request.json()
            attempts.append(payload)
            return web.json_response({'ok':len(attempts)>1,'parameters':{'retry_after':1}})
        app=web.Application()
        app.router.add_post('/botfake-token/sendMessage',telegram)
        server=TestServer(app)
        await server.start_server()
        self.s.telegram_base=str(server.make_url('')).rstrip('/')
        async with aiohttp.ClientSession() as session:
            self.s.session=session
            task=asyncio.create_task(self.s.deliver())
            try:
                for _ in range(50):
                    if self.s.engine.db.execute('SELECT sent FROM outbox').fetchone()[0]: break
                    await asyncio.sleep(.1)
                self.assertEqual(self.s.engine.db.execute('SELECT sent FROM outbox').fetchone()[0],1)
                self.assertEqual(len(attempts),2)
                self.assertIn('DEVELOPING',attempts[1]['text'])
                self.assertIn('Buyable by rule: False',attempts[1]['text'])
            finally:
                self.s.stopping.set()
                await task
                await server.close()

    async def test_missing_tape_not_ready(self):
        response=await self.client.get('/health')
        self.assertFalse((await response.json())['ready'])

    async def test_analysis_missing_evidence_and_late_advice(self):
        from continuous.dossier import analyze
        stock={'ticker':'PENG','name':'Penguin','snapshot':{'price':74,'bar_time_utc':utc(time.time())},'news':[],
               'rating':{'classification':'LATE','buyable':False,'reasons':['chase']}}
        report=analyze(stock,{'epsTrailingTwelveMonths':-2},time.time())
        self.assertIn('لا تطارد',report['advice'])
        self.assertIsNone(report['scenarios']['target'])
        self.assertIsNone(report['scenarios']['risk_reward'])
        self.assertTrue(any('SEC' in item for item in report['missing']))
        self.assertIn('سالبة',report['valuation']['interpretation'][0])
        stock['rating']['classification']='DEVELOPING'
        stock['snapshot']['bar_time_utc']=None
        report=analyze(stock,{},time.time())
        self.assertIn('انتظار',report['advice'])
        self.assertIn('السعر غير متاح أو قديم',report['missing'])

    async def test_nomination_price_is_fixed_and_ranking_never_enables_buy(self):
        from continuous.early import rank
        now=time.time()
        quote={'regularMarketPrice':65,'regularMarketChangePercent':3,'regularMarketTime':now,'regularMarketVolume':200000}
        item={'ticker':'PENG','name':'Penguin','trend_reasons':['الأكثر تداولًا','الأكثر ارتفاعًا'],'quote':quote}
        async def stop(seconds):self.s.stopping.set()
        for price in (65,66):
            item=dict(item,quote=dict(quote,regularMarketPrice=price))
            self.s.stopping.clear()
            with patch('continuous.service.discover',return_value=([item],{}, {'active':1})),patch.object(self.s,'pause',stop):
                await self.s.trend_monitor()
        row=self.s.engine.db.execute('SELECT * FROM candidate_journal').fetchone()
        self.assertEqual(row['price'],65)
        self.assertEqual(row['last_price'],66)
        data=json.loads((await self.s.trends(None)).text)['data'][0]
        result=rank(data,time.time())
        self.assertFalse(result['buyable'])
        self.assertIn('غير مؤكد',result['label'])
        data['snapshot']['bar_time_utc']=utc(now-1000)
        self.assertIsNone(rank(data,time.time()))

    async def test_trend_discovery_dedupes_and_failure_preserves_stale_list(self):
        from continuous.trending import normalize
        results={'بحث رائج':[{'symbol':'PENG'},{'symbol':'BTC-USD'}],
            'الأكثر ارتفاعًا':[{'symbol':'PENG','regularMarketPrice':{'raw':74},'regularMarketChangePercent':12}]}
        data=normalize(results,self.s.engine.universe)
        self.assertEqual(len(data),1)
        self.assertEqual(len(data[0]['trend_reasons']),2)
        async def stop(seconds):self.s.stopping.set()
        with patch('continuous.service.discover',return_value=(data,{}, {'trending':2})),patch.object(self.s,'pause',stop):
            await self.s.trend_monitor()
        response=await (await self.client.get('/trends')).json()
        self.assertEqual(response['total'],1)
        self.assertEqual(response['data'][0]['rating']['classification'],'LATE')
        self.assertFalse(response['data'][0]['rating']['buyable'])
        self.s.stopping.clear()
        with patch('continuous.service.discover',return_value=([],{'trending':'HTTPError'},{})),patch.object(self.s,'pause',stop):
            await self.s.trend_monitor()
        self.s.engine.db.execute('UPDATE trends SET observed=?',(time.time()-200,))
        response=await (await self.client.get('/trends')).json()
        self.assertEqual(response['total'],1)
        self.assertTrue(response['data'][0]['trend_stale'])
        self.assertFalse(response['data'][0]['rating']['buyable'])

    async def test_opportunities_require_fresh_price_and_reject_chasing(self):
        now=time.time()
        self.s.engine.event(dict(title='Penguin Solutions earnings beat',url='https://example.org/beat',published_at=utc(now-10)),self.s.sources['ir'],now)
        snapshot=dict(price=65,change_pct=2,holds_vwap=True,bar_time_utc=utc(now))
        with patch('continuous.service.yahoo_market_snapshot',return_value=snapshot):
            data=await (await self.client.get('/opportunities')).json()
        self.assertEqual(len(data['data']),1)
        self.assertEqual(data['confirmed_entries'],0)
        self.assertFalse(data['data'][0]['rating']['buyable'])
        self.s.market_cache.clear()
        with patch('continuous.service.yahoo_market_snapshot',return_value=dict(snapshot,change_pct=12)):
            data=await (await self.client.get('/opportunities')).json()
        self.assertEqual(data['data'],[])
        self.assertEqual(data['excluded'][0]['ticker'],'PENG')
        self.s.market_cache.clear()
        with patch('continuous.service.yahoo_market_snapshot',return_value=dict(snapshot,bar_time_utc=utc(now-3600))):
            data=await (await self.client.get('/opportunities')).json()
        self.assertEqual(data['data'],[])
        self.assertEqual(data['watch'][0]['ticker'],'PENG')
        self.assertFalse(data['watch'][0]['rating']['buyable'])
        self.assertIn('earnings beat',data['watch'][0]['catalyst_title'])

    async def test_background_sweep_resumes_unattempted_symbols(self):
        self.s.engine.universe['ZZZ']=['Example Company']
        async def stop_after_one(seconds):
            self.s.stopping.set()
        with patch('continuous.service.yahoo_market_snapshot',return_value={'price':65}) as fetch, patch.object(self.s,'pause',stop_after_one):
            await self.s.market_sweep()
            self.s.stopping.clear()
            await self.s.market_sweep()
            self.assertEqual([c.args[0] for c in fetch.call_args_list],['PENG','ZZZ'])
        coverage=(await (await self.client.get('/health')).json())['coverage']
        self.assertEqual(coverage['public_attempted'],2)
        self.assertEqual(coverage['public_available'],2)
        self.assertEqual(coverage['live_tape_symbols'],0)
        self.assertFalse(coverage['full_market_realtime'])

    async def test_directory_search_and_pagination(self):
        self.s.engine.universe.update({f'T{i}':[f'Issuer {i}'] for i in range(40)})
        response=await self.client.get('/stocks?limit=12&offset=12')
        data=await response.json()
        self.assertEqual(data['total'],41)
        self.assertEqual(len(data['data']),12)
        data=await (await self.client.get('/stocks?q=Penguin')).json()
        self.assertEqual([s['ticker'] for s in data['data']],['PENG'])

    async def test_snapshot_cache_and_no_unrelated_news_anchor(self):
        self.s.engine.event(dict(title='Penguin Solutions earnings',url='https://example.org/public',published_at=utc(time.time()-100)),self.s.sources['ir'],time.time())
        with patch('continuous.service.yahoo_market_snapshot',return_value={'price':74,'change_pct':12}) as fetch:
            first=await (await self.client.get('/stock/PENG')).json()
            second=await (await self.client.get('/stock/PENG')).json()
            self.assertEqual(first,second)
            fetch.assert_called_once_with('PENG')
            self.assertEqual(first['rating']['classification'],'LATE')
            self.assertFalse(first['rating']['buyable'])

    async def test_corrected_bars_do_not_double_volume(self):
        t=time.time()-70
        b={'S':'PENG','t':utc(t),'o':64,'h':65,'l':63,'c':64,'v':100,'vw':64}
        self.s.consume_bar(b,seed=True)
        self.s.consume_bar(dict(b,v=200,vw=65),seed=True)
        totals=list(self.s.session_totals.values())[0]
        self.assertEqual(len(totals),1)
        self.assertEqual(list(totals.values())[0],(200,65))


if __name__=='__main__':unittest.main()
