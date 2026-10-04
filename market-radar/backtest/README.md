# Market Radar historical research protocol

## Two distinct evidence lanes

1. **Timestamped event replay:** each alert is frozen at `captured_utc`. A filing or article may be used only after it was actually available to the scanner. The five-minute gate reads a completed bar and assumes execution at the **next** bar's open, with explicit spread and slippage scenarios. It never earns a return from the signal bar's own high or low.
2. **Broad daily price/volume benchmark:** a deterministic sample of up to 180 current US listings (100 Nasdaq, 60 NYSE, 20 NYSE American), selected without examining returns. Every liquid symbol-day is a control observation, including flat and losing days. The fixed rule uses only daily bars through the signal close and fills at the following regular session open. It is not a historical news or catalyst backtest.

The broad workflow publishes `universe.json` with the official Nasdaq Trader directory hash and fetch time, a machine-readable report, and a Markdown summary. It uses a 60/20/20 chronological development/validation/final split with seven calendar days excluded around both boundaries. The rule and thresholds are fixed before reading validation and final outcomes. Up to two alerts per week are chosen at each day's close; the program never ranks Monday's signals using Thursday's prices.

The target is whether +5% or -5% is touched first within five subsequent daily bars. If both are touched in one daily bar, the result is `ORDER_UNVERIFIED`, never a win. It reports five-day return, favorable/adverse excursion, source coverage and a 95% Wilson interval. Missed-winner blockers describe **why the rule skipped a later winner**, not why the company rose. False-positive cases are reported separately.

## Limits that must stay visible

- Nasdaq Trader's public directory describes **current** listings. Historical delisted firms are missing; the two-year sample has survivorship bias. [Nasdaq directory definitions](https://www.nasdaqtrader.com/trader.aspx?id=symboldirdefs).
- Yahoo historical OHLCV may be revised or split adjusted. Neither historical bid/ask nor intraday ordering within a daily candle is known. Spread and slippage are assumptions, not measured fills.
- Company news, analyst actions, rumors, SEC acceptance timestamps and sector links are **not** in the broad price-only benchmark. It cannot establish a causal reason for a move or validate the news scanner's recall.
- The archived event lane is currently small. Increasing it requires collecting candidates and non-candidates forward in time; retroactively selecting winners would bias the sample.
- Threshold changes must be proposed from development data, checked once on validation and then evaluated on untouched final data. A small or weak final cohort must be reported as inconclusive. The live gate is never changed just because a historical pilot looks favorable.

The SEC provides public submissions history and filing data for a future timestamped catalyst join: [EDGAR APIs](https://www.sec.gov/search-filings/edgar-application-programming-interfaces). Availability must use dissemination/acceptance timing, not just a filing date.

## Prospective catalyst corpus

The live scanner now saves a first-seen, immutable JSON file in `live/candidates/` for each fresh ticker-matched headline, including items below the alert threshold. It records the publication time claimed by the feed, actual scan/capture time, URL, source, ticker, score, threshold and matched vocabulary; later returns never overwrite it. A news item without a usable publication timestamp remains ineligible. These files are the denominator for future news-scanner precision and missed-opportunity analysis. They do not prove the headline caused a move, and collection begins only after deployment.

The Yahoo day-gainers audit has no timestamp for when a stock first crossed its gain threshold. It therefore reports recent alert **overlap**, not advance-warning recall. A genuine recall estimate needs all eligible stocks plus timestamped first-crossing events and the frozen candidates above.
The audit runs only after 16:15 New York time on a weekday and accepts quotes stamped for that same New York market date. A premature code-push run removes a stale same-day audit rather than recording a previous session as today's movers.
