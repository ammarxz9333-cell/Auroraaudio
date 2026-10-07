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

    async def test_corrected_bars_do_not_double_volume(self):
        t=time.time()-70
        b={'S':'PENG','t':utc(t),'o':64,'h':65,'l':63,'c':64,'v':100,'vw':64}
        self.s.consume_bar(b,seed=True)
        self.s.consume_bar(dict(b,v=200,vw=65),seed=True)
        totals=list(self.s.session_totals.values())[0]
        self.assertEqual(len(totals),1)
        self.assertEqual(list(totals.values())[0],(200,65))


if __name__=='__main__':unittest.main()
