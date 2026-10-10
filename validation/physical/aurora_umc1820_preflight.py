#!/usr/bin/env python3
"""Fail-closed Linux ALSA preflight for Aurora's UMC1820 + ADA8200 profile."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import shutil
import subprocess
import sys


EXPECTED_FORMAT = "S24_3LE"
EXPECTED_CHANNELS = 20
EXPECTED_RATE = 48_000


def parse_playback_block(text: str) -> dict[str, object]:
    match = re.search(r"(?ms)^Playback:\s*\n(.*?)(?=^Capture:\s*$|\Z)", text)
    if not match:
        raise ValueError("missing Playback section")
    block = match.group(1)

    def one(pattern: str, label: str) -> str:
        found = re.search(pattern, block, re.MULTILINE)
        if not found:
            raise ValueError(f"missing playback {label}")
        return found.group(1).strip()

    fmt = one(r"^\s*Format:\s*(\S+)\s*$", "format")
    channels = int(one(r"^\s*Channels:\s*(\d+)\s*$", "channel count"))
    rates_text = one(r"^\s*Rates:\s*(.+?)\s*$", "rates")
    rates = [int(value) for value in re.findall(r"\d+", rates_text)]
    endpoint = None
    endpoint_match = re.search(r"^\s*Endpoint:\s*(.+?)\s*$", block, re.MULTILINE)
    if endpoint_match:
        endpoint = endpoint_match.group(1).strip()
    return {
        "format": fmt,
        "channels": channels,
        "rates": rates,
        "endpoint": endpoint,
    }


def resolve_stream0(proc_root: pathlib.Path, card: str) -> tuple[pathlib.Path, str]:
    direct = proc_root / card / "stream0"
    if direct.exists():
        return direct, card

    cards_file = proc_root / "cards"
    if cards_file.exists():
        cards = cards_file.read_text(encoding="utf-8", errors="replace")
        pattern = re.compile(r"^\s*(\d+)\s+\[([^\]]+)\]\s*:", re.MULTILINE)
        for number, identifier in pattern.findall(cards):
            if identifier.strip().lower() == card.lower():
                candidate = proc_root / f"card{number}" / "stream0"
                if candidate.exists():
                    return candidate, number
    raise FileNotFoundError(
        f"could not resolve ALSA card {card!r}; expected {direct} or matching /proc/asound/cards"
    )


def evaluate(info: dict[str, object]) -> list[str]:
    errors: list[str] = []
    if info["channels"] != EXPECTED_CHANNELS:
        errors.append(
            f"expected {EXPECTED_CHANNELS} playback channels, got {info['channels']}; "
            "the UMC1820 + ADA8200 profile requires the 20-channel ADAT topology"
        )
    if info["format"] != EXPECTED_FORMAT:
        errors.append(
            f"expected hardware playback format {EXPECTED_FORMAT}, got {info['format']}"
        )
    rates = info["rates"]
    assert isinstance(rates, list)
    if EXPECTED_RATE not in rates:
        errors.append(f"{EXPECTED_RATE} Hz is not advertised by the playback endpoint")
    return errors


def parse_mixer_values(text: str) -> list[str]:
    matches = re.findall(r"(?m)^\s*: values=(.+?)\s*$", text)
    if not matches:
        raise ValueError("missing mixer values")
    return [value.strip() for value in matches[-1].split(",")]


def inspect_mixer(card: str, require_controls: bool) -> tuple[dict[str, object], list[str]]:
    result: dict[str, object] = {
        "amixer_available": shutil.which("amixer") is not None,
        "playback_switch": None,
        "playback_volume": None,
    }
    errors: list[str] = []
    if not result["amixer_available"]:
        if require_controls:
            errors.append("amixer is required to verify UMC1820 playback mute state")
        return result, errors

    controls = (
        ("playback_switch", "UMC1820 Output Playback Switch"),
        ("playback_volume", "UMC1820 Output Playback Volume"),
    )
    for key, name in controls:
        process = subprocess.run(
            ["amixer", "-c", card, "cget", f"name={name}"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        if process.returncode != 0:
            result[key] = {
                "available": False,
                "returncode": process.returncode,
                "stderr": process.stderr.strip(),
            }
            if require_controls:
                errors.append(f"missing ALSA mixer control {name!r}")
            continue
        try:
            values = parse_mixer_values(process.stdout)
        except ValueError as error:
            result[key] = {
                "available": True,
                "parse_error": str(error),
                "raw": process.stdout,
            }
            if require_controls:
                errors.append(f"could not parse ALSA mixer control {name!r}")
            continue
        result[key] = {"available": True, "values": values}

    switch = result.get("playback_switch")
    if isinstance(switch, dict) and switch.get("available") is True:
        values = switch.get("values")
        if isinstance(values, list):
            if len(values) < 16:
                errors.append(
                    f"UMC1820 playback switch exposes {len(values)} values; expected at least 16"
                )
            else:
                muted = [index + 1 for index, value in enumerate(values[:16]) if value.lower() != "on"]
                if muted:
                    errors.append(
                        "UMC1820 playback outputs muted in ALSA mixer: "
                        + ",".join(map(str, muted))
                    )

    volume = result.get("playback_volume")
    if isinstance(volume, dict) and volume.get("available") is True:
        values = volume.get("values")
        if isinstance(values, list):
            numeric: list[int] = []
            for value in values[:16]:
                match = re.search(r"-?\d+", value)
                if match:
                    numeric.append(int(match.group(0)))
            if len(numeric) >= 16:
                zero = [index + 1 for index, value in enumerate(numeric[:16]) if value <= 0]
                if zero:
                    errors.append(
                        "UMC1820 playback volumes are zero for outputs: "
                        + ",".join(map(str, zero))
                    )

    return result, errors


def synthetic_stream() -> str:
    return """BEHRINGER UMC1820 at usb-0000:00:1d.0-1.3, high speed : USB Audio

Playback:
  Status: Stop
  Interface 1
    Altset 1
    Format: S24_3LE
    Channels: 20
    Endpoint: 1 OUT (ASYNC)
    Rates: 44100, 48000, 88200, 96000

Capture:
  Status: Stop
  Interface 2
    Altset 1
    Format: S32_LE
    Channels: 18
    Endpoint: 2 IN (ASYNC)
    Rates: 44100, 48000, 88200, 96000
"""


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--card", default="UMC1820")
    parser.add_argument("--proc-root", default="/proc/asound")
    parser.add_argument("--report")
    parser.add_argument("--require-mixer-controls", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        info = parse_playback_block(synthetic_stream())
        errors = evaluate(info)
        if errors:
            raise SystemExit("self-test failed: " + "; ".join(errors))
        broken = synthetic_stream().replace("Channels: 20", "Channels: 12")
        broken_info = parse_playback_block(broken)
        if not evaluate(broken_info):
            raise SystemExit("self-test failed to reject 12-channel topology")
        switch = parse_mixer_values(": values=on,on,on,off\n")
        if switch != ["on", "on", "on", "off"]:
            raise SystemExit("self-test failed to parse mixer values")
        print(
            "AURORA-UMC1820-PREFLIGHT-SELFTEST-PASS "
            f"channels={info['channels']} format={info['format']} rates={','.join(map(str, info['rates']))}"
        )
        return 0

    try:
        stream_path, resolved_card = resolve_stream0(pathlib.Path(args.proc_root), args.card)
        text = stream_path.read_text(encoding="utf-8", errors="replace")
        info = parse_playback_block(text)
        errors = evaluate(info)
        mixer, mixer_errors = inspect_mixer(resolved_card, args.require_mixer_controls)
        errors.extend(mixer_errors)
    except (OSError, ValueError) as error:
        print(f"AURORA-UMC1820-PREFLIGHT-FAIL: {error}", file=sys.stderr)
        return 2

    report = {
        "card_requested": args.card,
        "card_resolved": resolved_card,
        "stream0": str(stream_path),
        "playback": info,
        "mixer": mixer,
        "expected": {
            "format": EXPECTED_FORMAT,
            "channels": EXPECTED_CHANNELS,
            "sample_rate": EXPECTED_RATE,
        },
        "pass": not errors,
        "errors": errors,
        "notes": [
            "Use UMC1820 OPT I/O = ADAT at 48 kHz.",
            "Use one UMC1820 USB clock domain; ADA8200 is the ADAT expansion.",
            "Aurora's umc1820-ada8200 profile emits 20 USB slots and keeps unused slots silent.",
            "Use a conversion-capable ALSA endpoint such as plughw because Aurora emits S32_LE while the hardware endpoint advertises S24_3LE.",
            "Some Linux/ALSA combinations have been observed with UMC1820 playback outputs muted by default; mixer state is checked when the controls are available.",
        ],
    }

    if args.report:
        target = pathlib.Path(args.report)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if errors:
        print("AURORA-UMC1820-PREFLIGHT-FAIL: " + "; ".join(errors), file=sys.stderr)
        return 1

    print(
        "AURORA-UMC1820-PREFLIGHT-PASS "
        f"card={resolved_card} channels={info['channels']} format={info['format']} "
        f"rates={','.join(map(str, info['rates']))}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
