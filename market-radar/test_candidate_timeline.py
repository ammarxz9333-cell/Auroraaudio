import datetime as dt
import json
import tempfile
from pathlib import Path
from unittest.mock import patch

import missed_movers
import radar


def test_below_threshold_candidate_is_frozen_once_without_future_outcome():
    published = dt.datetime(2026, 9, 28, 12, tzinfo=dt.timezone.utc)
    first_seen = published + dt.timedelta(minutes=3)
    with tempfile.TemporaryDirectory() as tmp, patch.object(radar, "CANDIDATE_DIR", Path(tmp)):
        item = {"title": "ABC explores options", "url": "https://example.org/abc",
                "published": published}
        p = radar.freeze_scanned_candidate(
            {"name": "Example", "class": "industry"}, item, 5, ["ABC"],
            ["review"], "stable-id", 8, first_seen)
        first = p.read_bytes()
        radar.freeze_scanned_candidate(
            {"name": "Example", "class": "industry"}, item, 12, ["ABC"],
            ["later"], "stable-id", 8, first_seen + dt.timedelta(hours=1))
        row = json.loads(p.read_text())
        assert p.read_bytes() == first
        assert row["score_at_capture"] == 5
        assert row["alert_eligible_at_capture"] is False
        assert row["captured_utc"] == first_seen.isoformat()
        assert row["outcome"] is None


def test_candidate_rejects_publication_after_capture():
    captured = dt.datetime(2026, 9, 28, 12, tzinfo=dt.timezone.utc)
    with tempfile.TemporaryDirectory() as tmp, patch.object(radar, "CANDIDATE_DIR", Path(tmp)):
        try:
            radar.freeze_scanned_candidate({"name": "Source"},
                {"published": captured + dt.timedelta(minutes=1)}, 8, ["ABC"],
                [], "future", 8, captured)
        except ValueError:
            pass
        else:
            raise AssertionError("future-dated publication was accepted")
        assert not list(Path(tmp).iterdir())


def test_same_day_alert_overlap_is_not_claimed_as_advance_recall():
    rows = [
        {"alerted_within_lookback": True, "watchlist_member": True},
        {"alerted_within_lookback": False, "watchlist_member": True},
    ]
    summary = missed_movers.summarize_overlap(rows)
    assert summary["recent_alert_overlap"] == 1
    assert summary["no_recent_alert"] == 1
    assert summary["advance_warning_recall_pct"] is None
    assert "recall_pct" not in summary
