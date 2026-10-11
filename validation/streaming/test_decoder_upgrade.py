#!/usr/bin/env python3
"""Upgrade plan regression tests: no unsafe package swaps or false success."""
from __future__ import annotations

import copy
from pathlib import Path
import unittest

from aurora_decoder_upgrade import (
    BundleError, DecoderUpgrade, GateEvidence,
    REQUIRED_GATES, UpgradeState, load_catalog, validate_catalog,
)

ROOT = Path(__file__).resolve().parents[2]
ORIGINAL = load_catalog(ROOT / "config/aurora-codec-bundles-v1.json")


def all_test_evidence() -> GateEvidence:
    """Synthetic input solely to exercise promotion policy; NOT physical proof."""
    return GateEvidence(REQUIRED_GATES, True, True, True)


class PackageUpgradeTests(unittest.TestCase):
    def test_manifest_pins_known_pairs_and_immutable_shas(self):
        validate_catalog(ORIGINAL)
        self.assertEqual(len(ORIGINAL["bundles"]), 2)
        self.assertEqual(ORIGINAL["canonical_output"]["channels"], 12)

    def test_rejects_harletty_new_omni_old_abi(self):
        candidate = copy.deepcopy(ORIGINAL)
        target = candidate["bundles"][1]
        target["components"]["omniphony"] = copy.deepcopy(
            candidate["bundles"][0]["components"]["omniphony"]
        )
        with self.assertRaises(BundleError):
            validate_catalog(candidate)

    def test_rejects_channel_order_and_unpinned_upstream(self):
        changed = copy.deepcopy(ORIGINAL)
        changed["canonical_output"]["channel_roles"][4] = "SBR"
        with self.assertRaises(BundleError):
            validate_catalog(changed)
        changed = copy.deepcopy(ORIGINAL)
        changed["bundles"][0]["components"]["omniphony"]["commit"] = "main"
        with self.assertRaises(BundleError):
            validate_catalog(changed)

    def test_cannot_activate_unverified_candidate_even_with_claimed_gate_pass(self):
        upgrade = DecoderUpgrade(ORIGINAL, "joc-current-pin")
        upgrade.stage("joc-spatial-v0.8-candidate")
        self.assertFalse(upgrade.muted)
        self.assertEqual(upgrade.active["id"], "joc-current-pin")
        with self.assertRaises(BundleError):
            upgrade.begin_muted_swap(all_test_evidence())
        self.assertEqual(upgrade.active["id"], "joc-current-pin")
        self.assertFalse(upgrade.muted)

    def test_full_gate_required_and_rollback_fails_closed(self):
        catalog = copy.deepcopy(ORIGINAL)
        # Synthetic configuration to exercise a *future* externally accepted
        # bundle; actual production candidate in repo stays unverified.
        candidate = catalog["bundles"][1]
        candidate["physically_verified"] = True
        candidate["state"] = "existing-software-fixture-validated"
        slot = DecoderUpgrade(catalog, "joc-current-pin")
        slot.stage("joc-spatial-v0.8-candidate")
        with self.assertRaises(BundleError):
            slot.begin_muted_swap(GateEvidence(
                REQUIRED_GATES - {"physical-output"}, False, True, True
            ))
        self.assertFalse(slot.muted)
        epoch = slot.begin_muted_swap(all_test_evidence())
        self.assertTrue(slot.muted)
        self.assertEqual(slot.state, UpgradeState.MUTED_PROBATION)
        self.assertEqual(slot.active["id"], "joc-current-pin")
        # A crashed/new decoder cannot unmute, and the previous pinned
        # version remains installed and targeted for recovery.
        self.assertFalse(slot.confirm_new_worker(
            epoch, decoder_healthy=False,
            output_contract_valid=True, no_xruns=True,
        ))
        self.assertEqual(slot.state, UpgradeState.ROLLED_BACK)
        self.assertEqual(slot.active["id"], "joc-current-pin")
        self.assertTrue(slot.muted)
        self.assertFalse(slot.previous_worker_recovered(
            decoder_healthy=False, output_contract_valid=False
        ))
        self.assertTrue(slot.previous_worker_recovered(
            decoder_healthy=True, output_contract_valid=True
        ))
        self.assertFalse(slot.muted)

    def test_promote_and_guard_stale_generation(self):
        catalog = copy.deepcopy(ORIGINAL)
        catalog["bundles"][1]["physically_verified"] = True
        catalog["bundles"][1]["state"] = "existing-software-fixture-validated"
        slot = DecoderUpgrade(catalog, "joc-current-pin")
        slot.stage("joc-spatial-v0.8-candidate")
        epoch = slot.begin_muted_swap(all_test_evidence())
        self.assertFalse(slot.confirm_new_worker(
            epoch + 1, decoder_healthy=True,
            output_contract_valid=True, no_xruns=True,
        ))
        self.assertTrue(slot.muted)
        self.assertEqual(slot.active["id"], "joc-current-pin")
        slot.previous_worker_recovered(
            decoder_healthy=True, output_contract_valid=True
        )
        slot.stage("joc-spatial-v0.8-candidate")
        next_epoch = slot.begin_muted_swap(all_test_evidence())
        self.assertTrue(slot.confirm_new_worker(
            next_epoch, decoder_healthy=True,
            output_contract_valid=True, no_xruns=True,
        ))
        self.assertEqual(slot.active["id"], "joc-spatial-v0.8-candidate")
        self.assertEqual(slot.last_good["id"], "joc-current-pin")
        self.assertEqual(slot.state, UpgradeState.COMMITTED)


if __name__ == "__main__":
    unittest.main()
