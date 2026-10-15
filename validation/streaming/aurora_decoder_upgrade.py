#!/usr/bin/env python3
"""Aurora audio plugin group contract and fail-closed A/B upgrade state machine.

This is a control-plane reference. Native decoder/renderer plugins stay in
separate processes and are never loaded/reloaded inside the audio callback.
Physical hardware and corpus evidence must come from an independent bench.
"""
from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
import json
from pathlib import Path
import re
from typing import Mapping

REQUIRED_OUTPUT = {
    "format": "s32le",
    "sample_rate_hz": 48_000,
    "channels": 12,
    "channel_roles": [
        "FL", "FR", "FC", "LFE", "SL", "SR", "SBL", "SBR",
        "TFL", "TFR", "TRL", "TRR",
    ],
}
REQUIRED_GATES = frozenset({
    "decoder-corpus", "renderer-shape", "channel-map",
    "stream-fault-recovery", "physical-output",
})
COMPATIBLE_VERSION_PAIRS = {
    ("v0.7.4", "v0.5.2", "legacy-pin-tested-together"),
    ("v0.8.0", "v0.6.0", "bridge_api-0.4"),
}


class BundleError(ValueError):
    pass


class UpgradeState(Enum):
    IDLE = "idle"
    STAGED = "staged"
    MUTED_PROBATION = "muted-probation"
    COMMITTED = "committed"
    ROLLED_BACK = "rolled-back"


@dataclass(frozen=True)
class GateEvidence:
    """A *reference* to externally collected and validated evidence.

    The real activation host MUST authenticate CI and physical evidence.
    This dataclass is only the policy input; creating an instance doesn't
    itself prove a loudspeaker or receiver was tested.
    """
    passed: frozenset[str]
    physical_attested: bool
    corpus_verified: bool
    firmware_checked: bool


def validate_catalog(data: Mapping) -> None:
    if data.get("schema_version") != 1:
        raise BundleError("unsupported bundle catalog schema")
    if data.get("canonical_output") != REQUIRED_OUTPUT:
        raise BundleError("7.1.4 / 48k S32LE or channel order contract changed")
    policy = data.get("upgrade_policy", {})
    if (policy.get("strategy") != "staged-paired-process-restart-atomic-pointer-rollback"
        or policy.get("allow_hot_swap_in_audio_callback") is not False
        or policy.get("keep_previous_bundle") is not True
        or policy.get("no_auto_upgrade") is not True
        or policy.get("advertise_only_physically_verified") is not True
        or frozenset(policy.get("required_test_gates", [])) != REQUIRED_GATES):
        raise BundleError("unsafe or incomplete update safety policy")
    bundles = data.get("bundles", [])
    if len(bundles) < 2 or len(bundles) > 16:
        raise BundleError("candidate and current stable slots required")
    names = set()
    for item in bundles:
        name = item.get("id")
        if not isinstance(name, str) or not name or name in names:
            raise BundleError("duplicate or empty decoder bundle id")
        names.add(name)
        parts = item.get("components", {})
        if set(parts) != {"harletty-bridge", "omniphony"}:
            raise BundleError("Harletty and Omniphony must be upgraded together")
        pair = (parts["harletty-bridge"].get("version"),
                parts["omniphony"].get("version"),
                item.get("expected_interface"))
        if pair not in COMPATIBLE_VERSION_PAIRS:
            raise BundleError("unverified incompatible Harletty/Omniphony ABI pair")
        for component in parts.values():
            commit = component.get("commit", "")
            if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit):
                raise BundleError("codec package commit must be full immutable 40-hex SHA")
            if not component.get("upstream", "").startswith("https://github.com/"):
                raise BundleError("untrusted upstream origin")
        if item.get("eligible_for_live_hotswap") is not False:
            raise BundleError("no in-callback hot swap is permitted")
        if not isinstance(item.get("physically_verified"), bool):
            raise BundleError("physical verification status must be explicit")
        if item.get("state") not in {
            "existing-software-fixture-validated", "research-candidate-not-integrated"
        }:
            raise BundleError("unrecognized codec bundle status")


def load_catalog(filename: Path) -> dict:
    data = json.loads(filename.read_text(encoding="utf-8"))
    validate_catalog(data)
    return data


class DecoderUpgrade:
    """A/B promotion choreography, with no runtime mutation before checkpoints.

    The running decoder epoch must be muted and drained OUTSIDE this class.
    Switching a real external subprocess needs an adapter that implements the
    same stage/check/restart/rollback interface; this reference never dlopens.
    """

    def __init__(self, catalog: Mapping, active_id: str):
        validate_catalog(catalog)
        self.catalog = catalog
        self.active = self._by_id(active_id)
        self.last_good = self.active
        self.standby = None
        self.generation = 0
        self.state = UpgradeState.IDLE
        self.muted = False

    def _by_id(self, name: str) -> Mapping:
        matches = [b for b in self.catalog["bundles"] if b["id"] == name]
        if len(matches) != 1:
            raise BundleError(f"unknown bundle {name}")
        return matches[0]

    def stage(self, candidate_id: str) -> None:
        if self.state not in {UpgradeState.IDLE, UpgradeState.ROLLED_BACK, UpgradeState.COMMITTED}:
            raise BundleError("cannot re-stage during active promotion")
        if candidate_id == self.active["id"]:
            raise BundleError("cannot re-stage the currently active bundle")
        candidate = self._by_id(candidate_id)
        self.standby = candidate
        self.state = UpgradeState.STAGED
        # Staging does not touch the currently active decoder.
        assert not self.muted

    def begin_muted_swap(self, evidence: GateEvidence) -> int:
        if self.state is not UpgradeState.STAGED or self.standby is None:
            raise BundleError("no staged candidate")
        if evidence.passed != REQUIRED_GATES:
            raise BundleError("missing decoder, renderer, recovery, channel, or physical gate")
        if not evidence.physical_attested or not evidence.corpus_verified or not evidence.firmware_checked:
            raise BundleError("physical/codec/firmware claims are not independently attested")
        if self.standby["state"] == "research-candidate-not-integrated":
            raise BundleError("upstream candidate has not passed Aurora integration")
        if not self.standby["physically_verified"]:
            raise BundleError("no physical acceptance for candidate")
        self.last_good = self.active
        self.muted = True
        self.generation += 1
        self.state = UpgradeState.MUTED_PROBATION
        return self.generation

    def confirm_new_worker(self, generation: int, *, decoder_healthy: bool,
                           output_contract_valid: bool, no_xruns: bool) -> bool:
        if self.state is not UpgradeState.MUTED_PROBATION:
            raise BundleError("no worker in probation")
        if generation != self.generation or not (decoder_healthy and output_contract_valid and no_xruns):
            self.rollback()
            return False
        assert self.standby is not None
        self.active = self.standby
        self.standby = None
        self.muted = False
        self.state = UpgradeState.COMMITTED
        return True

    def rollback(self) -> None:
        self.muted = True
        self.generation += 1
        self.active = self.last_good
        self.standby = None
        self.state = UpgradeState.ROLLED_BACK
        # Do not unmute until previous worker is independently re-primed.

    def previous_worker_recovered(self, *, decoder_healthy: bool,
                                  output_contract_valid: bool) -> bool:
        if self.state is not UpgradeState.ROLLED_BACK:
            raise BundleError("rollback not in progress")
        if decoder_healthy and output_contract_valid:
            self.muted = False
            return True
        return False


if __name__ == "__main__":
    import sys
    catalog = load_catalog(Path(sys.argv[1]))
    print(f"AURORA-DECODER-BUNDLE-CONTRACT-PASS bundles={len(catalog['bundles'])} "
          f"output=12ch/48000/S32LE physical_upgrade_pass=false")
