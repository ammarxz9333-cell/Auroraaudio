#!/usr/bin/env python3
"""Deterministic and strict Aurora 7.1.4 component-chain contracts.

Control-plane validator only. It does NOT start plugins, negotiate eARC,
enable an unverified codec, or prove 12 physical loudspeaker channels.
"""
from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path
from typing import Any, Mapping

OUTPUT_ROLES = (
    "FL", "FR", "FC", "LFE", "SL", "SR",
    "SBL", "SBR", "TFL", "TFR", "TRL", "TRR",
)
REQUIRED_STEPS = (
    "stage-new-generation",
    "run-component-conformance",
    "validate-adjacent-interfaces",
    "mute-and-flush",
    "activate-on-media-control-thread",
    "verify-current-generation-only",
    "rollback-to-last-known-good-on-failure",
)
FORBIDDEN_RT_CALLS = frozenset({
    "dlopen", "untrusted-plugin-call", "subprocess",
    "file-io", "network-block", "dynamic-version-switch",
})
EXPECTED_MODULES = (
    "earc-ingress", "format-router", "decoder-worker",
    "spatial-renderer", "post-dsp", "speaker-output",
    "wireless-speaker-fabric",
)
EXPECTED_KINDS = (
    "transport", "control", "codec", "renderer",
    "dsp", "output", "wireless",
)
EXTERNAL_SOURCE = "audio-carrier-source/v1"
FINAL_CONTRACT = "timed-speaker-output/v1"
PCM_CONTRACT = "aurora-12ch-pcm/v1"


class ComponentContractError(ValueError):
    pass


@dataclass(frozen=True)
class ModuleContract:
    name: str
    kind: str
    implementation: str
    requires: str
    provides: str
    independent: bool

    @staticmethod
    def decode(item: Mapping[str, Any]) -> "ModuleContract":
        name = item.get("id", "")
        if not isinstance(name, str) or not name:
            raise ComponentContractError("empty module identity")
        fields = ("kind", "implementation", "requires", "provides")
        if any(not isinstance(item.get(key), str) or not item[key] for key in fields):
            raise ComponentContractError("module requires nonblank typed contracts")
        if item.get("independent") is not True:
            raise ComponentContractError("module may not bypass independent upgrade boundary")
        if "/v" not in item["requires"] or "/v" not in item["provides"]:
            raise ComponentContractError("module inputs and outputs need explicit ABI version")
        if any(".." in segment for segment in item["implementation"].split("/")):
            raise ComponentContractError("module implementation escapes repository")
        return ModuleContract(
            name, item["kind"], item["implementation"],
            item["requires"], item["provides"], True,
        )


@dataclass(frozen=True)
class AudioComponentGraph:
    modules: tuple[ModuleContract, ...]
    channels: int
    sample_rate_hz: int
    sample_format: str
    channel_roles: tuple[str, ...]

    def index(self, module_id: str) -> int:
        try:
            return next(i for i, module in enumerate(self.modules) if module.name == module_id)
        except StopIteration as exc:
            raise ComponentContractError(f"unknown Aurora component: {module_id}") from exc

    def replace(self, candidate: ModuleContract) -> "AudioComponentGraph":
        index = self.index(candidate.name)
        if candidate.kind != self.modules[index].kind:
            raise ComponentContractError("module kind changed across upgrade")
        swapped = list(self.modules)
        swapped[index] = candidate
        return AudioComponentGraph(
            tuple(swapped), self.channels, self.sample_rate_hz,
            self.sample_format, self.channel_roles,
        ).verify()

    def verify(self) -> "AudioComponentGraph":
        if len(self.modules) != len(EXPECTED_MODULES):
            raise ComponentContractError("missing or unexpected audio pipeline stages")
        if tuple(x.name for x in self.modules) != EXPECTED_MODULES:
            raise ComponentContractError("pipeline order/identity changed without migration")
        if tuple(x.kind for x in self.modules) != EXPECTED_KINDS:
            raise ComponentContractError("module responsibilities changed without migration")
        if self.modules[0].requires != EXTERNAL_SOURCE:
            raise ComponentContractError("unexpected input transport API")
        if self.modules[-1].provides != FINAL_CONTRACT:
            raise ComponentContractError("speaker output contract changed")
        for first, second in zip(self.modules, self.modules[1:]):
            if first.provides != second.requires:
                raise ComponentContractError(
                    f"incompatible interface: {first.name}:{first.provides}"
                    f" -> {second.name}:{second.requires}"
                )
        for name in ("spatial-renderer", "post-dsp", "speaker-output"):
            item = self.modules[self.index(name)]
            if name == "spatial-renderer" and item.provides != PCM_CONTRACT:
                raise ComponentContractError("spatial renderer violated 12-channel contract")
            if name == "post-dsp" and (item.requires != PCM_CONTRACT or item.provides != PCM_CONTRACT):
                raise ComponentContractError("DSP violated fixed PCM format")
            if name == "speaker-output" and item.requires != PCM_CONTRACT:
                raise ComponentContractError("speaker output PCM contract changed")
        if (self.channels != 12 or self.sample_rate_hz != 48_000
                or self.sample_format != "S32_LE" or self.channel_roles != OUTPUT_ROLES):
            raise ComponentContractError("Aurora canonical 7.1.4 bus changed")
        return self


def load_graph(path: Path) -> AudioComponentGraph:
    data = json.loads(path.read_text(encoding="utf-8"))
    if data.get("schema_version") != 1:
        raise ComponentContractError("unknown component ABI catalog schema")
    if tuple(data.get("required_update_protocol", [])) != REQUIRED_STEPS:
        raise ComponentContractError("missing immutable upgrade transition rule")
    if frozenset(data.get("callback_forbidden", [])) != FORBIDDEN_RT_CALLS:
        raise ComponentContractError("unsafe audio callback permissions")
    bus = data.get("audio_bus", {})
    if not isinstance(bus, dict):
        raise ComponentContractError("audio bus must be a dictionary")
    raw = data.get("modules")
    if not isinstance(raw, list):
        raise ComponentContractError("modules must be an ordered array")
    graph = AudioComponentGraph(
        tuple(ModuleContract.decode(i) for i in raw),
        bus.get("channels"), bus.get("sample_rate_hz"),
        bus.get("format"), tuple(bus.get("role_order", [])),
    )
    return graph.verify()


if __name__ == "__main__":
    import sys
    graph = load_graph(Path(sys.argv[1]))
    print(f"AURORA-COMPONENT-CONTRACT-PASS stages={len(graph.modules)} "
          f"output={graph.channels}ch/{graph.sample_rate_hz}/{graph.sample_format} "
          "physical_acceptance=false")
