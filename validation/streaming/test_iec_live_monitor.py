#!/usr/bin/env python3
"""Verify bounded on-the-fly IEC monitor: true bytes/no raw retention/fault gate."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from test_streaming_bed import encode, raw_pcm

MONITOR = Path(__file__).with_name("aurora_iec_live_monitor.py")


def run(pcm: bytes, *, allow_switches: bool):
    with tempfile.TemporaryDirectory() as dirname:
        path = Path(dirname) / "bounded-report.json"
        cmd = [
            sys.executable, str(MONITOR), "--report", str(path),
            "--retained-records", "3", "--minimum-valid-bursts", "2",
            "--require-locked-end", "--chunk-bytes", "4096",
        ]
        if not allow_switches:
            cmd.extend(["--max-relocks", "0"])
        result = subprocess.run(cmd, input=pcm, capture_output=True, timeout=35)
        assert path.exists(), result.stderr.decode(errors="replace")
        report = json.loads(path.read_text())
        return result, report, list(Path(dirname).iterdir())


def main():
    ac3 = encode(raw_pcm(), "ac3")
    ddp = encode(raw_pcm(), "eac3")
    stream = ac3 + ddp + ac3
    passed, report, files = run(stream, allow_switches=True)
    assert passed.returncode == 0, passed.stderr.decode(errors="replace")[-1000:]
    assert passed.stdout == stream
    assert report["verdict"] == "pass"
    assert report["relock_count"] >= 2
    assert report["retention"]["retained_bursts"] == 3
    assert report["runtime_monitor"]["raw_iec_capture_saved"] is False
    assert all(path.suffix == ".json" for path in files)
    print(f"PASS streaming bit-exact IEC {len(stream)} bytes, only bounded JSON evidence retained")

    rejected, bad_report, _ = run(stream, allow_switches=False)
    assert rejected.returncode != 0
    assert bad_report["verdict"] == "fail"
    assert len(rejected.stdout) < len(stream)
    print("PASS codec switch is fail-closed when only fixed-codec mode is requested")

    malformed = bytearray(stream)
    malformed[4] = 0x77  # Illegal burst kind.
    failed, report, _ = run(bytes(malformed), allow_switches=True)
    assert failed.returncode != 0
    assert report["invalid_bursts"] > 0
    assert len(failed.stdout) < len(malformed)
    print("PASS invalid IEC type blocked before forwarding to decoder")
    print("AURORA-IEC-LIVE-MONITOR-SOFTWARE-PASS")


if __name__ == "__main__":
    main()
