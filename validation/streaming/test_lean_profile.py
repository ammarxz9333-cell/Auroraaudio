#!/usr/bin/env python3
"""Reject any accidental duplicate external decoder, renderer or DSP backend."""
from __future__ import annotations

import copy
import json
from pathlib import Path
import unittest

from aurora_lean_profile import ProfileError, check_profile, load_profile

ROOT = Path(__file__).resolve().parents[2]
PROFILE = load_profile(ROOT / "config/aurora-lean-audio-profile-v1.json")


class LeanProfileTests(unittest.TestCase):
    def reject(self, change):
        mutated = copy.deepcopy(PROFILE)
        change(mutated)
        with self.assertRaises(ProfileError):
            check_profile(mutated)

    def test_no_shipping_of_all_research_projects(self):
        record = check_profile(PROFILE)
        self.assertEqual(record["distinct_upstream_runtime_packages"], 4)
        self.assertEqual(record["runtime_groups"], 3)
        self.assertGreaterEqual(record["research_only_projects"], 12)
        self.assertEqual(record["single_owner_formats"], 21)
        self.assertEqual(record["fastest_backend_claim"], "UNMEASURED")
        self.assertFalse(record["physical_audio_verified"])

    def test_no_same_decoder_twice(self):
        self.reject(lambda x: x["runtime_groups"][2]["packages"].append(
            copy.deepcopy(x["runtime_groups"][0]["packages"][0])
        ))

    def test_no_research_oracle_allowed_on_live_path(self):
        def mutate(x):
            x["runtime_groups"][0]["packages"][0]["id"] = "openjoc"
        self.reject(mutate)

    def test_immersive_spatial_renderer_cannot_be_separately_substituted(self):
        self.reject(lambda x: x["runtime_groups"][1]["packages"].pop())
        self.reject(lambda x: x["runtime_groups"][1].update(abi_pair="bridge_api-0.6"))

    def test_reject_duplicated_ordinary_codec_assignment(self):
        self.reject(lambda x: x["source_format_owners"].update(ac3="harletty-omniphony-objects"))
        self.reject(lambda x: x["source_format_owners"].update(eac3=["ffmpeg-channel-bed", "openjoc"]))
        self.reject(lambda x: x["source_format_owners"].pop("mpeg-h"))

    def test_cannot_enable_unproven_ac4_or_iamf_runtime(self):
        self.reject(lambda x: x["conditional_format_specialists"][0].update(mode="ENABLED"))
        self.reject(lambda x: x["conditional_format_specialists"][2].update(mode="PRODUCTION"))

    def test_channel_order_never_changes_with_backend_upgrade(self):
        self.reject(lambda x: x["canonical_output"]["roles"].reverse())
        self.reject(lambda x: x["canonical_output"].update(channels=2))

    def test_latest_candidate_does_not_autoinstall(self):
        self.reject(lambda x: x["policy"].update(automatic_upgrade=True))
        self.reject(lambda x: x["policy"].update(
            benchmark_decisions_require_measured_same_host=False
        ))
        self.reject(lambda x: x["policy"].update(max_third_party_runtime_groups=20))

    def test_no_duplicate_specialist(self):
        self.reject(lambda x: x["conditional_format_specialists"].append(
            copy.deepcopy(x["conditional_format_specialists"][0])
        ))


if __name__ == "__main__":
    unittest.main()
