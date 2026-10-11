#!/usr/bin/env python3
"""Negative hardware compatibility and independent replacement tests."""
from __future__ import annotations

from copy import deepcopy
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from aurora_swappable_hardware import (  # noqa: E402
    HardwareProfileError, MANIFEST, ROLES, SLOTS, assess, load_catalog,
)


class SwappableBudgetTests(unittest.TestCase):
    def setUp(self):
        self.catalog = load_catalog()

    def test_default_is_modular_not_falsely_physically_ready(self):
        result = assess(self.catalog)
        self.assertTrue(result.compatible_design)
        self.assertFalse(result.physical_ready)
        self.assertEqual(result.choices["compute"], "pi5-4gb")
        self.assertEqual(result.choices["ingress"], "lindy-sii9437")
        self.assertEqual(result.choices["output"], "uac2-12ch-single-device")
        self.assertIn("output", result.missing_cost_slots)
        self.assertIn("amplifier", result.missing_cost_slots)
        self.assertIn("wireless", result.missing_cost_slots)
        self.assertGreaterEqual(result.known_incremental_minimum_eur, 120.60)
        self.assertEqual(len(ROLES), 12)

    def test_each_replaceable_slot_swaps_without_changing_other_choices(self):
        substitutes = {
            "compute": "generic-linux-arm64",
            "ingress": "open-file-fixture",
            "output": "xmos-lib-xua-16ch-prototype",
            "wireless": "esp-avb-ptp-prototype",
        }
        baseline = assess(self.catalog)
        for slot, substitute in substitutes.items():
            with self.subTest(slot=slot):
                result = assess(self.catalog, {slot: substitute})
                self.assertTrue(result.compatible_design)
                self.assertFalse(result.physical_ready)
                self.assertEqual(result.choices[slot], substitute)
                self.assertEqual(
                    {k: result.choices[k] for k in SLOTS if k != slot},
                    {k: baseline.choices[k] for k in SLOTS if k != slot},
                )

    def test_8channel_pi_tdm_and_multiple_dacs_are_not_valid_714(self):
        for bad_output in ("rpi5-rp1-tdm8", "multiple-independent-usb-dacs"):
            with self.subTest(option=bad_output):
                with self.assertRaises(HardwareProfileError):
                    assess(self.catalog, {"output": bad_output})

    def test_wrong_codec_output_format_and_single_clock_gate(self):
        for mutated in (
            {"rate_hz": 44100},
            {"alsa_format": "S16_LE"},
            {"analog_outputs": 11},
            {"shared_clock": False},
            {"open_host_driver": False},
        ):
            with self.subTest(mutated=mutated):
                config = deepcopy(self.catalog)
                item = next(x for x in config["options"]["output"]
                            if x["id"] == config["selected"]["output"])
                item.update(mutated)
                with self.assertRaises(HardwareProfileError):
                    assess(config)

    def test_proprietary_earc_exception_is_only_lindy(self):
        config = deepcopy(self.catalog)
        config["selected"]["ingress"] = "mystery-earc"
        config["options"]["ingress"].append({
            "id": "mystery-earc", "input": "tv-earc",
            "output": "aurora-iec61937/v1", "hardware_closed": True,
            "approved_exception": True, "price_incremental_eur": 0,
        })
        with self.assertRaises(HardwareProfileError):
            assess(config)
        config["permitted_closed_external_hardware"].append("mystery-earc")
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "unsafe.json"
            p.write_text(json.dumps(config))
            with self.assertRaises(HardwareProfileError):
                load_catalog(p)

    def test_11_amplifiers_and_active_sub_are_mandatory(self):
        for modification in ({"analog_speaker_amps": 10}, {"subwoofer_type": "passive"}):
            with self.subTest(modification=modification):
                config = deepcopy(self.catalog)
                config["options"]["amplifier"][0].update(modification)
                with self.assertRaises(HardwareProfileError):
                    assess(config)

    def test_unknown_replacement_is_blocked(self):
        with self.assertRaises(HardwareProfileError):
            assess(self.catalog, {"output": "2ch-cheap-usb"})
        with self.assertRaises(HardwareProfileError):
            assess(self.catalog, {"fictional-module": "anything"})

    def test_catalog_cannot_silently_change_12ch_abi(self):
        config = deepcopy(self.catalog)
        config["canonical_bus"]["roles"][:2] = ["FR", "FL"]
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "tampered.json"
            p.write_text(json.dumps(config))
            with self.assertRaises(HardwareProfileError):
                load_catalog(p)

    def test_replacement_is_never_a_claim_of_runtime_update(self):
        profile = assess(self.catalog, {"compute": "pi5-2gb"})
        self.assertFalse(profile.physical_ready)
        self.assertIn("not benchmarked", " ".join(profile.blockers))
        self.assertIn("power off", " ".join(profile.replacement_procedure).lower())


if __name__ == "__main__":
    unittest.main()
