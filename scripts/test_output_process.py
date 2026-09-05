"""Exercise the real postprocessor executable without audio devices (Windows/Linux)."""
import array
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    binary = str(Path(sys.argv[1]).resolve())
    roles = ["front-left", "front-right", "front-center", "low-frequency-effects",
             "surround-left", "surround-right", "surround-back-left", "surround-back-right",
             "top-front-left", "top-front-right", "top-rear-left", "top-rear-right"]
    signal = array.array("f", [0.0]) * (4800 * 12)
    for frame in range(4800):
        signal[frame * 12 + 10] = 0.1 * math.sin(2 * math.pi * 1000 * frame / 48000)
    if sys.byteorder != "little":
        signal.byteswap()
    env = {key: value for key, value in os.environ.items() if not key.startswith("AURORA_")}

    def run(extra=None, data=None):
        return subprocess.run([binary], input=signal.tobytes() if data is None else data,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              env={**env, **(extra or {})}, timeout=30)

    reference = run()
    assert reference.returncode == 0, reference.stderr.decode()
    assert len(reference.stdout) > 0 and len(reference.stdout) % (40 * 12 * 4) == 0
    with tempfile.TemporaryDirectory(prefix="aurora-output-") as directory:
        path = Path(directory) / "calibration.json"
        config = {"schema_version": 1, "sample_rate": 48000, "channels": [
            {"role": role, "trim_db": 0, "delay_frames": 0, "invert_polarity": i == 10, "peq": []}
            for i, role in enumerate(roles)]}
        path.write_text(json.dumps(config))
        inverted = run({"AURORA_SPEAKER_CALIBRATION": str(path)})
        assert inverted.returncode == 0, inverted.stderr.decode()
        assert len(inverted.stdout) == len(reference.stdout)
        first = array.array("f"); first.frombytes(reference.stdout)
        second = array.array("f"); second.frombytes(inverted.stdout)
        if sys.byteorder != "little":
            first.byteswap(); second.byteswap()
        assert max(abs(value) for value in first[10::12]) > 0.01
        for i, (a, b) in enumerate(zip(first, second)):
            assert math.isfinite(a) and math.isfinite(b)
            assert abs(a + b if i % 12 == 10 else a - b) < 1e-6
        config["channels"][10]["role"] = "front-left"
        path.write_text(json.dumps(config))
        rejected = run({"AURORA_SPEAKER_CALIBRATION": str(path)})
        assert rejected.returncode != 0 and not rejected.stdout
    assert run({"AURORA_HEADROOM_DB": "NaN"}).returncode != 0
    assert run(data=b"\x00").returncode != 0
    print("PASS: real postprocessor stream, calibration routing, invalid config and truncated input; no hardware opened")


if __name__ == "__main__":
    main()
