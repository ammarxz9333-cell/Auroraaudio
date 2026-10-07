# Market Radar

For the persistent VPS service, Docker Compose, streaming price/volume correlation,
Telegram outbox and explicit PENG acceptance evidence, see [continuous/README.md](continuous/README.md).

A free first-public-source market monitor that runs every 5 minutes on GitHub Actions and opens a GitHub Issue when a new public item scores as potentially market-moving.

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

## How alerts work

Every new item gets a score from:

1. source quality / originality,
2. catalyst language (M&A, FDA, contracts, guidance, financing, dilution, etc.),
3. whether a tracked ticker/company is mentioned,
4. early-information language such as exclusive, people familiar, scoop, leak,
5. noise penalties.

Default alert threshold is **8**. A qualifying event opens an Issue and assigns it to the repository owner, which makes GitHub the zero-cost notification channel.

The first run bootstraps existing items and only alerts items published within the previous 8 hours, preventing a flood of old stories.

## Optional Telegram alerts

The system already works without Telegram. For immediate phone alerts through Telegram, add repository Actions secrets:

- TELEGRAM_BOT_TOKEN
- TELEGRAM_CHAT_ID

If the secrets are absent, Telegram is simply skipped.

## Files

- radar.py — fetch, parse, score, deduplicate and alert.
- sources.json — monitored sources and ticker/company aliases.
- state.json — hashes of seen public items, updated by the workflow.
- .github/workflows/market-radar.yml — five-minute scheduler.

## Important operational detail

GitHub documents a minimum scheduled-workflow interval of 5 minutes. Scheduled workflows can occasionally be delayed during heavy platform load, so this is a fast free monitor, not a guaranteed low-latency trading feed.
