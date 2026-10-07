# Continuous Market Radar

This extends the existing `market-radar-live` branch and PR #229. The old five-minute
GitHub Actions scanner remains available. This service is the VPS path: independent
async source listeners, Alpaca streaming minute bars and quotes, a persistent
correlator, authenticated push ingestion and a durable Telegram outbox. It places no orders.

## Start on an existing Linux VPS

Install Docker Engine with Compose using Docker's official instructions. Check out
the repository's `market-radar-live` branch, then:

```sh
cd market-radar
cp .env.example .env
chmod 600 .env
# Set Alpaca SIP market-data credentials, Telegram bot token/chat ID,
# ingestion token and an identifying User-Agent contact in .env.
docker compose up -d --build
docker compose logs --tail=100 radar
curl http://127.0.0.1:8787/health
```

`alive` means the service responds. `ready` requires recent tape plus configured
Telegram credentials; it does not prove delivery or complete source coverage.
Docker restarts the service after host reboot or process exit. An unhealthy check
alone does not restart a running container; use a VPS monitoring service to page
the owner on unhealthy status or adapter failures. During market closure tape age
is expected to rise; consult the exchange calendar when interpreting readiness.
Database state lives in the named volume. Never run two replicas on that database.
Back up with SQLite's backup API, not a live copy of its database file alone.

No server account, host target, Alpaca key, Telegram credentials or Docker daemon
was available during implementation. Cloud deployment and real phone delivery
remain unverified. This project does not purchase a VPS or data subscription.

The user requested Google sign-in only. Render's Google sign-in page is prepared;
`market-radar/render.yaml` defines a persistent Docker background worker with a 1GB
disk in Frankfurt. Import that Blueprint path after sign-in. Review the actual
recurring compute/disk price before creation; no paid service has been created.
Google sign-in does not provide Alpaca or Telegram credentials. The initial focused
universe suits this small worker; full-market operation requires capacity review.

Live feed probe on 2026-10-07: Google News feeds returned HTTP 200; Reddit feeds
returned 429 and are backed off. SEC returned an access error from this host.
The unverified Penguin RSS URL is disabled; a labelled Google News PENG fallback
is enabled. Direct Penguin IR ingestion therefore requires a working official feed
or approved push integration. These limitations prevent any claim that live
official-first PENG detection is currently operational.

## Scope and latency

The supplied focused universe includes the existing project's company aliases and
PENG. `RADAR_FULL_UNIVERSE=1` expands from SEC's issuer directory, which also
includes some non-common-share securities. For a curated tradable universe, supply
`RADAR_UNIVERSE_FILE` and mount it into the container. Full-market subscriptions
require matching provider entitlements, adequate memory and measured load testing.
Do not interpret the focused default as an all-market scan.

RSS/Atom and Google News run independently every 15–30 seconds, with conditional
requests, timeouts, retries and HTTP 429 backoff. SEC is one request per 15 seconds;
set a real contact in the User-Agent. Feed publication/indexing delays remain outside
our control. Feed claims are not proof that an event has not reached Reuters/Bloomberg.

Tape uses Alpaca WebSocket bars and quotes, never market-wide quote polling. RVOL
uses the median volume in the same New York minute from up to 20 prior sessions,
requiring at least five sessions. Today's data is excluded from the denominator.
Session VWAP is volume-weighted and resets on NY dates, using available premarket
and regular bars. Gap uses regular open vs preceding regular close. Relative strength
uses SPY at the same timestamp when available. Missing metrics remain unknown.
Minute confirmation may take over 60 seconds including bar finalization; an initial
news alert can arrive earlier as DEVELOPING. Sub-minute end-to-end performance is a
target, not a demonstrated guarantee. IEX is supported for development but its
single-exchange volume cannot stand in for consolidated SIP volume.

No baseline or current spread means no buyable classification. Long/negative text
matching, aliases, clustering and scores are heuristics. They are not investment
performance evidence. Complete dilution risk needs a separate issuer filing review;
the service marks missing review as unknown. Options remain unavailable unless a
separately licensed adapter is implemented.

## Radar Intelligence boundary

Reference reviewed at commit `2c6644884c3515d7316ce8023a799d529b14351c`:
https://github.com/Scognamiglio1969/radar-intelligence (AGPL-3.0).
No upstream source code is vendored into Aurora. This service reads its actual
Postgres `mentions(source,url,title,content,published_at,project_id)` schema through
a read-only DB role and `RADAR_INTELLIGENCE_DSN`/`RADAR_INTELLIGENCE_PROJECT_ID`.
An existing dedicated project must collect only approved public material. Do not
use an unrestricted mixed/private project. Its collector must run continuously;
reading every ten seconds does not speed up a daily upstream collector.

Allowlisted connector IDs cover Telegram, Reddit, Bluesky, Mastodon, Hacker News,
RSS, Google News, GDELT, GitHub, SEC and X. X still requires upstream credentials.
Discord is disabled until the operator approves specific public channels and bot
permissions. These connectors are **bridge capabilities**, not all running on this
installation. Telegram public channel collection is separate from outbound Bot API
alerts. No stolen dumps or private groups are included in the new configuration.

Edge Scanner reference: https://github.com/simonro/edge-scanner (MIT). We implement
the equivalent event-driven tape path directly against Alpaca's documented stream;
we do not claim to have integrated Edge Scanner's proprietary alert schema.

## Push an approved public event

Register the source in `continuous/sources.json` with `public:true`, `allow_push:true`,
its confidence and origin group. Identity and confidence come from server config,
not the inbound payload. Keep the listener bound to loopback, access it over SSH
or a TLS reverse proxy, and keep the bearer token private.

```sh
curl -X POST http://127.0.0.1:8787/events \
  -H "Authorization: Bearer $RADAR_INGEST_TOKEN" \
  -H 'Content-Type: application/json' \
  --data '{"source_id":"ri:rss","title":"$PENG raises outlook on AI demand","content":"Public release","url":"https://example.org/public-release","published_at":"2026-10-07T13:30:00Z"}'
```

Use the real source timestamp. An absent/naive/future timestamp is rejected.
Publication time is the earliest observed timestamp for a clustered monitored story,
not proof of the first appearance anywhere. Identical/similar syndicated headlines
share a story; origin groups prevent known copies counting as independent.
Configure publisher-specific origin groups where known. Generic social connectors
have conservative confidence and cannot produce HIGH-CONVICTION EARLY by themselves.

## Classifications and alerts

- DEVELOPING: initial source or unconfirmed tape anomaly; missing confirmation.
- HIGH-CONVICTION EARLY: credible public rumor, fresh source-price anchor, tape
  breakout above VWAP with RVOL >=2, spread <=1%, heuristic score >=70 and age <=10m.
- CONFIRMED: the same requirements with an official configured source.
- LATE: >=8% since the last pre-source completed bar, or >=5% above VWAP when
  source price exists. This is a conservative chase rule, not a universal optimum.
- AVOID: detected offering/dilution or negative catalyst for that issuer in the
  preceding day. Unknown filings are explicitly unknown rather than cleared.

Alerts contain timestamps, source and discovery prices, movement, RVOL and its
basis, acceleration, gap, VWAP, breakout, available RS/options, source confidence,
novelty, provenance, trigger/prior high, invalidation/VWAP, chase distance and
rule-based buyability. Confidence is an uncalibrated heuristic, not a probability.
There is one alert per state transition. Telegram errors and rate limits retry from
SQLite across restart. Delivery is at-least-once: a crash after Telegram accepts
but before the database commits can duplicate a message.

## Free Voroa background worker

Select Auroraaudio, branch `market-radar-live`, root `/market-radar`, start command
`python -m continuous.service`, and the free Nano worker. The root requirements.txt
installs the continuous service dependencies for native Python builds. One worker
fits the advertised 750 monthly hours; do not create additional free workers.
Skip optional billing setup. Configure Alpaca and Telegram secrets in the dashboard,
never in Git. Free IEX data is only one venue and is not consolidated market volume.
The Basic plan permits 30 streaming symbols. IEX mode prioritizes PENG and SPY then
the first configured issuers up to 30; /health exposes the selected symbols and
excluded count. News remains collected for all configured issuers. This free mode
does not confirm tape across the whole US market.
Without those credentials the process collects public news but cannot deliver
Telegram or confirm live tape. Verify storage persistence before relying on restart
deduplication; a container's temporary filesystem is not durable VPS storage.

## Validation

Local news matching now uses Nasdaq's public Nasdaq-listed and other-exchange
directories (`RADAR_NASDAQ_UNIVERSE=1`), cached and refreshed daily. ETFs and test
issues are excluded; ADRs and other listed non-ETF securities remain. OTC is absent.
Six broad catalyst queries complement the configured feeds. This does not cover
every news item or every social account. Whole-market tape is still unavailable
without appropriate data credentials. The simple Arabic dashboard shows the latest
alert per ticker and refuses buy presentation when tape or alert data is stale/missing.

Windows local mode: launch `start-local.ps1 -Python <venv Python path>` in a hidden
PowerShell process. The supervisor restarts crashes and persists SQLite in runtime/.
Open http://127.0.0.1:8787 for locally stored alerts. An optional ignored .env supplies
credentials; without them news works, tape confirmation and Telegram do not.
The installed Windows Startup shortcut runs after sign-in. Keep the PC awake and
online. To stop, create runtime/STOP and stop the child PID in runtime/service.pid;
remove STOP before starting again. Logs are timestamped in runtime/.

```sh
python -m pip install -r continuous/requirements.txt
python -m unittest continuous.test_core continuous.test_service -v
python -m unittest test_market_snapshot test_entry_gate test_tape_discovery test_missed_movers live.test_outcome_tracker live.test_end_to_end
python backtest/test_pit_backtester.py
python -m continuous.peng_acceptance
```

`peng_acceptance` distinguishes synthetic contract checks from historical acceptance.
Verified TickerLayer bars show October 7's first minute closing at $66 and the third
at $73.74. October 6's returned data ends at the regular close and omits the 20:05 UTC
release window. Full historical acceptance requires source timestamp proof, after-hours
bars, point-in-time volume baselines and historical bid/ask. It is currently INCOMPLETE.
Do not infer early detection, historical profitability or live operation from fixtures.
# Local classification and stock browser

The Arabic dashboard searches the full loaded symbol directory and paginates it.
Visible stocks retrieve public company headlines and Yahoo 5-minute snapshots with
two concurrent requests and a 60-second per-symbol cache; this is not a whole-market
streaming scanner. Snapshot volume ratio uses Yahoo's limited history and is displayed
separately from the streaming prior-session, same-minute RVOL.

The agreed classes are HIGH-CONVICTION EARLY, DEVELOPING, CONFIRMED, LATE and AVOID.
Core early-entry gates require a catalyst within 10 minutes, source confidence >=70,
fresh pre-source anchor and tape, RVOL >=2, price above VWAP and a 20-minute breakout,
known spread <=1%, and score >=70. CONFIRMED additionally requires an official source.
Risk overrides all positive evidence. LATE means >=8% since source or >=5% VWAP
extension with a known source anchor. The public snapshot display also flags daily
gains >=8% as a conservative chase warning, explicitly distinct from since-source move.

Score weights: source confidence x0.55, tape confirmation +20, acceleration >=2 +10,
two independent source groups +10, source age <=10 minutes +5, capped at 100.
These are uncalibrated rules, not success probabilities or evidence of a world-best
system. Missing options/relative strength/SEC review are disclosed rather than invented.
Snapshot data alone cannot authorize entry; streaming alerts older than 90 seconds
cannot authorize entry through the dashboard. The dashboard shows reasons and provenance.

The background public sweep now visits every directory symbol, resumes oldest/unattempted symbols after restart, and stores results in SQLite public_scan. With 7,487 symbols and a two-second minimum spacing, one cycle takes at least 250 minutes plus request time. It does not meet sub-minute whole-market price detection. The dashboard reports attempted, available, recently fetched snapshots and actual streaming symbols separately.

The default Opportunities tab selects recent catalyst headlines (24 hours, source confidence >=55), enriches up to 25 candidates, and excludes risk/late classifications, unavailable or older-than-10-minute quotes, negative daily returns and prices below VWAP. It reports evaluated and unexamined counts. A watch candidate is not an entry signal; only fresh streaming gates can authorize an entry.
