import datetime as dt
import json
import urllib.error
from unittest.mock import patch

import radar


def test_alert_artifact_is_stable_and_retries_are_idempotent():
    calls = []
    item = {"title": "Acme merger", "url": "https://example.org/news",
            "published": dt.datetime(2026, 9, 27, tzinfo=dt.timezone.utc)}
    def api(path, method="GET", payload=None):
        calls.append((path, method, payload))
        if method == "PUT" and len(calls) == 2:
            raise urllib.error.HTTPError(path, 422, "already exists", {}, None)
        if method == "GET":
            return {"path": path.split("?", 1)[0].removeprefix(f"/repos/{radar.REPO}/contents/")}
    with patch.object(radar, "github_api", side_effect=api):
        with patch.object(radar, "now_utc", return_value=dt.datetime(2026, 9, 27, 21, 0, tzinfo=dt.timezone.utc)):
            for _ in range(2):
                path = radar.publish_live_alert({"name": "Official", "class": "primary"},
                    item, 10, ["ABC"], ["merger"], "fixed-id", [], {})
    assert path.endswith("/fixed-id.json")
    assert calls[0][2]["branch"] == "market-radar-live"
    assert json.loads(__import__("base64").b64decode(calls[0][2]["content"]))["review_status"].startswith("UNREVIEWED")


def test_entry_gate_rejects_stale_regular_bar_without_asking_quote_service():
    snapshot = {"ticker": "ABC", "market_session": "REGULAR",
                "bar_time_utc": "2026-09-25T20:00:00+00:00"}
    with patch.object(radar, "now_utc", return_value=dt.datetime(2026, 9, 27, 21, 0, tzinfo=dt.timezone.utc)):
        with patch.object(radar, "yahoo_quote_bid_ask") as quote:
            result = radar.evaluate_entry_gate(snapshot, 10, 8)
    assert result["state"] == "WAIT"
    quote.assert_not_called()


def test_alert_publish_does_not_hide_unrelated_validation_error():
    item = {"title": "Acme merger", "url": "https://example.org/news", "published": None}
    def api(path, method="GET", payload=None):
        if method == "PUT":
            raise urllib.error.HTTPError(path, 422, "invalid branch", {}, None)
        return {}
    with patch.object(radar, "github_api", side_effect=api):
        try:
            radar.publish_live_alert({"name": "Official"}, item, 10, ["ABC"], [], "rid", [], {})
        except urllib.error.HTTPError:
            pass
        else:
            raise AssertionError("unrelated validation error was hidden")


def test_unlisted_explicit_symbol_and_institutional_holding_noise():
    assert radar.match_watchlist("NASDAQ: MXL and $VIAV", {}) == ["MXL", "VIAV"]
    item = {"title": "232,020 Shares of BTGO $BTGO Acquired by State Street Corp", "snippet": ""}
    score, tickers, _ = radar.score_item({"weight": 2, "class": "industry"}, item, {})
    assert tickers == ["BTGO"]
    assert score < radar.THRESHOLD


def test_buyable_entry_wakes_pr_reviewer():
    with patch.object(radar, "append_live_artifact") as publish:
        radar.publish_live_entry("story", "ABC", {"state": "BUYABLE_NOW"},
            {"bar_time_utc": "2026-09-28T14:00:00+00:00"}, {"source": "Official", "score": 12})
    path, artifact = publish.call_args.args
    assert path.endswith("/story-ABC-entry.json")
    assert artifact["type"] == "ENTRY_CANDIDATE"
    assert artifact["gate"]["state"] == "BUYABLE_NOW"


def test_old_market_bar_cannot_label_fresh_news_as_reacting():
    old = {"ticker": "ABC", "reaction": "major-reprice", "bar_time_utc": "2026-09-25T20:00:00+00:00"}
    item = {"title": "Acme news", "published": dt.datetime(2026, 9, 27, 21, 0, tzinfo=dt.timezone.utc)}
    with patch.object(radar, "now_utc", return_value=dt.datetime(2026, 9, 27, 21, 5, tzinfo=dt.timezone.utc)):
        with patch.object(radar, "github_api", return_value={"html_url": "https://example.org/issue"}) as api:
            radar.create_issue({"name": "Official"}, item, 10, ["ABC"], [], "rid", [old])
    assert "[MARKET-RADAR" in api.call_args.kwargs["payload"]["title"]
