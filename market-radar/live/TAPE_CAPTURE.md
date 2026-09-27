# Live tape capture

Historical daily OHLC cannot prove the first executable Entry Gate timestamp.
Market Radar must persist every evaluated 1m/5m bar before calculating or
emitting BUYABLE_NOW.

Required fields: timestamp, price, volume, VWAP, same-time RVOL, regular open,
session high/low-to-date and spread. Store the exact gate input and gate output
with the alert. This makes +5%-before--5% validation reproducible without
future-data leakage.

Retention: keep raw bars for every WATCH candidate for at least 30 trading
days, including candidates that never become BUYABLE_NOW.
