#!/usr/bin/env python3
"""Synthetic codec-change and fail-closed decoder-generation contract tests."""
import unittest
from aurora_format_routes import AudioSource, ObjectMetadata
from aurora_stream_epoch import AuroraIngressEpochs, DecoderEvidence, EpochState

class EpochContractTest(unittest.TestCase):
    def evidence(self, system, native=False, synthetic=True, **changes):
        payload = dict(
            epoch=system.epoch, backend=system.selected.backend,
            output_channels=12, sample_rate_hz=48_000, valid_pcm_frames=256,
            object_metadata_recovered=native, synthetic_extensions=synthetic,
            finite_samples=True, channel_map_verified=True,
        )
        payload.update(changes)
        return DecoderEvidence(**payload)

    def test_switch_from_ac3_to_plain_eac3_without_leaking_old_pcm(self):
        core = AuroraIngressEpochs()
        a=core.new_source(AudioSource("iec61937","ac3",6,ObjectMetadata.NONE))
        self.assertEqual(a.action, "mute-reset-start-decoder")
        self.assertEqual(core.decoder_evidence(self.evidence(core)).state, EpochState.AUDIBLE)
        old_epoch=core.epoch
        probe=core.new_source(AudioSource("iec61937","eac3",6,ObjectMetadata.UNKNOWN))
        self.assertEqual(probe.state,EpochState.WAIT_OBJECT_PROOF)
        self.assertGreater(core.epoch,old_epoch)
        self.assertEqual(core.accept_object_probe(
            AudioSource("iec61937","eac3",6,ObjectMetadata.UNKNOWN)
        ).action,"keep-muted-probe")
        ready=core.accept_object_probe(AudioSource("iec61937","eac3",6,ObjectMetadata.NONE))
        self.assertEqual(ready.state,EpochState.DECODER_PRIMING)
        self.assertEqual(core.decoder_evidence(self.evidence(core,epoch=old_epoch)).state,
                         EpochState.FAULTED)
        reset=core.discontinuity("stale decoder frame")
        self.assertEqual(reset.state,EpochState.MUTED)
        self.assertEqual(reset.action,"mute-flush-and-reprobe")
        core.new_source(AudioSource("iec61937","eac3",6,ObjectMetadata.NONE))
        self.assertEqual(core.decoder_evidence(self.evidence(core)).state, EpochState.AUDIBLE)

    def test_verified_joc_and_unproved_mat(self):
        core=AuroraIngressEpochs()
        core.new_source(AudioSource("iec61937","eac3",6,ObjectMetadata.UNKNOWN))
        core.accept_object_probe(AudioSource("iec61937","eac3",6,ObjectMetadata.VERIFIED))
        self.assertEqual(core.decoder_evidence(self.evidence(
            core,native=True,synthetic=False)).state,EpochState.AUDIBLE)
        blocked=core.new_source(AudioSource("hdmi-mat","dolby-mat",8,ObjectMetadata.VERIFIED))
        self.assertEqual(blocked.action,"mute-unsupported")
        self.assertEqual(blocked.state,EpochState.UNSUPPORTED)

    def test_no_unverified_codec_or_decoder_unmute(self):
        core=AuroraIngressEpochs()
        unknown=core.new_source(AudioSource("stream-app-encoded","ac4",8,ObjectMetadata.UNKNOWN))
        self.assertEqual(unknown.state,EpochState.UNSUPPORTED)
        source=AudioSource("hdmi-lpcm","pcm",8,ObjectMetadata.NONE)
        core.new_source(source)
        self.assertEqual(core.decoder_evidence(self.evidence(core,native=True)).state,
                         EpochState.FAULTED)

    def test_malformed_probe_cannot_start_incompatible_decoder(self):
        core=AuroraIngressEpochs()
        core.new_source(AudioSource("iec61937","eac3",6,ObjectMetadata.UNKNOWN))
        out=core.accept_object_probe(AudioSource("iec61937","ac3",6,ObjectMetadata.NONE))
        self.assertEqual(out.state, EpochState.FAULTED)

if __name__ == "__main__":
    unittest.main()
