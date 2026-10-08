# Always-on Market Radar

This is the deployment layer for moving the radar off GitHub Actions' five-minute schedule.

## Architecture

Original public catalyst sources -> existing radar.py scoring/entity extraction -> HOT candidates -> tape checks -> Telegram/GitHub alerts.

The daemon repeats the existing scanner every 20 seconds by default. It deliberately enforces a 10-second floor so public endpoints are not hammered. Existing per-domain SEC/Reddit rate limits still apply.

Important: this removes scheduler latency, not data-feed latency. Yahoo/public endpoints are not a substitute for an exchange-grade tick feed. A later adapter can consume a licensed/free WebSocket feed without changing the catalyst layer.

## Linux deployment

Clone to /opt/Auroraaudio, create a low-privilege market-radar user, copy market-radar.service to /etc/systemd/system/, and place secrets/config in /etc/market-radar.env. Then enable the service with systemd.

Required/optional environment variables are the same as radar.py, including TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID when Telegram alerts are desired.

## Promotion gate

Do not call this production-ready merely because it runs faster. Before increasing capital exposure, record point-in-time alert price, source timestamp, MFE/MAE at 5m/30m/1d, duplicates, direction errors, and false positives. Promote only after live/out-of-sample expectancy is positive and pre-3% recall improves.
