#!/usr/bin/env python3
"""Test that all known streaming format families are represented honestly."""
from __future__ import annotations

import unittest
from aurora_format_routes import (
    AudioSource, AVAILABLE_SOFTWARE, ObjectMetadata, Route, REQUIRED_FORMATS,
    POTENTIAL_BACKENDS, advertised_codecs, choose_route,
)


class FormatAdmissonTests(unittest.TestCase):
    def test_all_required_formats_have_explicit_admission(self):
        self.assertGreaterEqual(len(REQUIRED_FORMATS), 20)
        for codec in REQUIRED_FORMATS:
            transport = "hdmi-mat" if codec == "dolby-mat" else (
                "hdmi-lpcm" if codec == "pcm" else (
                    "iec61937" if codec in {"ac3", "eac3", "eac3-joc"} else
                    "stream-app-encoded"
                )
            )
            objects = ObjectMetadata.UNKNOWN
            route = choose_route(AudioSource(transport, codec, 6, objects))
            self.assertIsInstance(route.route, Route)
            self.assertEqual(route.canonical_channels, 12)
            if codec not in {"pcm", "dolby-mat", "ac3", "eac3", "eac3-joc"}:
                self.assertIn(codec, POTENTIAL_BACKENDS)
                self.assertEqual(route.route, Route.BLOCKED,
                                 f"{codec}: unverified external backend must not be advertised")

    def test_verified_plain_formats_keep_truthful_object_labels(self):
        for codec in ["ac3", "eac3"]:
            source = AudioSource("iec61937", codec, 6, ObjectMetadata.NONE)
            selection = choose_route(source)
            self.assertEqual(selection.route, Route.CHANNEL_BED)
            self.assertFalse(selection.native_objects)
            self.assertTrue(selection.synthetic_extensions)

    def test_ddp_joc_carrier_cannot_be_inferred_from_data_type_only(self):
        unknown = AudioSource("iec61937", "eac3", 6, ObjectMetadata.UNKNOWN)
        self.assertEqual(choose_route(unknown).route, Route.HOLD_FOR_OBJECT_PROBE)
        confirmed = AudioSource("iec61937", "eac3", 6, ObjectMetadata.VERIFIED)
        res = choose_route(confirmed)
        self.assertEqual(res.route, Route.NATIVE_OBJECTS)
        self.assertTrue(res.native_objects)
        fake_claim = AudioSource("iec61937", "eac3-joc", 6, ObjectMetadata.NONE)
        self.assertEqual(choose_route(fake_claim).route, Route.BLOCKED)

    def test_7_1_pcm_passes_only_from_explicit_lpcm_transport(self):
        proper = AudioSource("hdmi-lpcm", "pcm", 8, ObjectMetadata.NONE)
        self.assertEqual(choose_route(proper).route, Route.LPCM_BED)
        guessed = AudioSource("iec61937", "pcm", 8, ObjectMetadata.NONE)
        self.assertEqual(choose_route(guessed).route, Route.BLOCKED)

    def test_mat_is_not_faked_as_eac3_or_basic_pcm(self):
        mat = AudioSource("hdmi-mat", "dolby-mat", 8, ObjectMetadata.VERIFIED)
        self.assertEqual(choose_route(mat).route, Route.BLOCKED)
        self.assertEqual(choose_route(mat, verified_backends=frozenset({
            "licensed-mat-object-decoder"
        })).route, Route.NATIVE_OBJECTS)

    def test_no_unverified_advertising(self):
        declared = advertised_codecs(AVAILABLE_SOFTWARE)
        self.assertIn("ac3", declared)
        self.assertIn("eac3", declared)
        self.assertNotIn("dolby-mat", declared)
        self.assertNotIn("ac4", declared)
        self.assertNotIn("truehd", declared)
        self.assertNotIn("dts-x", declared)

    def test_missing_backend_is_not_a_stereo_fallback(self):
        for codec in ("ac4", "mpeg-h", "dts-x", "truehd-atmos", "xhe-aac"):
            actual = choose_route(AudioSource(
                "stream-app-encoded", codec, 8, ObjectMetadata.VERIFIED
            ))
            self.assertEqual(actual.route, Route.BLOCKED)
            self.assertEqual(actual.canonical_channels, 12)
            self.assertFalse(actual.native_objects)

    def test_rejects_impossible_metadata_or_channel_counts(self):
        self.assertEqual(choose_route(AudioSource(
            "iec61937", "ac3", 6, ObjectMetadata.VERIFIED,
        )).route, Route.BLOCKED)
        self.assertEqual(choose_route(AudioSource(
            "hdmi-lpcm", "pcm", 33, ObjectMetadata.NONE,
        )).route, Route.BLOCKED)
        self.assertEqual(choose_route(AudioSource(
            "hdmi-lpcm", "unknown", 8, ObjectMetadata.NONE,
        )).route, Route.BLOCKED)


if __name__ == "__main__":
    unittest.main()
