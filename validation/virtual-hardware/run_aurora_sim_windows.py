#!/usr/bin/env python3
"""Run Aurora's full-system JOC + virtual-hardware regression natively on Windows.

This is the laptop convenience lane. It verifies exact carrier/component identity,
runs the real pinned Harletty/Omniphony path, captures moving-object telemetry,
runs a media-paced 12-channel render, then drives the merged virtual TDM16/DAC
model. The independent OpenJOC reference lane remains authoritative Linux CI.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import urllib.request
import zipfile
from pathlib import Path

DOLBY_URL = "https://ott.dolby.com/OnDelKits/DDP/Dolby_Digital_Plus_Online_Delivery_Kit_v1.4.1/Test_Signals/elementary_streams/audio.zip"
DOLBY_NAME = "Living-Room-Atmos_6ch_640kbps_ddp_joc.ec3"
ZIP_SHA = "f94d5e3e933f756856686546763f42a8a5f16b10c264fc7af1d228acc09baa62"
SOURCE_SHA = "2470373db2c3621d56a2852df070e140293e9a99fdaa07e5c06de3c86bec307f"
DERIVED_SHA = "0219a241559de5231f31c6093072740ff9fe0657b3354541bc6838ef2d5e5be0"
FIRST_AU_BYTES = 2560
HARLETTY_URL = "https://github.com/harletty/harletty-bridge/releases/download/v0.7.4/harletty-bridge-v0.7.4-windows-x86_64.zip"
HARLETTY_ZIP_SHA = "3ed126e5bb837882c5c2abbc5d35d1ebede199f81ed64127fb968bfe86bd6686"
ASIO_COMMIT = "496a0765b8bb9c26f764f22f9a9712a937177db2"
ASIO_URL = f"https://github.com/audiosdk/asio/archive/{ASIO_COMMIT}.zip"
SYNC = bytes.fromhex("72f81f4e")


def phase(text: str) -> None:
    print(f"\n== AuroraSim Windows: {text} ==", flush=True)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def assert_sha(path: Path, expected: str, label: str) -> None:
    actual = sha256(path)
    if actual != expected:
        raise RuntimeError(f"{label} SHA-256 mismatch: expected {expected} got {actual}")


def download_verified(url: str, destination: Path, expected: str, label: str) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        try:
            assert_sha(destination, expected, label)
            return
        except RuntimeError:
            destination.unlink()
    request = urllib.request.Request(url, headers={"User-Agent": "Aurora-validation/1"})
    with urllib.request.urlopen(request, timeout=60) as response, destination.open("wb") as out:
        shutil.copyfileobj(response, out)
    assert_sha(destination, expected, label)


def command(name: str, hint: str = "") -> str:
    found = shutil.which(name)
    if not found:
        suffix = f" {hint}" if hint else ""
        raise RuntimeError(f"Missing required command {name!r}.{suffix}")
    return found


def run(args: list[str], *, cwd: Path | None = None, env: dict[str, str] | None = None, check: bool = True) -> subprocess.CompletedProcess[str]:
    print("+ " + subprocess.list2cmdline(args), flush=True)
    result = subprocess.run(args, cwd=cwd, env=env, text=True)
    if check and result.returncode != 0:
        raise RuntimeError(f"Command failed ({result.returncode}): {subprocess.list2cmdline(args)}")
    return result


def capture(args: list[str], *, cwd: Path | None = None, env: dict[str, str] | None = None) -> str:
    result = subprocess.run(args, cwd=cwd, env=env, text=True, capture_output=True)
    if result.returncode != 0:
        raise RuntimeError(
            f"Command failed ({result.returncode}): {subprocess.list2cmdline(args)}\n{result.stdout}\n{result.stderr}"
        )
    return result.stdout.strip()


def clone_pinned(git: str, url: str, version: str, commit: str, dest: Path) -> None:
    if (dest / ".git").exists():
        run([git, "-C", str(dest), "fetch", "--depth", "1", "origin", version])
        run([git, "-C", str(dest), "reset", "--hard", commit])
        run([git, "-C", str(dest), "clean", "-fdx"])
    else:
        if dest.exists():
            shutil.rmtree(dest)
        run([git, "clone", "--quiet", "--depth", "1", "--branch", version, url, str(dest)])
    actual = capture([git, "-C", str(dest), "rev-parse", "HEAD"])
    if actual != commit:
        raise RuntimeError(f"Pinned commit mismatch for {url}: expected {commit} got {actual}")


def setup_asio(cache: Path, env: dict[str, str]) -> None:
    archive = cache / f"asio-{ASIO_COMMIT}.zip"
    root = cache / "asio-sdk"
    sdk = root / f"asio-{ASIO_COMMIT}"
    required = [sdk / "common/asio.h", sdk / "common/asiosys.h", sdk / "host/asiodrivers.h"]
    if not all(path.exists() for path in required):
        if root.exists():
            shutil.rmtree(root)
        root.mkdir(parents=True)
        request = urllib.request.Request(ASIO_URL, headers={"User-Agent": "Aurora-validation/1"})
        with urllib.request.urlopen(request, timeout=60) as response, archive.open("wb") as out:
            shutil.copyfileobj(response, out)
        with zipfile.ZipFile(archive) as zf:
            zf.extractall(root)
    if not all(path.exists() for path in required):
        missing = [str(path) for path in required if not path.exists()]
        raise RuntimeError(f"ASIO SDK extraction incomplete: {missing}")
    env["CPAL_ASIO_DIR"] = str(sdk)

    if not env.get("LIBCLANG_PATH"):
        candidates = [
            Path(r"C:\Program Files\LLVM\bin"),
            Path(r"C:\Program Files (x86)\LLVM\bin"),
        ]
        for candidate in candidates:
            if (candidate / "libclang.dll").exists():
                env["LIBCLANG_PATH"] = str(candidate)
                break


def parse_iec(path: Path) -> tuple[int, int]:
    data = path.read_bytes()
    if not data.startswith(SYNC):
        raise RuntimeError("IEC61937 carrier does not start with sync")
    burst_bytes = data.find(SYNC, 4)
    if burst_bytes <= 0 or len(data) % burst_bytes:
        raise RuntimeError("Cannot derive fixed IEC61937 burst geometry")
    bursts = len(data) // burst_bytes
    if bursts != 2360:
        raise RuntimeError(f"Expected 2360 IEC61937 bursts, got {bursts}")
    for index in range(bursts):
        offset = index * burst_bytes
        if data[offset : offset + 4] != SYNC:
            raise RuntimeError(f"IEC61937 sync lost at burst {index}")
        if data[offset + 4] & 0x1F != 0x15:
            raise RuntimeError(f"Non-E-AC-3 IEC61937 type at burst {index}")
    return burst_bytes, bursts


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--cache-dir", type=Path)
    parser.add_argument("--skip-fault-profiles", action="store_true")
    args = parser.parse_args()

    if os.name != "nt":
        raise SystemExit("This convenience launcher targets native Windows. Use test-aurora-full-system-sim.sh on Linux.")

    root = Path(__file__).resolve().parents[2]
    output = (args.output_dir or (root / "artifacts/aurora-sim-windows")).resolve()
    cache = (args.cache_dir or (root / ".cache/aurora-sim-windows")).resolve()
    output.mkdir(parents=True, exist_ok=True)
    cache.mkdir(parents=True, exist_ok=True)
    work = output / "work"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)

    manifest_path = root / "config/external-components-v1.json"
    patch = root / "validation/immersive/omniphony-v0.5.2-low-latency-stdout.patch"
    moving_analyzer = root / "validation/immersive/aurora_joc_moving_evidence.py"
    virtual_analyzer = root / "validation/virtual-hardware/aurora_full_system_sim.py"
    pacer = root / "validation/virtual-hardware/pace_orender.py"
    telemetry_source = root / "validation/virtual-hardware/aurora_moving_telemetry.rs"
    for path in (manifest_path, patch, moving_analyzer, virtual_analyzer, pacer, telemetry_source):
        if not path.is_file():
            raise RuntimeError(f"Missing Aurora validation dependency: {path}")

    git = command("git", "Install Git for Windows: winget install -e --id Git.Git")
    ffmpeg = command("ffmpeg", "Install FFmpeg and reopen the terminal.")
    rustup = command("rustup", "Install Rustup: winget install -e --id Rustlang.Rustup")
    cargo = command("cargo", "Install Rustup and reopen the terminal.")
    python = sys.executable

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    components = {item["id"]: item for item in manifest["components"]}
    omnip = components["omniphony"]

    env = os.environ.copy()
    phase("prepare pinned Windows renderer dependencies")
    run([rustup, "toolchain", "install", "stable", "--profile", "minimal"], env=env)
    setup_asio(cache, env)
    if not env.get("LIBCLANG_PATH"):
        print("WARNING: LIBCLANG_PATH is not set. If bindgen fails, install LLVM: winget install -e --id LLVM.LLVM", file=sys.stderr)

    omnip_dir = cache / "Omniphony"
    clone_pinned(git, str(omnip["upstream"]), str(omnip["tested_version"]), str(omnip["pinned_commit"]), omnip_dir)
    renderer_dir = omnip_dir / "omniphony-renderer"
    run([git, "-C", str(renderer_dir), "apply", "--check", str(patch)])
    run([git, "-C", str(renderer_dir), "apply", str(patch)])

    omnip_target = cache / "omniphony-target"
    omnip_target.mkdir(parents=True, exist_ok=True)
    build_env = env.copy()
    build_env["CARGO_TARGET_DIR"] = str(omnip_target)
    run([
        cargo, "+stable", "build", "--release", "--manifest-path", str(renderer_dir / "Cargo.toml"), "-p", "omniphony-renderer"
    ], env=build_env)
    orender = omnip_target / "release/orender.exe"
    layout = omnip_dir / "layouts/7.1.4.yaml"
    if not orender.is_file() or not layout.is_file():
        raise RuntimeError(f"Missing Windows renderer build product: orender={orender.exists()} layout={layout.exists()}")

    phase("acquire verified Harletty Windows bridge")
    harletty_zip = cache / "harletty-bridge-v0.7.4-windows-x86_64.zip"
    harletty_dir = cache / "harletty-bridge-v0.7.4-windows"
    download_verified(HARLETTY_URL, harletty_zip, HARLETTY_ZIP_SHA, "Harletty Windows bridge archive")
    if harletty_dir.exists():
        shutil.rmtree(harletty_dir)
    with zipfile.ZipFile(harletty_zip) as zf:
        zf.extractall(harletty_dir)
    bridge_matches = list(harletty_dir.rglob("harletty_bridge.dll"))
    if len(bridge_matches) != 1:
        raise RuntimeError(f"Expected one harletty_bridge.dll, found {bridge_matches}")
    bridge = bridge_matches[0]

    phase("acquire checksum-pinned official Dolby carrier and derive exact suffix")
    dolby_zip = work / "audio.zip"
    source = work / DOLBY_NAME
    derived = work / "Living-Room-Atmos_after_first_au.ec3"
    download_verified(DOLBY_URL, dolby_zip, ZIP_SHA, "Dolby archive")
    with zipfile.ZipFile(dolby_zip) as zf:
        matches = [name for name in zf.namelist() if Path(name).name == DOLBY_NAME]
        if len(matches) != 1:
            raise RuntimeError(f"Expected exactly one {DOLBY_NAME!r}, found {matches!r}")
        source.write_bytes(zf.read(matches[0]))
    assert_sha(source, SOURCE_SHA, "Dolby source carrier")
    source_data = source.read_bytes()
    if len(source_data) <= FIRST_AU_BYTES or source_data[:2] != b"\x0b\x77":
        raise RuntimeError("Unexpected official E-AC-3 carrier shape")
    frame_size = 2 * ((((source_data[2] & 0x07) << 8) | source_data[3]) + 1)
    if frame_size != FIRST_AU_BYTES:
        raise RuntimeError(f"Unexpected first AU size: expected {FIRST_AU_BYTES}, got {frame_size}")
    derived.write_bytes(source_data[frame_size:])
    assert_sha(derived, DERIVED_SHA, "Derived moving JOC suffix")

    phase("wrap exact moving suffix as IEC61937 type 0x15")
    moving_iec = work / "moving.spdif"
    run([ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i", str(derived), "-map", "0:a:0", "-c:a", "copy", "-f", "spdif", str(moving_iec)])
    burst_bytes, bursts = parse_iec(moving_iec)
    print(f"AURORA-WINDOWS-IEC61937-PASS bursts={bursts} burst_bytes={burst_bytes}")

    phase("capture moving-object Harletty telemetry")
    harness = work / "telemetry-harness"
    (harness / "src").mkdir(parents=True)
    cargo_path = renderer_dir.as_posix()
    (harness / "Cargo.toml").write_text(
        "\n".join([
            "[package]",
            'name = "aurora-moving-joc-telemetry-windows"',
            'version = "0.1.0"',
            'edition = "2024"',
            "publish = false",
            "",
            "[dependencies]",
            'abi_stable = "0.11"',
            f'bridge_api = {{ path = "{cargo_path}/bridge_api" }}',
            f'spdif = {{ path = "{cargo_path}/spdif" }}',
            'serde_json = "1"',
            "",
        ]),
        encoding="utf-8",
    )
    shutil.copy2(telemetry_source, harness / "src/main.rs")
    telemetry = output / "aurora-bridge-telemetry.json"
    telemetry_env = env.copy()
    telemetry_env["CARGO_TARGET_DIR"] = str(cache / "windows-telemetry-target")
    run([
        cargo, "+stable", "run", "--quiet", "--release", "--manifest-path", str(harness / "Cargo.toml"), "--", str(bridge), str(moving_iec), str(telemetry)
    ], env=telemetry_env)
    if not telemetry.is_file():
        raise RuntimeError("Moving-object telemetry was not produced")

    phase("render full moving carrier to 7.1.4")
    unpaced = output / "aurora-moving-unpaced-7.1.4.f32"
    unpaced_log = output / "orender-unpaced.log"
    with unpaced_log.open("w", encoding="utf-8", errors="replace") as log_handle:
        result = subprocess.run([
            str(orender), str(moving_iec), "--bridge-path", str(bridge), "--enable-vbap", "--speaker-layout", str(layout),
            "--output-backend", "file", "--output-file", str(unpaced), "--output-file-format", "raw-f32"
        ], stdout=log_handle, stderr=subprocess.STDOUT, env=env, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"Unpaced Omniphony render failed ({result.returncode}); see {unpaced_log}")
    if not unpaced.is_file() or unpaced.stat().st_size % (12 * 4):
        raise RuntimeError("Unpaced render is missing or not whole 12-channel f32 frames")
    frames = unpaced.stat().st_size // (12 * 4)
    if frames % bursts or frames // bursts != 1536:
        raise RuntimeError(f"Unexpected render cadence: frames={frames} bursts={bursts}")
    print(f"AURORA-WINDOWS-7.1.4-SHAPE-PASS bursts={bursts} frames={frames}")

    phase("run media-paced full carrier through the real Windows renderer")
    paced = output / "aurora-moving-paced-7.1.4.f32"
    paced_log = output / "orender-paced.log"
    pacing = output / "pacing.json"
    run([
        python, str(pacer), "--orender", str(orender), "--bridge", str(bridge), "--layout", str(layout), "--carrier", str(moving_iec),
        "--unpaced-render", str(unpaced), "--paced-render", str(paced), "--log", str(paced_log), "--report", str(pacing)
    ], env=env)

    phase("run Aurora moving-JOC evidence analyzer")
    moving_evidence = output / "aurora-joc-moving-evidence.json"
    run([
        python, str(moving_analyzer), "analyze", "--input", str(derived), "--expected-sha256", DERIVED_SHA,
        "--provenance", "Windows local functional run; exact checksum-pinned no-reencode suffix from official Dolby DDP Online Delivery Kit v1.4.1; authoritative independent OpenJOC reference remains Linux CI",
        "--telemetry", str(telemetry), "--pcm", str(unpaced), "--pacing", str(pacing), "--sample-rate", "48000", "--channels", "12", "--output", str(moving_evidence)
    ], env=env)

    phase("drive merged deterministic virtual TDM16/DAC hardware model")
    virtual_report = output / "aurora-full-system-sim.json"
    run([
        python, str(virtual_analyzer), "run", "--render", str(paced), "--joc-evidence", str(moving_evidence), "--report", str(virtual_report),
        "--fault", "none", "--tdm-slots", "16", "--latency-frames", "256"
    ], env=env)

    if not args.skip_fault_profiles:
        phase("verify virtual hardware fails closed under injected faults")
        for fault in ("dropout", "channel-silence", "disconnect", "drift"):
            report = output / f"fault-{fault}.json"
            result = run([
                python, str(virtual_analyzer), "run", "--render", str(paced), "--joc-evidence", str(moving_evidence), "--report", str(report),
                "--fault", fault, "--tdm-slots", "16", "--latency-frames", "256"
            ], env=env, check=False)
            if result.returncode != 1:
                raise RuntimeError(f"Expected fail-closed exit 1 for fault {fault!r}, got {result.returncode}")
            payload = json.loads(report.read_text(encoding="utf-8"))
            if payload.get("verdict") != "fail" or not payload.get("failures"):
                raise RuntimeError(f"Fault {fault!r} did not emit explicit failure evidence")
            print(f"AURORA-WINDOWS-NEGATIVE-PASS fault={fault} failures={len(payload['failures'])}")

    final = json.loads(virtual_report.read_text(encoding="utf-8"))
    if final.get("verdict") != "pass":
        raise RuntimeError(f"Final AuroraSim verdict is {final.get('verdict')!r}")
    hardware = final["virtual_hardware"]
    health = final["channel_health"]
    print("\nAURORA-WINDOWS-FULL-SYSTEM-SIM-PASS")
    print(f"frames={hardware['sink_frames']} channels={len(health['active_channel_indices'])}/12 xruns={hardware['xrun_count']}")
    print(f"report={virtual_report}")
    print("Truth boundary: local Windows functional/simulation evidence; not physical eARC/UAC2/TDM/DAC or independent OpenJOC reference proof.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError, zipfile.BadZipFile) as exc:
        print(f"AURORA-WINDOWS-FULL-SYSTEM-SIM-ERROR: {exc}", file=sys.stderr)
        raise SystemExit(2)
