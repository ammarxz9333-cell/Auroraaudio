"""Deterministic tests: no network, no incomplete bars, no false SHORT trade."""
import asyncio
import unittest

from continuous.crypto_kucoin import (
    HOUR, classify_hourly, select_liquid_usdt, scan_kucoin,
)


def series(up=True, length=300, extra_forming=False):
    out = []
    for i in range(length + int(extra_forming)):
        p = (100 + i * .4) if up else (200 - i * .25)
        v = 250 if i == length - 1 else 100
        out.append([str(i * HOUR), str(p), str(p), str(p * 1.001),
                    str(p * .999), str(v), "100000"])
    return out


def clock(length=300):
    return length * HOUR + 30


class CryptoLayerTest(unittest.TestCase):
    def test_long_watch_and_heuristic_not_probability(self):
        x = classify_hourly("BTC-USDT", series(), clock())
        self.assertEqual(x["status"], "LONG_WATCH")
        self.assertEqual(x["score"], 100)
        self.assertFalse(x["actionable"])
        self.assertFalse(x["automated_orders"])
        self.assertFalse(x["is_calibrated_probability"])
        self.assertEqual(x["instrument"], "spot_reference_only")

    def test_short_watch_is_not_executable_futures_order(self):
        x = classify_hourly("ETH-USDT", series(up=False), clock())
        self.assertEqual(x["status"], "SHORT_WATCH")
        self.assertFalse(x["actionable"])
        self.assertEqual(x["instrument"], "spot_reference_only")

    def test_incomplete_forming_bar_excluded(self):
        candles = series(extra_forming=True)
        candles[-1][1:5] = ["100000", "100000", "100000", "100000"]
        x = classify_hourly("BTC-USDT", candles, clock())
        self.assertEqual(x["status"], "LONG_WATCH")
        self.assertLess(x["close"], 1000)

    def test_missing_bar_fail_closed(self):
        candles = series(length=301)
        candles.pop(100)
        x = classify_hourly("BTC-USDT", candles, 301 * HOUR + 30)
        self.assertEqual(x["reason"], "missing_hourly_candles")

    def test_stale_or_inadequate_history_fail_closed(self):
        self.assertEqual(classify_hourly("BTC-USDT", series(), 304 * HOUR)["reason"],
                         "stale_hourly_candles")
        self.assertEqual(classify_hourly("BTC-USDT", series(length=299), clock())["reason"],
                         "insufficient_completed_1h_history")

    def test_duplicate_even_in_forming_bar_is_rejected(self):
        candles = series()
        candles.append(candles[-1])
        self.assertEqual(classify_hourly("BTC-USDT", candles, clock())["status"],
                         "UNAVAILABLE")

    def test_unverified_volume_rejected(self):
        candles = series()
        candles[-4][5] = "0"
        self.assertEqual(classify_hourly("BTC-USDT", candles, clock())["reason"],
                         "missing_volume")

    def test_selects_only_liquid_spread_qualified_spot(self):
        rows = [
            {"symbol": "BTC-USDT", "volValue": "4000000", "buy": "100", "sell": "100.1"},
            {"symbol": "ETH-USDT", "volValue": "2500000", "buy": "80", "sell": "80.1"},
            {"symbol": "LOW-USDT", "volValue": "100", "buy": "10", "sell": "10.1"},
            {"symbol": "USDC-USDT", "volValue": "9000000", "buy": "1", "sell": "1.0001"},
            {"symbol": "BTC3L-USDT", "volValue": "7000000", "buy": "2", "sell": "2.01"},
            {"symbol": "BAD-USDT", "volValue": "8000000", "buy": "1", "sell": "2"},
        ]
        symbols = [x["symbol"] for x in select_liquid_usdt(rows)]
        self.assertEqual(symbols, ["BTC-USDT", "ETH-USDT"])


class FakeResponse:
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


class FakeSession:
    def get(self, url, params=None):
        if url.endswith("allTickers"):
            return FakeResponse({"code": "200000", "data": {
                "ticker": [{"symbol": "BTC-USDT", "volValue": "2000000",
                            "buy": "100", "sell": "100.1"}]}})
        return FakeResponse({"code": "200000", "data": series()})


class ScanTest(unittest.TestCase):
    def test_public_scan_reports_watch_not_trade(self):
        x = asyncio.run(scan_kucoin(FakeSession(), clock(), top_n=20))
        self.assertEqual(x["status"], "COMPLETE")
        self.assertEqual(x["requested"], 1)
        self.assertEqual(x["scanned"], 1)
        self.assertEqual(x["watch"][0]["status"], "LONG_WATCH")
        self.assertFalse(x["automated_orders"])


if __name__ == "__main__":
    unittest.main()
