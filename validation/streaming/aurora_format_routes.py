#!/usr/bin/env python3
"""Truthful format negotiation for all Aurora streaming ingress families.

A route decision is NOT proof of an available decoder or HDMI electrical sink.
Only VERIFIED backends may be advertised. A format with no backend never
silently turns into a claimed Atmos track or a fake stereo fallback.
"""
from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import FrozenSet


class Route(Enum):
    CHANNEL_BED = "channel-bed"
    NATIVE_OBJECTS = "native-objects"
    LPCM_BED = "lpcm-bed"
    EXTERNAL_CODEC = "external-codec"
    HOLD_FOR_OBJECT_PROBE = "hold-for-object-probe"
    BLOCKED = "blocked"


class ObjectMetadata(Enum):
    NONE = "none"
    VERIFIED = "verified"
    UNKNOWN = "unknown"


@dataclass(frozen=True)
class AudioSource:
    transport: str
    codec: str
    channels: int | None
    objects: ObjectMetadata = ObjectMetadata.UNKNOWN


@dataclass(frozen=True)
class AudioRoute:
    route: Route
    backend: str | None
    canonical_channels: int
    native_objects: bool
    synthetic_extensions: bool
    reason: str


# Source codecs/interfaces that Aurora must not forget as it expands.
# A proposed external adapter is not automatically an installed decoder.
REQUIRED_FORMATS: FrozenSet[str] = frozenset({
    "pcm", "dolby-mat", "ac3", "eac3", "eac3-joc", "truehd",
    "truehd-atmos", "dts", "dts-hd", "dts-x", "ac4", "mpeg-h",
    "iamf", "aac", "he-aac", "xhe-aac", "opus", "flac",
    "alac", "vorbis", "mp3",
})

# Routes below are intentionally conservative and do NOT claim the physical
# transport, object decoder, or legal right to use a proprietary codec.
AVAILABLE_SOFTWARE = frozenset({
    "bed-ac3", "bed-eac3", "lpcm-raw", "joc-fixture",
})

POTENTIAL_BACKENDS = {
    "truehd": "ffmpeg-truehd-bed",
    "truehd-atmos": "licensed-truehd-object-decoder",
    "dts": "ffmpeg-dts-bed",
    "dts-hd": "ffmpeg-dts-hd-bed",
    "dts-x": "licensed-dtsx-object-decoder",
    "ac4": "licensed-ac4-decoder",
    "mpeg-h": "mpeg-h-decoder",
    "iamf": "libiamf-decoder",
    "aac": "ffmpeg-aac",
    "he-aac": "ffmpeg-heaac",
    "xhe-aac": "verified-xhe-aac-decoder",
    "opus": "ffmpeg-opus",
    "flac": "ffmpeg-flac",
    "alac": "ffmpeg-alac",
    "vorbis": "ffmpeg-vorbis",
    "mp3": "ffmpeg-mp3",
}

ALLOWED_TRANSPORTS = frozenset({
    "iec61937", "hdmi-lpcm", "hdmi-mat", "media-file",
    "stream-app-encoded", "network-audio",
})


def choose_route(
    source: AudioSource,
    *,
    verified_backends: FrozenSet[str] = AVAILABLE_SOFTWARE,
) -> AudioRoute:
    def blocked(why: str, backend: str | None = None) -> AudioRoute:
        return AudioRoute(Route.BLOCKED, backend, 12, False, False, why)

    if source.transport not in ALLOWED_TRANSPORTS:
        return blocked("unrecognized audio transport; no IEC61937 guessing")
    if source.codec not in REQUIRED_FORMATS:
        return blocked("unknown codec; must add and verify an adapter")
    if source.channels is not None and not (1 <= source.channels <= 32):
        return blocked("impossible source channel count")

    if source.codec == "pcm":
        if source.transport != "hdmi-lpcm" or source.channels not in {2, 6, 8}:
            return blocked("LPCM requires separately signaled HDMI layout")
        if source.objects is not ObjectMetadata.NONE:
            return blocked("PCM object metadata not verified; MAT is separate")
        if "lpcm-raw" not in verified_backends:
            return blocked("LPCM adapter is not verified", "lpcm-raw")
        return AudioRoute(
            Route.LPCM_BED, "lpcm-raw", 12, False, source.channels < 12,
            "PCM signal path; derived extension channels are not Dolby Atmos",
        )

    if source.codec == "dolby-mat":
        if source.transport != "hdmi-mat":
            return blocked("Dolby MAT must arrive through a documented HDMI MAT transport")
        backend = "licensed-mat-object-decoder"
        if backend not in verified_backends:
            return blocked("Dolby MAT object extraction backend not yet proven", backend)
        return AudioRoute(Route.NATIVE_OBJECTS, backend, 12, True, False,
                          "native MAT object metadata proved by dedicated backend")

    if source.codec in {"ac3", "eac3", "eac3-joc"}:
        if source.transport != "iec61937":
            return blocked("AC-3 / DD+ channel path requires verified IEC61937 transport")
        if source.codec == "ac3":
            if source.objects is not ObjectMetadata.NONE:
                return blocked("AC-3 source objects must be declared absent")
            backend = "bed-ac3"
            if backend not in verified_backends:
                return blocked("AC-3 decoder unverified", backend)
            return AudioRoute(Route.CHANNEL_BED, backend, 12, False, True,
                              "AC-3 bed; synthetic surround/height channels")
        # Data type 0x15 alone CANNOT reveal DD+ JOC.
        if source.objects is ObjectMetadata.UNKNOWN:
            return AudioRoute(
                Route.HOLD_FOR_OBJECT_PROBE, None, 12, False, False,
                "E-AC-3 carrier may contain JOC: probe object semantics before routing",
            )
        if source.objects is ObjectMetadata.VERIFIED:
            backend = "joc-fixture"
            if backend not in verified_backends:
                return blocked("native object backend is not enabled", backend)
            return AudioRoute(Route.NATIVE_OBJECTS, backend, 12, True, False,
                              "JOC source object metadata verified by independent decoder")
        if source.codec == "eac3-joc":
            return blocked("JOC must have object semantics, not a plain DD+ downgrade")
        backend = "bed-eac3"
        if backend not in verified_backends:
            return blocked("DD+ bed decoder not verified", backend)
        return AudioRoute(Route.CHANNEL_BED, backend, 12, False, True,
                          "plain DD+ bed; synthetic extra channels")

    backend = POTENTIAL_BACKENDS.get(source.codec)
    if backend is None:
        return blocked("no defined decoder adapter")
    if backend not in verified_backends:
        return blocked("format recognized but backend is NOT currently verified", backend)
    if source.codec.endswith("atmos") or source.codec == "dts-x":
        if source.objects is not ObjectMetadata.VERIFIED:
            return blocked("native object metadata has not been verified", backend)
        return AudioRoute(Route.NATIVE_OBJECTS, backend, 12, True, False,
                          "verified native object decoder")
    if source.channels not in {2, 6, 8}:
        return blocked("codec backend needs explicit channel mapping")
    return AudioRoute(Route.EXTERNAL_CODEC, backend, 12, False, True,
                      "validated PCM channel-bed decoder; missing channels synthesized")


def advertised_codecs(verified_backends: FrozenSet[str]) -> FrozenSet[str]:
    """Never claim HDMI codec capability based only on speculative adapters."""
    result = set()
    if "bed-ac3" in verified_backends:
        result.add("ac3")
    if "bed-eac3" in verified_backends:
        result.add("eac3")
    if "joc-fixture" in verified_backends:
        # This is a SOFTWARE fixture only. Physical EDID admission is separate.
        result.add("eac3-joc-software-fixture")
    if "lpcm-raw" in verified_backends:
        result.add("lpcm-software")
    return frozenset(result)
