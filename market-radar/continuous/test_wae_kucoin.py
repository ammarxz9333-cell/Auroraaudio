"""KuCoin read-only WAE scanner tests, no network or trades."""
import asyncio
import unittest

from continuous.test_wae_early import history
from continuous.wae_kucoin import choose_contracts, scan


class Response:
    def __init__(self, payload):
        self.payload = payload

    async def __aenter__(self):
        return self

    async def __aexit__(self, *args):
        return False

    def raise_for_status(self):
        pass

    async def json(self):
        return self.payload


class FakeKuCoin:
    def __init__(self, five, hour, invalid=False):
        self.five, self.hour, self.invalid = five, hour, invalid
        self.requested = []

    def get(self, url, params=None):
        self.requested.append((url,params))
        if 'contracts/active' in url:
            return Response({'code':'200000','data':[
                {'symbol':'XBTUSDTM','turnoverOf24h':'9000000','isOpen':True},
                {'symbol':'DELISTEDUSDTM','turnoverOf24h':'8000000','isOpen':False}
            ]})
        data = self.five if params['interval']=='5min' else self.hour
        if self.invalid:
            return Response({'code':'200000','data':{'list':None}})
        rows = [[x['time'],x['open'],x['high'],x['low'],x['close'],x['volume'],1]
                for x in data]
        return Response({'code':'200000','data':{'list':rows}})


class ScannerTest(unittest.TestCase):
    def test_choose_only_liquid_open_contracts(self):
        rows=[{'symbol':'XBTUSDTM','turnoverOf24h':'1000000','isOpen':True},
              {'symbol':'LOWUSDTM','turnoverOf24h':'200','isOpen':True},
              {'symbol':'HALTUSDTM','turnoverOf24h':'2000000','isOpen':False}]
        self.assertEqual(choose_contracts(rows),['XBTUSDTM'])

    def test_both_call_put_scanning(self):
        for direction in (-1,1):
            five,hour,now=history(direction)
            api=FakeKuCoin(five,hour)
            out=asyncio.run(scan(api,asof=now))
            self.assertEqual(out['status'],'COMPLETE')
            self.assertEqual(out['checked'],1)
            self.assertEqual(out['requested'],1)
            self.assertFalse(out['automated_orders'])
            self.assertEqual(len(api.requested),3)
            self.assertIn('fresh_calls',out)
            self.assertIn('fresh_puts',out)

    def test_failed_provider_cannot_create_fake_signal(self):
        five,hour,now=history(1)
        out=asyncio.run(scan(FakeKuCoin(five,hour,invalid=True),asof=now))
        self.assertEqual(out['status'],'PARTIAL')
        self.assertEqual(out['fresh_calls'],[])
        self.assertEqual(out['fresh_puts'],[])
        self.assertEqual(len(out['errors']),1)


if __name__=='__main__':
    unittest.main()
