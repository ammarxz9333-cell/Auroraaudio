# Market Radar

A free first-public-source market monitor that runs every 5 minutes on GitHub Actions, scores potentially market-moving information, and creates persistent artifacts for human/LLM review.

## What it watches

- SEC: 8-K, S-3, 424B5, Schedule 13D/13G and Form 4 current filings.
- FDA public regulatory/news signals.
- U.S. Department of Defense contract releases.
- Government/procurement and power-infrastructure public sources.
- ICIJ, OCCRP, public-index/reporting around WikiLeaks and DDoSecrets.
- Reuters M&A, Bloomberg Deals/Scoops, The Information, FT Alphaville/M&A, Semafor Business.
- Breaking Defense, Utility Dive, Data Center Dynamics, Muddy Waters.
- Evan Blass, OnLeaks, Ice Universe, Ming-Chi Kuo, Digital Chat Station.
- Tom Henderson / Insider Gaming and Jason Schreier.
- Reddit WSB, stocks, ShortSqueeze, Biotechplays and pennystocks.

The scanner uses only lawfully public/free material. It does not authenticate into private systems or download underlying hacked/stolen dumps.

## Alert pipeline

Every new item gets a score from:

1. source quality / originality,
2. catalyst language (M&A, FDA, contracts, guidance, financing, dilution, etc.),
3. ticker/company identification,
4. early-information language such as exclusive, people familiar, scoop or leak,
5. market reaction and same-time volume when a ticker is known,
6. noise penalties.

Ticker identification is not limited to the static watchlist. The scanner also extracts explicit symbols such as `$CRWV`, `NASDAQ: RKLB`, `NYSE: XYZ`, etc.

Default alert threshold is **8** for identified tickers. Unidentified/social candidates require a higher threshold to reduce noise.

For every qualifying alert the scanner can:

- commit an append-only JSON artifact to `market-radar-live:market-radar/live/alerts/`,
- create a GitHub Issue with source/timestamp/score/market context,
- optionally send Telegram,
- update PR **#229 Market Radar Live Alerts**, whose commit activity is used by the ChatGPT Work live-review trigger.

The first run bootstraps existing items and only alerts items published within the previous 8 hours, preventing a flood of old stories.
Subsequent runs apply the same freshness bound. Sources without a usable publication time are not sent as time-sensitive alerts.

The append-only PR artifact uses the stable radar ID as its filename. A retry cannot create a second artifact for the same story; the Issue notification can be retried if its creation failed. Artifacts are unreviewed candidates, not entry signals.

## Market-reaction context

When a ticker is known, the scanner samples free Yahoo Finance 5-minute bars and records:

- current price and previous close,
- day % change,
- latest 5-minute move,
- cumulative volume relative to recent same-time sessions,
- 30-minute move before the event,
- first 30-minute move after the event,
- move since the event,
- session classification and reaction stage.

Stages include `EARLY`, `RADAR`, `REACTING` and `MAJOR-REPRICE`.

## Learning loop

`market-radar/evaluate.py` reads real alert artifacts from the persistent live branch and evaluates them without changing the original signal.

The daily learning workflow records:

- +5m, +30m, +1h, +24h and +48h returns,
- first and second regular-session closes,
- MFE and MAE over 24h and 48h,
- +5% follow-through within 24h,
- +10% follow-through within 48h,
- adverse -5% excursion within 24h,
- calibration by score band, stage, source class and source.

Outputs:

- `market-radar/learning/outcomes.json`
- `market-radar/learning/metrics.json`

The learning job runs after the U.S. session on weekdays and can also be dispatched manually. Metrics are descriptive; the system must accumulate adequate out-of-sample observations before score/threshold changes are justified.

The scheduled scanner performs syntax and functional checks before fetching sources. Historical replay runs on code pushes and manual dispatch, not on every five-minute scheduled scan. The live gate requires a current market bar and a real bid/ask quote; absent data results in WAIT or INSUFFICIENT_DATA.

## Optional Telegram alerts

The system works without Telegram. For phone alerts through Telegram, add repository Actions secrets:

- `TELEGRAM_BOT_TOKEN`
- `TELEGRAM_CHAT_ID`

If the secrets are absent, Telegram is skipped.

## Files

- `radar.py` — fetch, parse, identify, score, deduplicate, enrich and alert.
- `evaluate.py` — outcome measurement and calibration.
- `sources.json` — monitored sources and tracked ticker/company aliases.
- `state.json` — hashes of seen public items and scanner health.
- `learning/outcomes.json` — point-in-time signal outcomes.
- `learning/metrics.json` — aggregate calibration/performance metrics.
- `.github/workflows/market-radar.yml` — five-minute scanner plus syntax validation.
- `.github/workflows/market-radar-learning.yml` — daily learning loop.
- persistent branch `market-radar-live` / PR #229 — append-only live alert feed.

## Research discipline

The objective is high precision, not maximum alert count. No trade is a valid outcome when the signal is weak, late or already repriced.

Do not claim a fixed success rate until it is measured on a sufficiently large out-of-sample sample. Model changes should be walk-forward tested, documented and reversible; avoid future leakage and retrospective cherry-picking.

## Operational detail

GitHub documents a minimum scheduled-workflow interval of 5 minutes. Scheduled runs can occasionally be delayed during platform load, so this is a fast free monitor, not a guaranteed low-latency market-data feed.
