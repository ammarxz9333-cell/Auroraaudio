#!/usr/bin/env python3
"""Exercise Aurora's actual full eARC shell runner with legal synthetic IEC file.

The same shell entry point is used for captured TV eARC via --capture-device.
Here no real receiver, app, DAC or HDMI/HDCP is involved. The test must NOT
need Cargo, Git, Rustup, Harletty or Omniphony in channel-bed file mode.
"""
from __future__ import annotations

import array
import json
import os
from pathlib import Path
import subprocess
import tempfile

from test_streaming_bed import raw_pcm, encode

ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "validation/physical/run-live-earc-vbap.sh"


def main():
    ac3 = encode(raw_pcm(), "ac3")
    ddp = encode(raw_pcm(), "eac3")
    with tempfile.TemporaryDirectory(prefix="aurora-e2e-") as temp:
        folder = Path(temp)
        encoded = folder / "authorized-synthetic-test.spdif"
        out = folder / "surround-7.1.4.s32"
        workspace = folder / "runner"
        encoded.write_bytes(ac3 + ddp + ac3)
        env = dict(os.environ)
        env["AURORA_KEEP_LIVE_EARC_WORKDIR"] = "1"
        done = subprocess.run(
            [
                "bash", str(RUNNER), "--iec-input-file", str(encoded),
                "--decoder-mode", "auto-bed", "--output-file", str(out),
                "--work-dir", str(workspace),
            ], cwd=ROOT, env=env, capture_output=True, timeout=55,
        )
        assert done.returncode == 0, (
            f"Runner error={done.returncode}\n"
            + done.stdout[-1000:].decode(errors="replace")
            + done.stderr[-4000:].decode(errors="replace")
        )
        frame_bytes = 12 * 4
        pcm = out.read_bytes()
        assert len(pcm) > frame_bytes * 100
        assert len(pcm) % frame_bytes == 0
        samples = array.array("i")
        samples.frombytes(pcm)
        assert any(samples)
        evidence = json.loads((workspace / "artifacts/live-ingress-report.json").read_text())
        assert evidence["verdict"] == "pass", evidence["verdict_failures"]
        assert evidence["type_counts"]["AC3"] >= 10
        assert evidence["type_counts"]["EAC3_JOC_CANDIDATE"] >= 5
        assert evidence["relock_count"] >= 2
        assert "PHYSICAL RUN COMPLETE" in done.stderr.decode()
        assert not (workspace / "harletty-target").exists()
        assert not (workspace / "harletty-bridge").exists()
        assert not (workspace / "live-earc-harness").exists()
        print(
            f"AURORA-INTEGRATED-IEC-RUNNER-PASS "
            f"frames={len(pcm)//frame_bytes} "
            f"codec_relocks={evidence['relock_count']} "
            "native_atmos_proven=false physical_hardware_proven=false"
        )


if __name__ == "__main__":
    main()
